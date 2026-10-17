//! Common building blocks reused across transport subcommands.
//!
//! Hosts [`DatabaseArguments`], [`PiiArguments`], and [`SshArguments`] —
//! clap argument groups bundling every `--db-*`, `--pii-*`, and `--ssh-*`
//! flag so each is defined exactly once and embedded via
//! `#[command(flatten)]` wherever needed — and [`create_server`], the
//! backend-selection factory that maps a configured [`DatabaseBackend`]
//! onto the matching concrete adapter, tunnelling through SSH when
//! configured.

use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::Args;
use dbmcp_config::{
    Config, ConfigError, ConfigErrors, DatabaseBackend, DatabaseConfig, PiiCategory, PiiConfig, PiiOperator, SshConfig,
    StrictHostKeyChecking, ssh,
};
use dbmcp_mysql::MysqlHandler;
use dbmcp_postgres::PostgresHandler;
use dbmcp_sql::is_connection_error;
use dbmcp_sqlite::SqliteHandler;
use dbmcp_ssh::{Tunnel, TunnelHandle};
use tracing::info;

use dbmcp_server::ErrorContext;
pub(crate) use dbmcp_server::Server;

/// Shared database connection flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "Database")]
pub(crate) struct DatabaseArguments {
    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", default_value_t = DatabaseConfig::DEFAULT_BACKEND)]
    pub(crate) backend: DatabaseBackend,

    /// Database host (as seen from the SSH server when tunnelling)
    #[arg(long = "db-host", env = "DB_HOST", default_value = DatabaseConfig::DEFAULT_HOST)]
    pub(crate) host: String,

    /// Database port (default: backend-dependent)
    #[arg(long = "db-port", env = "DB_PORT")]
    pub(crate) port: Option<u16>,

    /// Database user (default: backend-dependent)
    #[arg(long = "db-user", env = "DB_USER")]
    pub(crate) user: Option<String>,

    /// Database password
    #[arg(long = "db-password", env = "DB_PASSWORD")]
    pub(crate) password: Option<String>,

    /// Database name or `SQLite` file path
    #[arg(long = "db-name", env = "DB_NAME")]
    pub(crate) name: Option<String>,

    /// Character set (MySQL/MariaDB only)
    #[arg(long = "db-charset", env = "DB_CHARSET")]
    pub(crate) charset: Option<String>,

    /// Enable SSL for database connection
    #[arg(
        long = "db-ssl",
        env = "DB_SSL",
        default_value_t = DatabaseConfig::DEFAULT_SSL,
    )]
    pub(crate) ssl: bool,

    /// Path to CA certificate
    #[arg(long = "db-ssl-ca", env = "DB_SSL_CA")]
    pub(crate) ssl_ca: Option<String>,

    /// Path to client certificate
    #[arg(long = "db-ssl-cert", env = "DB_SSL_CERT")]
    pub(crate) ssl_cert: Option<String>,

    /// Path to a client key
    #[arg(long = "db-ssl-key", env = "DB_SSL_KEY")]
    pub(crate) ssl_key: Option<String>,

    /// Verify server certificate
    #[arg(
        long = "db-ssl-verify-cert",
        env = "DB_SSL_VERIFY_CERT",
        default_value_t = DatabaseConfig::DEFAULT_SSL_VERIFY_CERT,
    )]
    pub(crate) ssl_verify_cert: bool,

    /// Enable read-only mode
    #[arg(
        long = "db-read-only",
        env = "DB_READ_ONLY",
        default_value_t = DatabaseConfig::DEFAULT_READ_ONLY,
    )]
    pub(crate) read_only: bool,

    /// Maximum connection pool size
    #[arg(
        long = "db-max-pool-size",
        env = "DB_MAX_POOL_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE,
        value_parser = clap::value_parser!(u32).range(1..)
    )]
    pub(crate) max_pool_size: u32,

    /// Connection timeout in seconds
    #[arg(
        long = "db-connection-timeout",
        env = "DB_CONNECTION_TIMEOUT",
        value_parser = clap::value_parser!(u64).range(1..)
    )]
    pub(crate) connection_timeout: Option<u64>,

    /// Query execution timeout in seconds
    #[arg(
        long = "db-query-timeout",
        env = "DB_QUERY_TIMEOUT",
        default_value_t = DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS,
        value_parser = clap::value_parser!(u64)
    )]
    pub(crate) query_timeout: u64,

    /// Maximum items returned in a single paginated tool response
    #[arg(
        long = "db-page-size",
        env = "DB_PAGE_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_PAGE_SIZE,
        value_parser = clap::value_parser!(u16).range(1..=i64::from(DatabaseConfig::MAX_PAGE_SIZE)),
    )]
    pub(crate) page_size: u16,
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
            query_timeout: Some(db.query_timeout),
            page_size: db.page_size,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Shared PII flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "PII")]
pub(crate) struct PiiArguments {
    /// Enable PII redaction of query tool output
    #[arg(
        long = "pii",
        env = "PII_ENABLE",
        default_value_t = PiiConfig::DEFAULT_ENABLED,
        action = clap::ArgAction::Set,
        value_parser = clap::value_parser!(bool),
    )]
    pub(crate) enabled: bool,

    /// Operator applied to detected PII spans
    #[arg(
        long = "pii-operator",
        env = "PII_OPERATOR",
        default_value_t = PiiConfig::DEFAULT_OPERATOR,
    )]
    pub(crate) operator: PiiOperator,

    /// Comma-separated PII categories the analyzer should cover
    /// (personal, financial, government, contact, network, digital-identity, crypto)
    #[arg(
        long = "pii-categories",
        env = "PII_CATEGORIES",
        value_delimiter = ',',
        num_args = 1..,
    )]
    pub(crate) categories: Option<Vec<PiiCategory>>,

    /// Enable the optional ML/NER pass (person & location detection)
    #[arg(
        long = "pii-ner",
        env = "PII_NER_ENABLE",
        default_value_t = PiiConfig::DEFAULT_NER_ENABLED,
        action = clap::ArgAction::Set,
        value_parser = clap::value_parser!(bool),
    )]
    pub(crate) ner_enabled: bool,

    /// Path to the NER model directory (required with --pii-ner)
    #[arg(long = "pii-ner-model", env = "PII_NER_MODEL")]
    pub(crate) ner_model: Option<PathBuf>,
}

impl TryFrom<&PiiArguments> for PiiConfig {
    type Error = ConfigErrors;

    fn try_from(args: &PiiArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            enabled: args.enabled,
            operator: args.operator,
            categories: args.categories.clone(),
            ner_enabled: args.ner_enabled,
            ner_model: args.ner_model.clone(),
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Shared SSH tunnel flags embedded in transport subcommands.
///
/// Every argument uses an explicit `id = "ssh-*"` so its clap id doesn't
/// collide with the same-named fields in [`DatabaseArguments`]. `port`, `strict_host_key_checking`, and `keepalive_interval` stay
/// `Option` so [`SshArguments::to_config`] can tell whether they were set;
/// their defaults are applied there.
#[derive(Args)]
#[command(next_help_heading = "SSH")]
pub(crate) struct SshArguments {
    /// SSH server host; enables the tunnel when set
    #[arg(id = "ssh-host", long = "ssh-host", value_name = "HOST", env = "SSH_HOST")]
    pub(crate) host: Option<String>,

    /// SSH server port
    #[arg(
        id = "ssh-port",
        long = "ssh-port",
        value_name = "PORT",
        env = "SSH_PORT",
        value_parser = clap::value_parser!(u16).range(1..),
    )]
    pub(crate) port: Option<u16>,

    /// SSH user
    #[arg(id = "ssh-user", long = "ssh-user", value_name = "USER", env = "SSH_USER")]
    pub(crate) user: Option<String>,

    /// SSH password
    #[arg(
        id = "ssh-password",
        long = "ssh-password",
        value_name = "PASSWORD",
        env = "SSH_PASSWORD",
        hide_env_values = true
    )]
    pub(crate) password: Option<String>,

    /// Path to SSH private key (OpenSSH, PKCS#1, PKCS#8, SEC1)
    #[arg(id = "ssh-key", long = "ssh-key", value_name = "PATH", env = "SSH_KEY")]
    pub(crate) key: Option<PathBuf>,

    /// Passphrase for the SSH private key
    #[arg(
        id = "ssh-key-passphrase",
        long = "ssh-key-passphrase",
        value_name = "PASSPHRASE",
        env = "SSH_KEY_PASSPHRASE",
        hide_env_values = true
    )]
    pub(crate) key_passphrase: Option<String>,

    /// Path to known_hosts file (default: ~/.ssh/known_hosts)
    #[allow(clippy::doc_markdown, reason = "doc comment is the verbatim --help text")]
    #[arg(
        id = "ssh-known-hosts",
        long = "ssh-known-hosts",
        value_name = "PATH",
        env = "SSH_KNOWN_HOSTS"
    )]
    pub(crate) known_hosts: Option<PathBuf>,

    /// Host key policy: yes rejects unknown hosts, accept-new trusts on first use, no disables verification (insecure)
    #[arg(
        id = "ssh-strict-host-key-checking",
        long = "ssh-strict-host-key-checking",
        value_name = "MODE",
        env = "SSH_STRICT_HOST_KEY_CHECKING"
    )]
    pub(crate) strict_host_key_checking: Option<StrictHostKeyChecking>,

    /// SSH keepalive interval in seconds (0 disables)
    #[arg(
        id = "ssh-keepalive-interval",
        long = "ssh-keepalive-interval",
        value_name = "SECONDS",
        env = "SSH_KEEPALIVE_INTERVAL"
    )]
    pub(crate) keepalive_interval: Option<u64>,

    /// Local tunnel port (default: ephemeral)
    #[arg(
        id = "ssh-local-port",
        long = "ssh-local-port",
        value_name = "PORT",
        env = "SSH_LOCAL_PORT",
        value_parser = clap::value_parser!(u16).range(1..),
    )]
    pub(crate) local_port: Option<u16>,
}

impl std::fmt::Debug for SshArguments {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SshArguments")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("user", &self.user)
            .field("password", &self.password.as_ref().map(|_| "[REDACTED]"))
            .field("key", &self.key)
            .field("key_passphrase", &self.key_passphrase.as_ref().map(|_| "[REDACTED]"))
            .field("known_hosts", &self.known_hosts)
            .field("strict_host_key_checking", &self.strict_host_key_checking)
            .field("keepalive_interval", &self.keepalive_interval)
            .field("local_port", &self.local_port)
            .finish()
    }
}

impl SshArguments {
    /// Resolves the tunnel settings; `Ok(None)` when `SSH_HOST` is unset.
    ///
    /// Uses the process home directory for `~` expansion and the default
    /// `known_hosts` path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] when other `SSH_*` options are set
    /// without a host, or when [`SshConfig::validate`] fails.
    pub(crate) fn to_config(&self, backend: DatabaseBackend) -> Result<Option<SshConfig>, ConfigErrors> {
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

/// Logs the read-only banner and builds a [`Server`] for `config`.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable. The caller is expected to pass a fully
/// validated [`Config`].
///
/// When `config.ssh` is set, starts the SSH tunnel first and points the
/// adapter at its loopback address. The returned [`TunnelHandle`] must
/// outlive the [`Server`] so pool shutdown traffic reaches the database;
/// binding the tuple in order (`let (tunnel, server)`) drops the server
/// first on every return path. While the tunnel reports a failure, tool
/// errors caused by an unreachable database gain a `(SSH tunnel: <cause>)`
/// suffix before PII redaction; other errors are left alone.
///
/// # Errors
///
/// Returns [`crate::error::Error::Init`] when the SSH tunnel cannot start
/// or a handler fails to initialise (e.g. a fail-closed NER model load).
#[allow(clippy::result_large_err)]
pub(crate) async fn create_server(config: &Config) -> Result<(Option<TunnelHandle>, Server), crate::error::Error> {
    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    let Some(ssh) = &config.ssh else {
        return Ok((None, build_server(config)?));
    };

    let tunnel = Tunnel::start(ssh, &config.database.host, config.database.port)
        .await
        .map_err(|e| crate::error::Error::Init(e.to_string()))?;
    let local_addr = tunnel.local_addr();
    let mut tunnelled = config.clone();
    tunnelled.database.host = local_addr.ip().to_string();
    tunnelled.database.port = local_addr.port();
    let last_error = tunnel.last_error().clone();
    let context = ErrorContext::new(move |error| {
        is_connection_error(error)
            .then(|| last_error.get())
            .flatten()
            .map(|cause| format!("SSH tunnel: {cause}"))
    });
    let server = build_server(&tunnelled)?.with_error_context(context);
    Ok((Some(tunnel), server))
}

/// Builds the backend adapter selected by `config.database.backend`.
#[allow(clippy::result_large_err)]
pub(crate) fn build_server(config: &Config) -> Result<Server, crate::error::Error> {
    match config.database.backend {
        DatabaseBackend::Sqlite => init(SqliteHandler::new(config)),
        DatabaseBackend::Postgres => init(PostgresHandler::new(config)),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => init(MysqlHandler::new(config)),
    }
}

/// Maps a handler constructor result into a [`Server`], failing closed.
#[allow(clippy::result_large_err)]
fn init<H, E>(handler: Result<H, E>) -> Result<Server, crate::error::Error>
where
    H: Into<Server>,
    E: std::fmt::Display,
{
    handler
        .map(Into::into)
        .map_err(|e| crate::error::Error::Init(e.to_string()))
}

/// Future that resolves when the process should shut down.
///
/// Listens for Ctrl-C on all platforms and `SIGTERM` on Unix, which
/// is the signal `docker stop`, `systemctl stop`, and Kubernetes
/// send to request graceful termination. Whichever arrives first
/// wins. Shared by both transports.
pub(crate) async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("Ctrl-C received, shutting down..."),
        () = terminate => info!("SIGTERM received, shutting down..."),
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{DatabaseArguments, PiiArguments, SshArguments};
    use dbmcp_config::{
        ConfigError, DatabaseBackend, DatabaseConfig, PiiCategory, PiiConfig, PiiOperator, SshConfig,
        StrictHostKeyChecking,
    };

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct TestCli {
        #[command(flatten)]
        db: DatabaseArguments,
        #[command(flatten)]
        pii: PiiArguments,
        #[command(flatten)]
        ssh: SshArguments,
    }

    fn try_parse_with_page_size(value: &str) -> Result<u16, clap::Error> {
        // SAFETY: no other test in this file writes DB_PAGE_SIZE concurrently;
        // removing it here prevents a stale host value from leaking into clap.
        unsafe {
            std::env::remove_var("DB_PAGE_SIZE");
        }
        TestCli::try_parse_from(["--db-page-size", value]).map(|cli| cli.db.page_size)
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
        assert_eq!(try_parse_with_page_size("1").unwrap(), 1);
    }

    #[test]
    fn clap_accepts_page_size_at_max() {
        assert_eq!(try_parse_with_page_size("500").unwrap(), 500);
    }

    #[test]
    fn clap_default_page_size_is_100() {
        unsafe {
            std::env::remove_var("DB_PAGE_SIZE");
        }
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).unwrap();
        assert_eq!(cli.db.page_size, 100);
    }

    fn clear_pii_env() {
        // SAFETY: tests in this file do not write PII_* env vars concurrently.
        unsafe {
            std::env::remove_var("PII_ENABLE");
            std::env::remove_var("PII_OPERATOR");
            std::env::remove_var("PII_CATEGORIES");
            std::env::remove_var("PII_NER_ENABLE");
            std::env::remove_var("PII_NER_MODEL");
        }
    }

    #[test]
    fn clap_pii_operator_default_is_replace() {
        clear_pii_env();
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).unwrap();
        assert!(!cli.pii.enabled);
        assert_eq!(cli.pii.operator, PiiOperator::Replace);
    }

    #[test]
    fn clap_pii_operator_accepts_each_variant() {
        for (raw, expected) in [
            ("replace", PiiOperator::Replace),
            ("mask", PiiOperator::Mask),
            ("redact", PiiOperator::Redact),
            ("hash", PiiOperator::Hash),
        ] {
            clear_pii_env();
            let cli = TestCli::try_parse_from(["--pii-operator", raw]).expect("valid operator parses");
            assert_eq!(cli.pii.operator, expected, "operator {raw} must parse");
        }
    }

    #[test]
    fn clap_rejects_unknown_pii_operator() {
        clear_pii_env();
        let err = TestCli::try_parse_from(["--pii-operator", "xyz"]).expect_err("unknown operator must be rejected");
        let msg = err.to_string();
        assert!(msg.contains("xyz"), "error must name the offending value: {msg}");
        for accepted in ["replace", "mask", "redact", "hash"] {
            assert!(
                msg.contains(accepted),
                "error must list accepted value '{accepted}': {msg}"
            );
        }
    }

    fn clear_db_env() {
        // SAFETY: tests in this file do not write DB_* env vars concurrently.
        unsafe {
            std::env::remove_var("DB_BACKEND");
            std::env::remove_var("DB_NAME");
            std::env::remove_var("DB_SSL");
            std::env::remove_var("DB_SSL_CA");
            std::env::remove_var("DB_SSL_CERT");
            std::env::remove_var("DB_SSL_KEY");
        }
    }

    #[test]
    fn try_from_database_arguments_rejects_sqlite_without_db_name() {
        clear_db_env();
        let cli = TestCli::try_parse_from(["--db-backend", "sqlite"]).expect("clap parse");
        let errors = DatabaseConfig::try_from(&cli.db).expect_err("sqlite without name must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }

    #[test]
    fn try_from_database_arguments_rejects_missing_ssl_cert_files() {
        clear_db_env();
        let cli = TestCli::try_parse_from([
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
            "--db-ssl-cert",
            "/nonexistent/cert.pem",
            "--db-ssl-key",
            "/nonexistent/key.pem",
        ])
        .expect("clap parse");
        let errors = DatabaseConfig::try_from(&cli.db).expect_err("missing ssl cert files must fail");
        let cert_errors = errors
            .iter()
            .filter(|e| matches!(e, ConfigError::FileNotFound(_, _)))
            .count();
        assert_eq!(cert_errors, 3, "expected three FileNotFound errors, got {errors:?}");
    }

    #[test]
    fn try_from_database_arguments_accumulates_sqlite_name_and_ssl_errors() {
        clear_db_env();
        let cli = TestCli::try_parse_from([
            "--db-backend",
            "sqlite",
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
        ])
        .expect("clap parse");
        let errors = DatabaseConfig::try_from(&cli.db).expect_err("multiple errors must accumulate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::FileNotFound(name, path) if name == "DB_SSL_CA" && path == "/nonexistent/ca.pem"))
        );
    }

    #[test]
    fn pii_config_validate_is_called_from_try_from_path() {
        // Structural-presence guard: ensures PiiConfig::try_from invokes
        // PiiConfig::validate. When a future rule fires on default args,
        // this test deliberately fails — flagging the contributor.
        clear_pii_env();
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).expect("clap parse");
        PiiConfig::try_from(&cli.pii).expect("default pii args must validate");
    }

    #[test]
    fn clap_pii_flags_grouped_under_pii_heading() {
        let help = TestCli::command().render_help().to_string();
        let pii_pos = help.find("PII:").expect("PII heading must be present in --help");
        let flag_pos = help.find("--pii ").expect("--pii must be present in --help");
        assert!(
            pii_pos < flag_pos,
            "PII heading must precede --pii in help output:\n{help}"
        );
    }

    #[test]
    fn clap_pii_categories_default_unset() {
        clear_pii_env();
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).unwrap();
        assert!(cli.pii.categories.is_none(), "categories must default to None");
    }

    #[test]
    fn clap_pii_categories_comma_separated() {
        clear_pii_env();
        let cli = TestCli::try_parse_from(["--pii-categories", "financial,government"])
            .expect("comma-separated categories parse");
        let cats = cli.pii.categories.expect("categories set");
        assert_eq!(cats, vec![PiiCategory::Financial, PiiCategory::Government]);
    }

    #[test]
    fn clap_pii_categories_kebab_digital_identity() {
        clear_pii_env();
        let cli =
            TestCli::try_parse_from(["--pii-categories", "digital-identity"]).expect("digital-identity kebab parses");
        let cats = cli.pii.categories.expect("categories set");
        assert_eq!(cats, vec![PiiCategory::DigitalIdentity]);
    }

    #[test]
    fn clap_rejects_unknown_pii_category() {
        clear_pii_env();
        let err = TestCli::try_parse_from(["--pii-categories", "healthcare"]).expect_err("healthcare is not in v1");
        let msg = err.to_string();
        assert!(msg.contains("healthcare"), "error must name the offending value: {msg}");
    }

    #[test]
    fn pii_config_categories_empty_vec_errors_via_try_from() {
        // clap normally never produces an empty Vec (num_args=1..), but the validator
        // still defends against direct PiiConfig construction with Some(empty).
        let cfg = PiiConfig {
            enabled: true,
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
        clear_pii_env();
        let cli = TestCli::try_parse_from(Vec::<&str>::new()).unwrap();
        assert!(!cli.pii.ner_enabled, "ner must default off");
        assert!(cli.pii.ner_model.is_none(), "ner model must default unset");
    }

    #[test]
    fn clap_pii_ner_flags_parse_and_convert() {
        clear_pii_env();
        let cli =
            TestCli::try_parse_from(["--pii-ner", "true", "--pii-ner-model", "/models/ner"]).expect("ner flags parse");
        let cfg = PiiConfig::try_from(&cli.pii).expect("ner config validates");
        assert!(cfg.ner_enabled);
        assert_eq!(cfg.ner_model, Some(std::path::PathBuf::from("/models/ner")));
    }

    #[test]
    fn pii_ner_enabled_without_model_errors_via_try_from() {
        clear_pii_env();
        let cli = TestCli::try_parse_from(["--pii-ner", "true"]).expect("clap parse");
        let errors = PiiConfig::try_from(&cli.pii).expect_err("ner without model must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::PiiNerModelMissing)));
    }

    const SSH_ENV: [&str; 10] = [
        "SSH_HOST",
        "SSH_PORT",
        "SSH_USER",
        "SSH_PASSWORD",
        "SSH_KEY",
        "SSH_KEY_PASSPHRASE",
        "SSH_KNOWN_HOSTS",
        "SSH_STRICT_HOST_KEY_CHECKING",
        "SSH_KEEPALIVE_INTERVAL",
        "SSH_LOCAL_PORT",
    ];

    fn parse_ssh(args: &[&str]) -> SshArguments {
        // SAFETY: tests in this file do not write SSH_* env vars concurrently.
        unsafe {
            for name in SSH_ENV {
                std::env::remove_var(name);
            }
        }
        TestCli::try_parse_from(args).expect("clap parse").ssh
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    fn resolve(args: &[&str], home: Option<&Path>) -> Result<Option<SshConfig>, dbmcp_config::ConfigErrors> {
        parse_ssh(args).resolve(DatabaseBackend::Postgres, home)
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
                "SSH_HOST is required when other SSH_* options are set",
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
        assert_eq!(config.password.as_deref(), Some("pw"));
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
                ConfigError::FileNotFound(name, path) if name == "SSH_KEY" && *path == expected.display().to_string()
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
        let errors = parse_ssh(&["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"])
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
        parse_ssh(&[]);
        assert!(TestCli::try_parse_from(["--ssh-port", "0"]).is_err());
        assert!(TestCli::try_parse_from(["--ssh-local-port", "0"]).is_err());
    }

    #[test]
    fn clap_strict_host_key_checking_rejects_openssh_aliases() {
        parse_ssh(&[]);
        for value in ["ask", "off"] {
            assert!(
                TestCli::try_parse_from(["--ssh-strict-host-key-checking", value]).is_err(),
                "{value} must be rejected"
            );
        }
    }

    #[test]
    fn ssh_arguments_debug_redacts_secrets() {
        let args = parse_ssh(&["--ssh-password", "hunter2-pw", "--ssh-key-passphrase", "hunter2-pp"]);
        let debug = format!("{args:?}");
        assert!(!debug.contains("hunter2"), "{debug}");
        assert!(debug.contains("[REDACTED]"), "{debug}");
    }

    #[test]
    fn clap_ssh_flags_grouped_under_ssh_heading() {
        let help = TestCli::command().render_help().to_string();
        let ssh_pos = help.find("SSH:").expect("SSH heading must be present in --help");
        let flag_pos = help.find("--ssh-host").expect("--ssh-host must be present in --help");
        assert!(ssh_pos < flag_pos, "SSH heading must precede --ssh-host:\n{help}");
    }

    #[test]
    fn db_host_help_mentions_ssh_tunnel() {
        let help = TestCli::command().render_help().to_string();
        assert!(
            help.contains("Database host (as seen from the SSH server when tunnelling)"),
            "{help}"
        );
    }
}
