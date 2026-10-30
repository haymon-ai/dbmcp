//! Command-line arguments and their conversion into [`Config`].
//!
//! Hosts the flat top-level [`Arguments`] parsed by clap, the flag groups
//! it flattens ([`HttpArguments`], [`DatabaseArguments`], [`PiiArguments`],
//! [`SshArguments`]), the [`LogLevel`] selector, and [`flag_conventions`],
//! which shapes every flag by its value type. `TryFrom<&Arguments> for
//! Config` is the single builder: it converts each section and accumulates
//! their validation errors.

use std::any::TypeId;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::builder::BoolishValueParser;
use clap::{Arg, ArgAction, Args, Parser};
use dbmcp_config::{
    Config, ConfigError, ConfigErrors, DatabaseBackend, DatabaseConfig, HttpConfig, PiiCategory, PiiConfig,
    PiiOperator, Secret, SshConfig, StrictHostKeyChecking, Transport, ssh,
};

/// Log severity levels for the MCP server.
///
/// Maps directly to [`tracing::Level`] variants. Used as a
/// [`clap::ValueEnum`] for type-safe CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum LogLevel {
    /// Only errors.
    Error,
    /// Warnings and above.
    Warn,
    /// Informational and above (default).
    Info,
    /// Debug and above.
    Debug,
    /// All trace output.
    Trace,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
            Self::Trace => write!(f, "trace"),
        }
    }
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

/// Top-level CLI arguments parsed by clap.
///
/// Field order sets the `--help` heading order.
#[derive(Debug, Parser)]
#[command(about = "Database MCP Server", mut_args(flag_conventions))]
pub(crate) struct Arguments {
    /// MCP transport
    #[arg(
        long = "transport",
        env = "DBMCP_TRANSPORT",
        value_enum,
        default_value_t = Transport::Stdio,
        help_heading = "Transport"
    )]
    transport: Transport,

    /// HTTP transport flags, used only with `--transport http`.
    #[command(flatten)]
    http: HttpArguments,

    /// Database connection flags.
    #[command(flatten)]
    database: DatabaseArguments,

    /// PII flags.
    #[command(flatten)]
    pii: PiiArguments,

    /// SSH tunnel flags.
    #[command(flatten)]
    ssh: SshArguments,

    /// Log level
    #[arg(
        long = "log-level",
        env = "DBMCP_LOG_LEVEL",
        default_value_t = LogLevel::Info,
        ignore_case = true,
        help_heading = "Logging"
    )]
    pub(crate) log_level: LogLevel,
}

impl TryFrom<&Arguments> for Config {
    type Error = ConfigErrors;

    /// Builds the config, accumulating errors in db → http → pii → ssh order.
    ///
    /// HTTP options are converted only for `--transport http` and PII
    /// options only with `--pii`; otherwise their sections stay `None`.
    fn try_from(args: &Arguments) -> Result<Self, Self::Error> {
        let http = match args.transport {
            Transport::Stdio => Ok(None),
            Transport::Http => HttpConfig::try_from(&args.http).map(Some),
        };
        let pii = args.pii.enabled.then(|| PiiConfig::try_from(&args.pii)).transpose();
        match (
            DatabaseConfig::try_from(&args.database),
            http,
            pii,
            args.ssh.to_config(args.database.backend),
        ) {
            (Ok(database), Ok(http), Ok(pii), Ok(ssh)) => Ok(Self {
                transport: args.transport,
                database,
                http,
                pii,
                ssh,
            }),
            (database, http, pii, ssh) => {
                let mut errors: Vec<ConfigError> = Vec::new();
                if let Err(e) = database {
                    errors.extend(e);
                }
                if let Err(e) = http {
                    errors.extend(e);
                }
                if let Err(e) = pii {
                    errors.extend(e);
                }
                if let Err(e) = ssh {
                    errors.extend(e);
                }
                Err(ConfigErrors::from_vec(errors).expect("non-Ok branch implies at least one Err"))
            }
        }
    }
}

/// Applies the shared conventions for a flag's value type.
///
/// [`Secret`] flags hide their env value from `--help`. `bool` flags with
/// clap's default `SetTrue` action also accept an explicit value: bare
/// `--flag` means `true`; `--flag=<v>`, `--flag <v>`, and the env var accept
/// `true`/`false`, `yes`/`no`, `on`/`off`, and `1`/`0` in any case. Empty
/// and unknown values are rejected, so a blank or mistyped env var fails
/// startup instead of flipping a security setting. [`Arguments`] applies it
/// once, after flattening its flag groups, via
/// `#[command(mut_args(flag_conventions))]`, so every flag inherits it.
fn flag_conventions(arg: Arg) -> Arg {
    let value_type = arg.get_value_parser().type_id();
    if value_type == TypeId::of::<Secret>() {
        arg.hide_env_values(true)
    } else if value_type == TypeId::of::<bool>() && matches!(arg.get_action(), ArgAction::SetTrue) {
        let arg = arg
            .action(ArgAction::Set)
            .num_args(0..=1)
            .default_missing_value("true")
            .value_parser(BoolishValueParser::new());
        // `ArgAction::Set` drops the implicit `false` default of `SetTrue`.
        if arg.get_default_values().is_empty() {
            arg.default_value("false")
        } else {
            arg
        }
    } else {
        arg
    }
}

/// HTTP transport flags embedded in [`Arguments`].
///
/// Converted only for `--transport http`; ignored under other transports.
/// `host` and `port` use explicit `id = "http-*"` overrides so their
/// clap argument ids don't collide with the `host`/`port` fields in
/// [`DatabaseArguments`] when both are flattened into [`Arguments`].
#[derive(Debug, Args)]
#[command(next_help_heading = "HTTP Transport (with --transport http)")]
struct HttpArguments {
    /// Bind host for HTTP transport.
    #[arg(
        id = "http-host",
        long = "host",
        env = "DBMCP_HTTP_HOST",
        value_name = "HOST",
        default_value = HttpConfig::DEFAULT_HOST,
    )]
    host: String,

    /// Bind port for HTTP transport.
    #[arg(
        id = "http-port",
        long = "port",
        env = "DBMCP_HTTP_PORT",
        value_name = "PORT",
        default_value_t = HttpConfig::DEFAULT_PORT
    )]
    port: u16,

    /// Allowed browser origins (comma-separated, RFC 6454 `<scheme>://<host>[:<port>]` or `null`).
    ///
    /// Drives BOTH the CORS preflight allowlist AND the rmcp server-side
    /// Origin validator. Use `:*` to match any port or give an explicit
    /// port; entries without a port are deprecated. Malformed entries are
    /// rejected at startup. Pass an empty list
    /// (`--allowed-origins ""`) to disable server-side Origin validation
    /// and grant no CORS access — only do this for non-browser deployments.
    #[arg(
        long = "allowed-origins",
        env = "DBMCP_HTTP_ALLOWED_ORIGINS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_origins()
    )]
    allowed_origins: Vec<String>,

    /// Allowed host names (comma-separated).
    #[arg(
        long = "allowed-hosts",
        env = "DBMCP_HTTP_ALLOWED_HOSTS",
        value_delimiter = ',',
        default_values_t = HttpConfig::default_allowed_hosts()
    )]
    allowed_hosts: Vec<String>,

    /// Bearer token clients MUST send as `Authorization: Bearer <token>`.
    ///
    /// Required when binding a non-loopback host. Prefer the env var:
    /// command-line arguments are visible to other local users.
    #[arg(
        long = "auth-token",
        env = "DBMCP_HTTP_AUTH_TOKEN",
        value_name = "TOKEN",
        hide_env_values = true
    )]
    auth_token: Option<Secret>,

    /// Allow binding a non-loopback host without `DBMCP_HTTP_AUTH_TOKEN`.
    ///
    /// Only for deployments where a reverse proxy authenticates callers.
    /// Anyone who can reach the port gets full access to the database.
    #[arg(
        long = "dangerously-allow-unauthenticated",
        env = "DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED"
    )]
    dangerously_allow_unauthenticated: bool,
}

impl TryFrom<&HttpArguments> for HttpConfig {
    type Error = ConfigErrors;

    fn try_from(http: &HttpArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            host: http.host.clone(),
            port: http.port,
            // `--allowed-origins ""` arrives as `[""]`; dropping blanks turns
            // it into the empty list that disables Origin validation.
            allowed_origins: http
                .allowed_origins
                .iter()
                .filter(|origin| !origin.trim().is_empty())
                .cloned()
                .collect(),
            allowed_hosts: http.allowed_hosts.clone(),
            auth_token: http.auth_token.clone(),
            allow_unauthenticated: http.dangerously_allow_unauthenticated,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Database connection flags embedded in [`Arguments`].
#[derive(Debug, Args)]
#[command(next_help_heading = "Database")]
struct DatabaseArguments {
    /// Database backend
    #[arg(long = "db-backend", env = "DBMCP_DB_BACKEND", default_value_t = DatabaseConfig::DEFAULT_BACKEND)]
    backend: DatabaseBackend,

    /// Database host (as seen from the SSH server when tunnelling)
    #[arg(long = "db-host", env = "DBMCP_DB_HOST", default_value = DatabaseConfig::DEFAULT_HOST)]
    host: String,

    /// Database port (default: backend-dependent)
    #[arg(long = "db-port", env = "DBMCP_DB_PORT")]
    port: Option<u16>,

    /// Database user (default: backend-dependent)
    #[arg(long = "db-user", env = "DBMCP_DB_USER")]
    user: Option<String>,

    /// Database password
    #[arg(long = "db-password", env = "DBMCP_DB_PASSWORD", hide_env_values = true)]
    password: Option<Secret>,

    /// Database name or `SQLite` file path
    #[arg(long = "db-name", env = "DBMCP_DB_NAME")]
    name: Option<String>,

    /// Character set (MySQL/MariaDB only)
    #[arg(long = "db-charset", env = "DBMCP_DB_CHARSET")]
    charset: Option<String>,

    /// Enable SSL for database connection
    #[arg(
        long = "db-ssl",
        env = "DBMCP_DB_SSL",
        default_value_t = DatabaseConfig::DEFAULT_SSL,
    )]
    ssl: bool,

    /// Path to CA certificate
    #[arg(long = "db-ssl-ca", env = "DBMCP_DB_SSL_CA")]
    ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(long = "db-ssl-cert", env = "DBMCP_DB_SSL_CERT")]
    ssl_cert: Option<String>,

    /// Path to a client key
    #[arg(long = "db-ssl-key", env = "DBMCP_DB_SSL_KEY")]
    ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(
        long = "db-ssl-verify-cert",
        env = "DBMCP_DB_SSL_VERIFY_CERT",
        default_value_t = DatabaseConfig::DEFAULT_SSL_VERIFY_CERT,
    )]
    ssl_verify_cert: bool,

    /// Enable read-only mode
    #[arg(
        long = "db-read-only",
        env = "DBMCP_DB_READ_ONLY",
        default_value_t = DatabaseConfig::DEFAULT_READ_ONLY,
    )]
    read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "db-max-pool-size",
        env = "DBMCP_DB_MAX_POOL_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    max_pool_size: u32,

    /// Connection timeout in seconds
    #[arg(
        long = "db-connection-timeout",
        env = "DBMCP_DB_CONNECTION_TIMEOUT",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    connection_timeout: Option<u64>,

    /// Query execution timeout in seconds
    #[arg(
        long = "db-query-timeout",
        env = "DBMCP_DB_QUERY_TIMEOUT",
        default_value_t = DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS,
        value_parser = clap::value_parser!(u64)
    )]
    query_timeout: u64,

    /// Maximum items returned in a single paginated tool response
    #[arg(
        long = "db-page-size",
        env = "DBMCP_DB_PAGE_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_PAGE_SIZE,
        value_parser = clap::value_parser!(u16).range(1..=i64::from(DatabaseConfig::MAX_PAGE_SIZE)),
    )]
    page_size: u16,
}

impl TryFrom<&DatabaseArguments> for DatabaseConfig {
    type Error = ConfigErrors;

    fn try_from(db: &DatabaseArguments) -> Result<Self, Self::Error> {
        let backend = db.backend;
        let candidate = Self {
            backend,
            host: db.host.clone(),
            port: db.port.unwrap_or_else(|| backend.default_port()),
            user: db.user.clone().unwrap_or_else(|| backend.default_user().into()),
            password: db.password.clone(),
            name: db.name.clone(),
            charset: db.charset.clone(),
            ssl: db.ssl,
            ssl_ca: db.ssl_ca.clone(),
            ssl_cert: db.ssl_cert.clone(),
            ssl_key: db.ssl_key.clone(),
            ssl_verify_cert: db.ssl_verify_cert,
            read_only: db.read_only,
            max_pool_size: db.max_pool_size,
            connection_timeout: db.connection_timeout,
            query_timeout: db.query_timeout,
            page_size: db.page_size,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// PII flags embedded in [`Arguments`].
#[derive(Debug, Args)]
#[command(next_help_heading = "PII")]
struct PiiArguments {
    /// Enable PII redaction of query tool output
    #[arg(long = "pii", env = "DBMCP_PII_ENABLE")]
    enabled: bool,

    /// Operator applied to detected PII spans
    #[arg(
        long = "pii-operator",
        env = "DBMCP_PII_OPERATOR",
        default_value_t = PiiConfig::DEFAULT_OPERATOR,
    )]
    operator: PiiOperator,

    /// Comma-separated PII categories the analyzer should cover
    /// (personal, financial, government, contact, network, digital-identity, crypto)
    #[arg(
        long = "pii-categories",
        env = "DBMCP_PII_CATEGORIES",
        value_delimiter = ',',
        num_args = 1..,
    )]
    categories: Option<Vec<PiiCategory>>,

    /// Enable the optional ML/NER pass (person & location detection)
    #[arg(
        long = "pii-ner",
        env = "DBMCP_PII_NER_ENABLE",
        default_value_t = PiiConfig::DEFAULT_NER_ENABLED,
    )]
    ner_enabled: bool,

    /// Path to the NER model directory (required with --pii-ner)
    #[arg(long = "pii-ner-model", env = "DBMCP_PII_NER_MODEL")]
    ner_model: Option<PathBuf>,
}

impl TryFrom<&PiiArguments> for PiiConfig {
    type Error = ConfigErrors;

    fn try_from(args: &PiiArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            operator: args.operator,
            categories: args.categories.clone(),
            ner_enabled: args.ner_enabled,
            ner_model: args.ner_model.clone(),
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// SSH tunnel flags embedded in [`Arguments`].
///
/// Every argument uses an explicit `id = "ssh-*"` so its clap id doesn't
/// collide with the same-named fields in [`DatabaseArguments`]. `port`, `strict_host_key_checking`, and `keepalive_interval` stay
/// `Option` so [`SshArguments::to_config`] can tell whether they were set;
/// their defaults are applied there.
#[derive(Debug, Args)]
#[command(next_help_heading = "SSH")]
struct SshArguments {
    /// SSH server host; enables the tunnel when set
    #[arg(id = "ssh-host", long = "ssh-host", value_name = "HOST", env = "DBMCP_SSH_HOST")]
    host: Option<String>,

    /// SSH server port
    #[arg(
        id = "ssh-port",
        long = "ssh-port",
        value_name = "PORT",
        env = "DBMCP_SSH_PORT",
        value_parser = clap::value_parser!(u16).range(1..),
    )]
    port: Option<u16>,

    /// SSH user
    #[arg(id = "ssh-user", long = "ssh-user", value_name = "USER", env = "DBMCP_SSH_USER")]
    user: Option<String>,

    /// SSH password
    #[arg(
        id = "ssh-password",
        long = "ssh-password",
        value_name = "PASSWORD",
        env = "DBMCP_SSH_PASSWORD",
        hide_env_values = true
    )]
    password: Option<Secret>,

    /// Path to SSH private key (OpenSSH, PKCS#1, PKCS#8, SEC1)
    #[arg(id = "ssh-key", long = "ssh-key", value_name = "PATH", env = "DBMCP_SSH_KEY")]
    key: Option<PathBuf>,

    /// Passphrase for the SSH private key
    #[arg(
        id = "ssh-key-passphrase",
        long = "ssh-key-passphrase",
        value_name = "PASSPHRASE",
        env = "DBMCP_SSH_KEY_PASSPHRASE",
        hide_env_values = true
    )]
    key_passphrase: Option<Secret>,

    /// Path to known_hosts file (default: ~/.ssh/known_hosts)
    #[allow(clippy::doc_markdown, reason = "doc comment is the verbatim --help text")]
    #[arg(
        id = "ssh-known-hosts",
        long = "ssh-known-hosts",
        value_name = "PATH",
        env = "DBMCP_SSH_KNOWN_HOSTS"
    )]
    known_hosts: Option<PathBuf>,

    /// Host key policy: yes rejects unknown hosts, accept-new trusts on first use, no disables verification (insecure)
    #[arg(
        id = "ssh-strict-host-key-checking",
        long = "ssh-strict-host-key-checking",
        value_name = "MODE",
        env = "DBMCP_SSH_STRICT_HOST_KEY_CHECKING"
    )]
    strict_host_key_checking: Option<StrictHostKeyChecking>,

    /// SSH keepalive interval in seconds (0 disables)
    #[arg(
        id = "ssh-keepalive-interval",
        long = "ssh-keepalive-interval",
        value_name = "SECONDS",
        env = "DBMCP_SSH_KEEPALIVE_INTERVAL"
    )]
    keepalive_interval: Option<u64>,

    /// Local tunnel port (default: ephemeral)
    #[arg(
        id = "ssh-local-port",
        long = "ssh-local-port",
        value_name = "PORT",
        env = "DBMCP_SSH_LOCAL_PORT",
        value_parser = clap::value_parser!(u16).range(1..),
    )]
    local_port: Option<u16>,
}

impl SshArguments {
    /// Resolves the tunnel settings; `Ok(None)` when `DBMCP_SSH_HOST` is unset.
    ///
    /// Uses the process home directory for `~` expansion and the default
    /// `known_hosts` path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] when other `DBMCP_SSH_*` options are set
    /// without a host, or when [`SshConfig::validate`] fails.
    fn to_config(&self, backend: DatabaseBackend) -> Result<Option<SshConfig>, ConfigErrors> {
        self.resolve(backend, ssh::home_dir().as_deref())
    }

    /// Resolves the tunnel settings against an explicit `home` directory.
    fn resolve(&self, backend: DatabaseBackend, home: Option<&Path>) -> Result<Option<SshConfig>, ConfigErrors> {
        let Some(host) = &self.host else {
            return if self.any_set() {
                Err(ConfigError::MissingSshHost.into())
            } else {
                Ok(None)
            };
        };

        let known_hosts = match &self.known_hosts {
            Some(path) => Some(ssh::expand_tilde(path, home)),
            None => home.map(|home| home.join(".ssh").join("known_hosts")),
        };
        let candidate = SshConfig {
            host: host.clone(),
            port: self.port.unwrap_or(SshConfig::DEFAULT_PORT),
            user: self.user.clone().unwrap_or_default(),
            password: self.password.clone(),
            key: self.key.as_deref().map(|key| ssh::expand_tilde(key, home)),
            key_passphrase: self.key_passphrase.clone(),
            known_hosts,
            strict_host_key_checking: self
                .strict_host_key_checking
                .unwrap_or(SshConfig::DEFAULT_STRICT_HOST_KEY_CHECKING),
            keepalive_interval: Duration::from_secs(
                self.keepalive_interval
                    .unwrap_or(SshConfig::DEFAULT_KEEPALIVE_INTERVAL_SECS),
            ),
            local_port: self.local_port,
        };
        candidate.validate(backend)?;
        Ok(Some(candidate))
    }

    /// Returns `true` when any option other than `host` is set.
    fn any_set(&self) -> bool {
        self.port.is_some()
            || self.user.is_some()
            || self.password.is_some()
            || self.key.is_some()
            || self.key_passphrase.is_some()
            || self.known_hosts.is_some()
            || self.strict_host_key_checking.is_some()
            || self.keepalive_interval.is_some()
            || self.local_port.is_some()
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::Arguments;
    use dbmcp_config::{
        Config, ConfigError, DatabaseBackend, DatabaseConfig, HttpConfig, PiiCategory, PiiConfig, PiiOperator, Secret,
        SshConfig, StrictHostKeyChecking, Transport,
    };

    const TEST_TOKEN: &str = "test-token-123";

    /// Parses `args` with the real parser, ignoring inherited env vars.
    #[track_caller]
    fn parse(args: &[&str]) -> Arguments {
        try_parse(args).expect("valid arguments")
    }

    /// Fallible [`parse`] for tests that expect clap to reject `args`.
    fn try_parse(args: &[&str]) -> Result<Arguments, clap::Error> {
        // SAFETY: no test in this crate writes these env vars; removing them
        // keeps inherited host values out of clap.
        unsafe {
            for arg in Arguments::command().get_arguments() {
                if let Some(env) = arg.get_env() {
                    std::env::remove_var(env);
                }
            }
        }
        Arguments::try_parse_from(std::iter::once("dbmcp").chain(args.iter().copied()))
    }

    #[test]
    fn transport_defaults_to_stdio_without_http_or_pii() {
        let config = Config::try_from(&parse(&[])).expect("default args are valid");
        assert_eq!(config.transport, Transport::Stdio);
        assert!(config.http.is_none());
        assert!(config.pii.is_none());
    }

    #[test]
    fn transport_http_builds_http_config() {
        let config = Config::try_from(&parse(&["--transport", "http", "--port", "9100"])).expect("valid http args");
        assert_eq!(config.transport, Transport::Http);
        assert_eq!(config.http.expect("http config built").port, 9100);
    }

    #[test]
    fn transport_rejects_unknown_value_and_legacy_subcommands() {
        assert!(try_parse(&["--transport", "sse"]).is_err());
        for legacy in ["stdio", "http", "version"] {
            assert!(try_parse(&[legacy]).is_err(), "{legacy} must be rejected");
        }
    }

    #[test]
    fn invalid_http_options_are_ignored_under_stdio() {
        let config = Config::try_from(&parse(&["--host", ""])).expect("http options must be ignored under stdio");
        assert!(config.http.is_none());
    }

    #[test]
    fn pii_enable_builds_pii_config() {
        let config = Config::try_from(&parse(&["--pii", "--pii-operator", "mask"])).expect("valid pii args");
        assert_eq!(config.pii.expect("pii config built").operator, PiiOperator::Mask);
    }

    #[test]
    fn invalid_pii_options_are_ignored_without_pii() {
        let config = Config::try_from(&parse(&["--pii-ner"])).expect("pii options must be ignored without --pii");
        assert!(config.pii.is_none());
    }

    #[test]
    fn config_errors_accumulate_in_db_http_pii_ssh_order() {
        let args = parse(&[
            "--transport",
            "http",
            "--db-backend",
            "sqlite",
            "--host",
            "",
            "--pii",
            "--pii-ner",
            "--ssh-port",
            "2222",
        ]);
        let errors = Config::try_from(&args).expect_err("multi-section misconfig must fail");
        assert_eq!(errors.len(), 4, "{errors:?}");
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(errors[1], ConfigError::EmptyHttpHost));
        assert!(matches!(errors[2], ConfigError::PiiNerModelMissing));
        assert!(matches!(errors[3], ConfigError::MissingSshHost));
    }

    #[test]
    fn database_errors_accumulate_in_field_order() {
        let args = parse(&[
            "--db-backend",
            "sqlite",
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
            "--db-ssl-cert",
            "/nonexistent/cert.pem",
            "--db-ssl-key",
            "/nonexistent/key.pem",
        ]);
        let errors = Config::try_from(&args).expect_err("multi-field misconfig must fail");
        assert_eq!(errors.len(), 4);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(&errors[1], ConfigError::FileNotFound(name, _) if name == "DBMCP_DB_SSL_CA"));
        assert!(matches!(&errors[2], ConfigError::FileNotFound(name, _) if name == "DBMCP_DB_SSL_CERT"));
        assert!(matches!(&errors[3], ConfigError::FileNotFound(name, _) if name == "DBMCP_DB_SSL_KEY"));
    }

    #[test]
    fn secret_env_values_are_hidden_from_help() {
        for arg in Arguments::command().get_arguments() {
            let Some(env) = arg.get_env().and_then(|env| env.to_str()) else {
                continue;
            };
            if ["PASSWORD", "PASSPHRASE", "TOKEN", "SECRET"]
                .iter()
                .any(|word| env.contains(word))
            {
                assert!(arg.is_hide_env_values_set(), "{env} must not print its value in --help");
            }
        }
    }

    #[test]
    fn help_headings_follow_field_order() {
        let help = Arguments::command().render_help().to_string();
        let positions: Vec<usize> = [
            "Transport:",
            "HTTP Transport (with --transport http):",
            "Database:",
            "PII:",
            "SSH:",
            "Logging:",
        ]
        .iter()
        .map(|heading| {
            help.find(heading)
                .unwrap_or_else(|| panic!("{heading} missing:\n{help}"))
        })
        .collect();
        assert!(positions.is_sorted(), "headings out of order:\n{help}");
    }

    #[test]
    fn help_documents_db_page_size_flag() {
        let help = Arguments::command().render_long_help().to_string();
        assert!(
            help.contains("--db-page-size"),
            "help missing --db-page-size flag:\n{help}"
        );
        assert!(
            help.contains("DBMCP_DB_PAGE_SIZE"),
            "help missing DBMCP_DB_PAGE_SIZE env binding:\n{help}"
        );
        assert!(help.contains("[default: 100]"), "help missing [default: 100]:\n{help}");
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        assert!(parse(&[]).database.read_only);
    }

    #[test]
    fn db_query_timeout_default_matches_database_config_default() {
        let config = DatabaseConfig::try_from(&parse(&[]).database).expect("valid db args");
        assert_eq!(config.query_timeout, DatabaseConfig::default().query_timeout);
    }

    #[test]
    fn db_query_timeout_zero_passes_through() {
        let args = parse(&["--db-query-timeout", "0"]);
        let config = DatabaseConfig::try_from(&args.database).expect("valid db args");
        assert_eq!(config.query_timeout, 0);
    }

    #[test]
    fn db_args_populate_database_config() {
        let args = parse(&["--db-backend", "postgres", "--db-user", "pg", "--db-name", "app"]);
        assert_eq!(args.database.backend, DatabaseBackend::Postgres);
        assert_eq!(args.database.user.as_deref(), Some("pg"));
        assert_eq!(args.database.name.as_deref(), Some("app"));

        let config = DatabaseConfig::try_from(&args.database).expect("valid postgres args");
        assert_eq!(config.backend, DatabaseBackend::Postgres);
        assert_eq!(config.user, "pg");
        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn clap_default_yields_loopback_origins() {
        // Also guards that HttpConfig::try_from invokes validate: a future rule
        // firing on default args fails here.
        let config = HttpConfig::try_from(&parse(&[]).http).expect("default args are valid");
        assert_eq!(config.allowed_origins, HttpConfig::default_allowed_origins());
    }

    #[test]
    fn try_from_rejects_empty_host() {
        let args = parse(&["--host", ""]);
        let errors = HttpConfig::try_from(&args.http).expect_err("empty host must be rejected by validate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn try_from_rejects_whitespace_host() {
        let args = parse(&["--host", "   "]);
        let errors = HttpConfig::try_from(&args.http).expect_err("whitespace host must be rejected by validate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn clap_accepts_default_host() {
        assert_eq!(parse(&[]).http.host, HttpConfig::DEFAULT_HOST);
    }

    #[test]
    fn empty_allowed_origins_flag_yields_empty_list() {
        let args = parse(&["--allowed-origins", ""]);
        let config = HttpConfig::try_from(&args.http).expect("empty origins must validate");
        assert!(config.allowed_origins.is_empty(), "{:?}", config.allowed_origins);
    }

    #[test]
    fn try_from_rejects_malformed_origin() {
        let args = parse(&["--allowed-origins", "http://localhost:*,localhost"]);
        let errors = HttpConfig::try_from(&args.http).expect_err("malformed origin must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::InvalidAllowedOrigin(entry) if entry == "localhost"))
        );
    }

    #[test]
    fn try_from_rejects_non_loopback_without_token() {
        let args = parse(&["--host", "0.0.0.0"]);
        let errors = HttpConfig::try_from(&args.http).expect_err("0.0.0.0 without token must be rejected");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::UnauthenticatedNonLoopbackBind(_)))
        );
    }

    #[test]
    fn try_from_accepts_non_loopback_with_token() {
        let args = parse(&["--host", "0.0.0.0", "--auth-token", TEST_TOKEN]);
        let config = HttpConfig::try_from(&args.http).expect("token must allow non-loopback bind");
        assert_eq!(config.auth_token.as_ref().map(Secret::expose), Some(TEST_TOKEN));
    }

    #[test]
    fn try_from_accepts_non_loopback_with_opt_out() {
        let args = parse(&["--host", "0.0.0.0", "--dangerously-allow-unauthenticated"]);
        let config = HttpConfig::try_from(&args.http).expect("opt-out must allow non-loopback bind");
        assert!(config.allow_unauthenticated);
        assert!(config.auth_token.is_none());
    }

    fn try_parse_with_page_size(value: &str) -> Result<u16, clap::Error> {
        try_parse(&["--db-page-size", value]).map(|args| args.database.page_size)
    }

    #[test]
    fn clap_rejects_page_size_zero() {
        assert!(try_parse_with_page_size("0").is_err());
    }

    #[test]
    fn clap_rejects_page_size_above_max() {
        assert!(try_parse_with_page_size("501").is_err());
    }

    #[test]
    fn clap_rejects_negative_page_size() {
        assert!(try_parse_with_page_size("-1").is_err());
    }

    #[test]
    fn clap_rejects_non_integer_page_size() {
        assert!(try_parse_with_page_size("abc").is_err());
    }

    #[test]
    fn clap_accepts_page_size_at_min() {
        assert_eq!(try_parse_with_page_size("1").expect("valid page size"), 1);
    }

    #[test]
    fn clap_accepts_page_size_at_max() {
        assert_eq!(try_parse_with_page_size("500").expect("valid page size"), 500);
    }

    #[test]
    fn clap_default_page_size_is_100() {
        assert_eq!(parse(&[]).database.page_size, 100);
    }

    #[test]
    fn clap_pii_operator_default_is_replace() {
        let args = parse(&[]);
        assert!(!args.pii.enabled);
        assert_eq!(args.pii.operator, PiiOperator::Replace);
    }

    #[test]
    fn clap_pii_operator_accepts_each_variant() {
        for (raw, expected) in [
            ("replace", PiiOperator::Replace),
            ("mask", PiiOperator::Mask),
            ("redact", PiiOperator::Redact),
            ("hash", PiiOperator::Hash),
        ] {
            let args = parse(&["--pii-operator", raw]);
            assert_eq!(args.pii.operator, expected, "operator {raw} must parse");
        }
    }

    #[test]
    fn clap_rejects_unknown_pii_operator() {
        let err = try_parse(&["--pii-operator", "xyz"]).expect_err("unknown operator must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("xyz"), "error must name the offending value: {msg}");
        for accepted in ["replace", "mask", "redact", "hash"] {
            assert!(
                msg.contains(accepted),
                "error must list accepted value '{accepted}': {msg}"
            );
        }
    }

    #[test]
    fn pii_config_validate_is_called_from_try_from_path() {
        // Structural-presence guard: ensures PiiConfig::try_from invokes
        // PiiConfig::validate. When a future rule fires on default args,
        // this test deliberately fails — flagging the contributor.
        let args = parse(&[]);
        PiiConfig::try_from(&args.pii).expect("default pii args must validate");
    }

    #[test]
    fn clap_pii_flags_grouped_under_pii_heading() {
        let help = Arguments::command().render_help().to_string();
        let pii_pos = help.find("PII:").expect("PII heading must be present in --help");
        let flag_pos = help.find("--pii ").expect("--pii must be present in --help");
        assert!(
            pii_pos < flag_pos,
            "PII heading must precede --pii in help output:\n{help}"
        );
    }

    #[test]
    fn clap_pii_categories_default_unset() {
        let args = parse(&[]);
        assert!(args.pii.categories.is_none(), "categories must default to None");
    }

    #[test]
    fn clap_pii_categories_comma_separated() {
        let args = parse(&["--pii-categories", "financial,government"]);
        let cats = args.pii.categories.expect("categories set");
        assert_eq!(cats, vec![PiiCategory::Financial, PiiCategory::Government]);
    }

    #[test]
    fn clap_pii_categories_kebab_digital_identity() {
        let args = parse(&["--pii-categories", "digital-identity"]);
        let cats = args.pii.categories.expect("categories set");
        assert_eq!(cats, vec![PiiCategory::DigitalIdentity]);
    }

    #[test]
    fn clap_rejects_unknown_pii_category() {
        let err = try_parse(&["--pii-categories", "healthcare"]).expect_err("healthcare is not in v1");
        let msg = err.to_string();
        assert!(msg.contains("healthcare"), "error must name the offending value: {msg}");
    }

    #[test]
    fn pii_config_categories_empty_vec_errors_via_try_from() {
        // clap normally never produces an empty Vec (num_args=1..), but the validator
        // still defends against direct PiiConfig construction with Some(empty).
        let cfg = PiiConfig {
            operator: PiiOperator::Replace,
            categories: Some(Vec::new()),
            ..PiiConfig::default()
        };
        let errors = cfg.validate().expect_err("empty categories must error");
        assert!(
            errors.iter().any(|e| matches!(e, ConfigError::PiiCategoriesEmpty)),
            "expected PiiCategoriesEmpty in {errors:?}"
        );
    }

    #[test]
    fn clap_pii_ner_default_off() {
        let args = parse(&[]);
        assert!(!args.pii.ner_enabled, "ner must default off");
        assert!(args.pii.ner_model.is_none(), "ner model must default unset");
    }

    #[test]
    fn clap_pii_ner_flags_parse_and_convert() {
        let args = parse(&["--pii-ner", "true", "--pii-ner-model", "/models/ner"]);
        let cfg = PiiConfig::try_from(&args.pii).expect("ner config validates");
        assert!(cfg.ner_enabled);
        assert_eq!(cfg.ner_model, Some(std::path::PathBuf::from("/models/ner")));
    }

    #[test]
    fn pii_ner_enabled_without_model_errors_via_try_from() {
        let args = parse(&["--pii-ner", "true"]);
        let errors = PiiConfig::try_from(&args.pii).expect_err("ner without model must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::PiiNerModelMissing)));
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    fn resolve(args: &[&str], home: Option<&Path>) -> Result<Option<SshConfig>, dbmcp_config::ConfigErrors> {
        parse(args).ssh.resolve(DatabaseBackend::Postgres, home)
    }

    #[test]
    fn ssh_disabled_without_any_option() {
        assert!(resolve(&[], Some(&home())).expect("valid").is_none());
    }

    #[test]
    fn ssh_options_without_host_are_rejected() {
        for args in [
            &["--ssh-port", "2222"][..],
            &["--ssh-user", "deploy"],
            &["--ssh-password", "pw"],
            &["--ssh-key", "/k"],
            &["--ssh-key-passphrase", "pp"],
            &["--ssh-known-hosts", "/kh"],
            &["--ssh-strict-host-key-checking", "no"],
            &["--ssh-keepalive-interval", "0"],
            &["--ssh-local-port", "4000"],
        ] {
            let errors = resolve(args, Some(&home())).expect_err("orphan option must fail");
            assert_eq!(errors.len(), 1, "{args:?}");
            assert_eq!(
                errors[0].to_string(),
                "DBMCP_SSH_HOST is required when other DBMCP_SSH_* options are set",
                "{args:?}"
            );
        }
    }

    #[test]
    fn ssh_defaults_are_resolved() {
        let config = resolve(
            &["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"],
            Some(&home()),
        )
        .expect("valid")
        .expect("tunnel enabled");
        assert_eq!(config.host, "bastion");
        assert_eq!(config.port, 22);
        assert_eq!(config.user, "deploy");
        assert_eq!(config.password.as_ref().map(dbmcp_config::Secret::expose), Some("pw"));
        assert_eq!(config.known_hosts, Some(home().join(".ssh").join("known_hosts")));
        assert_eq!(config.strict_host_key_checking, StrictHostKeyChecking::Yes);
        assert_eq!(config.keepalive_interval, Duration::from_secs(30));
        assert_eq!(config.local_port, None);
    }

    #[test]
    fn ssh_explicit_values_override_defaults() {
        let config = resolve(
            &[
                "--ssh-host",
                "bastion",
                "--ssh-port",
                "2222",
                "--ssh-user",
                "deploy",
                "--ssh-password",
                "pw",
                "--ssh-known-hosts",
                "~/.ssh/custom_hosts",
                "--ssh-strict-host-key-checking",
                "accept-new",
                "--ssh-keepalive-interval",
                "0",
                "--ssh-local-port",
                "4000",
            ],
            Some(&home()),
        )
        .expect("valid")
        .expect("tunnel enabled");
        assert_eq!(config.port, 2222);
        assert_eq!(config.known_hosts, Some(home().join(".ssh").join("custom_hosts")));
        assert_eq!(config.strict_host_key_checking, StrictHostKeyChecking::AcceptNew);
        assert_eq!(config.keepalive_interval, Duration::ZERO);
        assert_eq!(config.local_port, Some(4000));
    }

    #[test]
    fn ssh_key_path_is_tilde_expanded() {
        let errors = resolve(
            &[
                "--ssh-host",
                "bastion",
                "--ssh-user",
                "deploy",
                "--ssh-key",
                "~/.ssh/missing_key",
            ],
            Some(&home()),
        )
        .expect_err("missing key file must fail");
        let expected = home().join(".ssh").join("missing_key");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                ConfigError::FileNotFound(name, path) if name == "DBMCP_SSH_KEY" && *path == expected.display().to_string()
            )),
            "{errors:?}"
        );
    }

    #[test]
    fn ssh_known_hosts_unresolved_without_home() {
        let errors = resolve(
            &["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"],
            None,
        )
        .expect_err("no home must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshKnownHosts)));

        let config = resolve(
            &[
                "--ssh-host",
                "bastion",
                "--ssh-user",
                "deploy",
                "--ssh-password",
                "pw",
                "--ssh-strict-host-key-checking",
                "no",
            ],
            None,
        )
        .expect("mode no needs no known_hosts")
        .expect("tunnel enabled");
        assert_eq!(config.known_hosts, None);
    }

    #[test]
    fn ssh_missing_user_and_auth_accumulate() {
        let errors = resolve(&["--ssh-host", "bastion"], Some(&home())).expect_err("must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshUser)));
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshAuth)));
    }

    #[test]
    fn ssh_rejected_for_sqlite_backend() {
        let errors = parse(&["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"])
            .ssh
            .resolve(DatabaseBackend::Sqlite, Some(&home()))
            .expect_err("sqlite must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::SshSqliteUnsupported)));
    }

    #[test]
    fn ssh_empty_host_is_rejected() {
        let errors = resolve(
            &["--ssh-host", "", "--ssh-user", "deploy", "--ssh-password", "pw"],
            Some(&home()),
        )
        .expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptySshHost)));
    }

    #[test]
    fn clap_rejects_ssh_port_zero_and_local_port_zero() {
        assert!(try_parse(&["--ssh-port", "0"]).is_err());
        assert!(try_parse(&["--ssh-local-port", "0"]).is_err());
    }

    #[test]
    fn clap_strict_host_key_checking_rejects_openssh_aliases() {
        for value in ["ask", "off"] {
            assert!(
                try_parse(&["--ssh-strict-host-key-checking", value]).is_err(),
                "{value} must be rejected"
            );
        }
    }

    #[test]
    fn bool_flags_accept_bare_and_explicit_values() {
        type Field = fn(&Arguments) -> bool;
        let flags: [(&str, Field); 6] = [
            ("--db-ssl", |args| args.database.ssl),
            ("--db-ssl-verify-cert", |args| args.database.ssl_verify_cert),
            ("--db-read-only", |args| args.database.read_only),
            ("--pii", |args| args.pii.enabled),
            ("--pii-ner", |args| args.pii.ner_enabled),
            ("--dangerously-allow-unauthenticated", |args| {
                args.http.dangerously_allow_unauthenticated
            }),
        ];
        for (flag, get) in flags {
            let mut cases = vec![
                (vec![flag.to_owned()], true),
                (vec![flag.to_owned(), "false".to_owned()], false),
            ];
            for value in ["true", "1", "yes", "on", "True", "TRUE"] {
                cases.push((vec![format!("{flag}={value}")], true));
            }
            for value in ["false", "0", "no", "off", "False", "FALSE"] {
                cases.push((vec![format!("{flag}={value}")], false));
            }
            for (args, expected) in cases {
                let args: Vec<&str> = args.iter().map(String::as_str).collect();
                let parsed = try_parse(&args).unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
                assert_eq!(get(&parsed), expected, "{args:?}");
            }
            for value in ["", "disabled", "nope", "never", "2"] {
                let arg = format!("{flag}={value}");
                assert!(try_parse(&[&arg]).is_err(), "{arg:?} must be rejected");
            }
        }
    }

    #[test]
    fn clap_ssh_flags_grouped_under_ssh_heading() {
        let help = Arguments::command().render_help().to_string();
        let ssh_pos = help.find("SSH:").expect("SSH heading must be present in --help");
        let flag_pos = help.find("--ssh-host").expect("--ssh-host must be present in --help");
        assert!(ssh_pos < flag_pos, "SSH heading must precede --ssh-host:\n{help}");
    }

    #[test]
    fn db_host_help_mentions_ssh_tunnel() {
        let help = Arguments::command().render_help().to_string();
        assert!(
            help.contains("Database host (as seen from the SSH server when tunnelling)"),
            "{help}"
        );
    }
}
