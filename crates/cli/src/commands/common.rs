//! Common building blocks reused across transport subcommands.
//!
//! Hosts [`SharedArguments`], [`DatabaseArguments`], [`PiiArguments`], and
//! [`SshArguments`] — clap argument groups bundling `--config`,
//! `--log-level`, and every `--db-*`, `--pii-*`, and `--ssh-*` flag so each
//! is defined exactly once and embedded via `#[command(flatten)]` wherever
//! needed. Each group converts into its [`ConfigLayer`] section, keeping
//! only values set on the command line or through an env var, and [`load`]
//! fills the gaps from dotenv files and merges the result over the config
//! file. Also provides [`create_server`],
//! the backend-selection factory that maps a configured [`DatabaseBackend`]
//! onto the matching concrete adapter, tunnelling through SSH when
//! configured, and [`flag_conventions`], which shapes every flag by its
//! value type.

use std::any::TypeId;
use std::ffi::OsStr;
use std::path::PathBuf;

use clap::builder::BoolishValueParser;
use clap::parser::ValueSource;
use clap::{Arg, ArgAction, ArgMatches, Args, CommandFactory, FromArgMatches};
use dbmcp_config::{
    Config, ConfigLayer, ConfigWarning, DatabaseBackend, DatabaseConfig, DatabaseLayer, Dotenv, LoadError, LogLevel,
    PiiCategory, PiiConfig, PiiLayer, PiiOperator, Provenance, Resolved, Route, Secret, Source, Sources, SshLayer,
    StrictHostKeyChecking, Transport, ssh,
};
use dbmcp_mysql::MysqlHandler;
use dbmcp_postgres::PostgresHandler;
use dbmcp_sql::is_connection_error;
use dbmcp_sqlite::SqliteHandler;
use dbmcp_ssh::{Tunnel, TunnelHandle};
use tracing::info;

use dbmcp_server::ErrorContext;
pub(crate) use dbmcp_server::Server;

/// Applies the shared conventions for a flag's value type.
///
/// [`Secret`] flags hide their env value from `--help`. `bool` flags with
/// clap's default `SetTrue` action also accept an explicit value: bare
/// `--flag` means `true`; `--flag=<v>`, `--flag <v>`, and the env var accept
/// `true`/`false`, `yes`/`no`, `on`/`off`, and `1`/`0` in any case. Empty
/// and unknown values are rejected, so a blank or mistyped env var fails
/// startup instead of flipping a security setting. Each transport
/// subcommand applies it once, after
/// flattening its flag groups, via `#[command(mut_args(flag_conventions))]`,
/// so every flag it exposes inherits it.
pub(crate) fn flag_conventions(arg: Arg) -> Arg {
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

/// Returns `true` when argument `id` was set on the command line or via its env var.
fn is_explicit(matches: &ArgMatches, id: &str) -> bool {
    matches!(
        matches.value_source(id),
        Some(ValueSource::CommandLine | ValueSource::EnvVariable)
    )
}

/// Returns `value` only when argument `id` was set on the command line or via its env var.
///
/// Values that came from a clap default are dropped, so lower layers (the
/// dotenv files, the config file, then the built-in defaults) can fill them.
pub(crate) fn explicit<T: Clone>(matches: &ArgMatches, id: &str, value: Option<&T>) -> Option<T> {
    is_explicit(matches, id).then(|| value.cloned()).flatten()
}

/// Config-file and logging flags shared by every transport subcommand.
#[derive(Debug, Args)]
pub(crate) struct SharedArguments {
    /// Path to a YAML config file
    #[arg(long = "config", value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,

    /// Path to a dotenv file; repeatable, later wins. Replaces the .env beside --config
    #[arg(long = "env", value_name = "PATH")]
    pub(crate) env: Vec<PathBuf>,

    /// Log level
    #[arg(
        long = "log-level",
        env = "LOG_LEVEL",
        default_value_t = LogLevel::DEFAULT,
        ignore_case = true,
        help_heading = "Logging"
    )]
    pub(crate) log_level: LogLevel,
}

/// Converts a parsed subcommand and its matches into its explicit [`ConfigLayer`].
pub(crate) type ToLayer<C> = fn(&C, &ArgMatches) -> ConfigLayer;

/// Loads the configuration for the parsed subcommand `cmd`.
///
/// Reads the `--config` and dotenv files, fills every flag not set on the
/// command line or via its env var from a dotenv variable of the same name,
/// and merges the result over the config file. Returns the outcome together
/// with every warning collected, including on failure.
pub(crate) fn load<C: CommandFactory + FromArgMatches>(
    cmd: &C,
    matches: &ArgMatches,
    shared: &SharedArguments,
    transport: Transport,
    to_layer: ToLayer<C>,
) -> (Result<Resolved, LoadError>, Vec<ConfigWarning>) {
    let mut warnings = Vec::new();
    let result = try_load(cmd, matches, shared, transport, to_layer, &mut warnings);
    (result, warnings)
}

fn try_load<C: CommandFactory + FromArgMatches>(
    cmd: &C,
    matches: &ArgMatches,
    shared: &SharedArguments,
    transport: Transport,
    to_layer: ToLayer<C>,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<Resolved, LoadError> {
    let process_env = |name: &str| std::env::var(name).ok();
    let sources = Sources::read(shared.config.as_deref(), &shared.env, &process_env, warnings)?;

    let flags = flag_routes(to_layer);
    let mut provenance = Provenance::default();
    for flag in &flags {
        let source = match matches.value_source(&flag.id) {
            Some(ValueSource::CommandLine) => Some(Source::Flag(flag.flag())),
            Some(ValueSource::EnvVariable) => flag.env.clone().map(Source::Env),
            _ => None,
        };
        for path in &flag.paths {
            provenance.add_route(path, flag.route());
            if let Some(source) = &source {
                provenance.set_source(path, source.clone());
            }
        }
    }
    let dotenv = dotenv_layer(matches, sources.dotenv(), &flags, &mut provenance, to_layer)?;
    sources.resolve(
        to_layer(cmd, matches).or(dotenv),
        provenance,
        transport,
        ssh::home_dir().as_deref(),
        warnings,
    )
}

/// One flag of a subcommand and the config paths it sets.
struct FlagRoute {
    /// Clap argument id.
    id: String,
    /// Long name, without `--`.
    long: String,
    /// Bound env var, if any.
    env: Option<String>,
    /// Config paths the flag sets, e.g. `database.port`.
    paths: Vec<&'static str>,
}

impl FlagRoute {
    fn flag(&self) -> String {
        format!("--{}", self.long)
    }

    fn route(&self) -> Route {
        Route {
            flag: self.flag(),
            env: self.env.clone(),
        }
    }
}

/// Returns `C`'s command with every env var binding removed.
///
/// Parsing a single `--flag=value` with it yields a layer holding only
/// that flag, whatever the process environment contains.
fn bare_command<C: CommandFactory>() -> clap::Command {
    C::command()
        .no_binary_name(true)
        .mut_args(|arg| arg.env(None::<&'static str>))
}

/// Parses `--long=value` alone into its layer.
fn single_flag_layer<C: CommandFactory + FromArgMatches>(
    long: &str,
    value: &str,
    to_layer: ToLayer<C>,
) -> Result<ConfigLayer, clap::Error> {
    let matches = bare_command::<C>().try_get_matches_from([format!("--{long}={value}")])?;
    C::from_arg_matches(&matches).map(|cmd| to_layer(&cmd, &matches))
}

/// Discovers which config paths each flag of `C` sets.
///
/// Each flag is parsed alone with a sample value (its first possible
/// value, or `1`); the paths set in the resulting layer are its paths.
fn flag_routes<C: CommandFactory + FromArgMatches>(to_layer: ToLayer<C>) -> Vec<FlagRoute> {
    C::command()
        .get_arguments()
        .filter_map(|arg| {
            let long = arg.get_long()?;
            let sample = arg
                .get_possible_values()
                .first()
                .map_or_else(|| "1".to_owned(), |value| value.get_name().to_owned());
            let paths = single_flag_layer(long, &sample, to_layer).ok()?.set_paths();
            (!paths.is_empty()).then(|| FlagRoute {
                id: arg.get_id().as_str().to_owned(),
                long: long.to_owned(),
                env: arg.get_env().and_then(OsStr::to_str).map(str::to_owned),
                paths,
            })
        })
        .collect()
}

/// Test access to private routing helpers.
#[cfg(test)]
pub(crate) mod tests_support {
    use super::{CommandFactory, FromArgMatches, ToLayer};

    /// Returns each discovered flag's long name and config paths.
    pub(crate) fn flag_routes<C: CommandFactory + FromArgMatches>(
        to_layer: ToLayer<C>,
    ) -> Vec<(String, Vec<&'static str>)> {
        super::flag_routes(to_layer)
            .into_iter()
            .map(|flag| (flag.long, flag.paths))
            .collect()
    }
}

/// Returns the layer of flags whose env var is defined only in a dotenv file.
///
/// Each value is parsed by re-running clap on `--<flag>=<value>`, so it
/// goes through exactly the parser its env var would. Records a
/// [`Source::Dotenv`] for every path it fills.
///
/// # Errors
///
/// Returns [`LoadError::Env`] listing every invalid value from the first
/// env file that has one.
fn dotenv_layer<C: CommandFactory + FromArgMatches>(
    matches: &ArgMatches,
    dotenv: &Dotenv,
    flags: &[FlagRoute],
    provenance: &mut Provenance,
    to_layer: ToLayer<C>,
) -> Result<ConfigLayer, LoadError> {
    let mut layer = ConfigLayer::default();
    let mut errors: Vec<(PathBuf, String)> = Vec::new();
    for flag in flags {
        if is_explicit(matches, &flag.id) {
            continue;
        }
        let Some((env, var)) = flag
            .env
            .as_deref()
            .and_then(|env| dotenv.get(env).map(|var| (env, var)))
        else {
            continue;
        };
        match single_flag_layer(&flag.long, &var.value, to_layer) {
            Ok(flag_layer) => {
                for path in flag_layer.set_paths() {
                    provenance.set_source(
                        path,
                        Source::Dotenv {
                            name: env.to_owned(),
                            location: var.location(),
                        },
                    );
                }
                layer = layer.or(flag_layer);
            }
            Err(_) => errors.push((
                var.path.clone(),
                format!("{}: {env}: invalid value for {}", var.location(), flag.flag()),
            )),
        }
    }
    match errors.first() {
        None => Ok(layer),
        Some((path, _)) => Err(LoadError::Env {
            path: path.clone(),
            errors: errors
                .iter()
                .filter(|(p, _)| p == path)
                .map(|(_, error)| error.clone())
                .collect(),
        }),
    }
}

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
    #[arg(long = "db-password", env = "DB_PASSWORD", hide_env_values = true)]
    pub(crate) password: Option<Secret>,

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
        default_value_t = DatabaseConfig::DEFAULT_MAX_POOL_SIZE
    )]
    pub(crate) max_pool_size: u32,

    /// Connection timeout in seconds
    #[arg(long = "db-connection-timeout", env = "DB_CONNECTION_TIMEOUT")]
    pub(crate) connection_timeout: Option<u64>,

    /// Query execution timeout in seconds
    #[arg(
        long = "db-query-timeout",
        env = "DB_QUERY_TIMEOUT",
        default_value_t = DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS
    )]
    pub(crate) query_timeout: u64,

    /// Maximum items returned in a single paginated tool response
    #[arg(
        long = "db-page-size",
        env = "DB_PAGE_SIZE",
        default_value_t = DatabaseConfig::DEFAULT_PAGE_SIZE
    )]
    pub(crate) page_size: u16,
}

impl DatabaseArguments {
    /// Returns the explicitly set database flags as a layer.
    pub(crate) fn layer(&self, matches: &ArgMatches) -> DatabaseLayer {
        DatabaseLayer {
            backend: explicit(matches, "backend", Some(&self.backend)),
            host: explicit(matches, "host", Some(&self.host)),
            port: explicit(matches, "port", self.port.as_ref()),
            user: explicit(matches, "user", self.user.as_ref()),
            password: explicit(matches, "password", self.password.as_ref()),
            name: explicit(matches, "name", self.name.as_ref()),
            charset: explicit(matches, "charset", self.charset.as_ref()),
            ssl: explicit(matches, "ssl", Some(&self.ssl)),
            ssl_ca: explicit(matches, "ssl_ca", self.ssl_ca.as_ref()),
            ssl_cert: explicit(matches, "ssl_cert", self.ssl_cert.as_ref()),
            ssl_key: explicit(matches, "ssl_key", self.ssl_key.as_ref()),
            ssl_verify_cert: explicit(matches, "ssl_verify_cert", Some(&self.ssl_verify_cert)),
            read_only: explicit(matches, "read_only", Some(&self.read_only)),
            max_pool_size: explicit(matches, "max_pool_size", Some(&self.max_pool_size)),
            connection_timeout: explicit(matches, "connection_timeout", self.connection_timeout.as_ref()),
            query_timeout: explicit(matches, "query_timeout", Some(&self.query_timeout)),
            page_size: explicit(matches, "page_size", Some(&self.page_size)),
        }
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
    )]
    pub(crate) ner_enabled: bool,

    /// Path to the NER model directory (required with --pii-ner)
    #[arg(long = "pii-ner-model", env = "PII_NER_MODEL")]
    pub(crate) ner_model: Option<PathBuf>,
}

impl PiiArguments {
    /// Returns the explicitly set PII flags as a layer.
    pub(crate) fn layer(&self, matches: &ArgMatches) -> PiiLayer {
        PiiLayer {
            enabled: explicit(matches, "enabled", Some(&self.enabled)),
            operator: explicit(matches, "operator", Some(&self.operator)),
            categories: explicit(matches, "categories", self.categories.as_ref()),
            ner_enabled: explicit(matches, "ner_enabled", Some(&self.ner_enabled)),
            ner_model: explicit(matches, "ner_model", self.ner_model.as_ref()),
        }
    }
}

/// Shared SSH tunnel flags embedded in transport subcommands.
///
/// Every argument uses an explicit `id = "ssh-*"` so its clap id doesn't
/// collide with the same-named fields in [`DatabaseArguments`]. Every
/// option stays `Option`: setting the host enables the tunnel, and the
/// defaults are applied by [`ConfigLayer::resolve`].
#[derive(Debug, Args)]
#[command(next_help_heading = "SSH")]
pub(crate) struct SshArguments {
    /// SSH server host; enables the tunnel when set
    #[arg(id = "ssh-host", long = "ssh-host", value_name = "HOST", env = "SSH_HOST")]
    pub(crate) host: Option<String>,

    /// SSH server port
    #[arg(id = "ssh-port", long = "ssh-port", value_name = "PORT", env = "SSH_PORT")]
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
    pub(crate) password: Option<Secret>,

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
    pub(crate) key_passphrase: Option<Secret>,

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
        env = "SSH_LOCAL_PORT"
    )]
    pub(crate) local_port: Option<u16>,
}

impl SshArguments {
    /// Returns the explicitly set SSH flags as a layer.
    pub(crate) fn layer(&self, matches: &ArgMatches) -> SshLayer {
        SshLayer {
            host: explicit(matches, "ssh-host", self.host.as_ref()),
            port: explicit(matches, "ssh-port", self.port.as_ref()),
            user: explicit(matches, "ssh-user", self.user.as_ref()),
            password: explicit(matches, "ssh-password", self.password.as_ref()),
            key: explicit(matches, "ssh-key", self.key.as_ref()),
            key_passphrase: explicit(matches, "ssh-key-passphrase", self.key_passphrase.as_ref()),
            known_hosts: explicit(matches, "ssh-known-hosts", self.known_hosts.as_ref()),
            strict_host_key_checking: explicit(
                matches,
                "ssh-strict-host-key-checking",
                self.strict_host_key_checking.as_ref(),
            ),
            keepalive_interval: explicit(matches, "ssh-keepalive-interval", self.keepalive_interval.as_ref()),
            local_port: explicit(matches, "ssh-local-port", self.local_port.as_ref()),
        }
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
/// Returns [`crate::error::ErrorKind::Init`] when the SSH tunnel cannot start
/// or a handler fails to initialise (e.g. a fail-closed NER model load).
#[allow(clippy::result_large_err)]
pub(crate) async fn create_server(config: &Config) -> Result<(Option<TunnelHandle>, Server), crate::error::ErrorKind> {
    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    let Some(ssh) = &config.ssh else {
        return Ok((None, build_server(config)?));
    };

    let tunnel = Tunnel::start(ssh, &config.database.host, config.database.port)
        .await
        .map_err(|e| crate::error::ErrorKind::Init(e.to_string()))?;
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
pub(crate) fn build_server(config: &Config) -> Result<Server, crate::error::ErrorKind> {
    match config.database.backend {
        DatabaseBackend::Sqlite => init(SqliteHandler::new(config)),
        DatabaseBackend::Postgres => init(PostgresHandler::new(config)),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => init(MysqlHandler::new(config)),
    }
}

/// Maps a handler constructor result into a [`Server`], failing closed.
#[allow(clippy::result_large_err)]
fn init<H, E>(handler: Result<H, E>) -> Result<Server, crate::error::ErrorKind>
where
    H: Into<Server>,
    E: std::fmt::Display,
{
    handler
        .map(Into::into)
        .map_err(|e| crate::error::ErrorKind::Init(e.to_string()))
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
    use clap::{CommandFactory, FromArgMatches, Parser};

    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::{DatabaseArguments, PiiArguments, SshArguments, explicit};
    use dbmcp_config::{
        Config, ConfigError, ConfigErrors, ConfigLayer, DatabaseBackend, PiiCategory, PiiOperator,
        StrictHostKeyChecking, Transport,
    };

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true, mut_args(super::flag_conventions))]
    struct TestCli {
        #[command(flatten)]
        db: DatabaseArguments,
        #[command(flatten)]
        pii: PiiArguments,
        #[command(flatten)]
        ssh: SshArguments,
    }

    /// Removes every env var bound to a [`TestCli`] flag.
    fn clear_env() {
        for arg in TestCli::command().get_arguments() {
            if let Some(env) = arg.get_env() {
                // SAFETY: tests in this file only remove these env vars, never set them.
                unsafe {
                    std::env::remove_var(env);
                }
            }
        }
    }

    fn try_parse(args: &[&str]) -> Result<TestCli, clap::Error> {
        clear_env();
        TestCli::try_parse_from(args)
    }

    /// Parses `args` and converts them into the CLI layer.
    fn layer(args: &[&str]) -> ConfigLayer {
        clear_env();
        let matches = TestCli::command()
            .try_get_matches_from(args)
            .unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
        let cli = TestCli::from_arg_matches(&matches).expect("from matches");
        ConfigLayer {
            database: cli.db.layer(&matches),
            pii: cli.pii.layer(&matches),
            ssh: cli.ssh.layer(&matches),
            ..ConfigLayer::default()
        }
    }

    fn resolve_with_home(args: &[&str], home: Option<&Path>) -> Result<Config, ConfigErrors> {
        layer(args)
            .resolve(Transport::Stdio, home)
            .map(|resolved| resolved.config)
    }

    fn resolve(args: &[&str]) -> Result<Config, ConfigErrors> {
        resolve_with_home(args, Some(&home()))
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    #[test]
    fn defaults_are_blanked_from_layer() {
        let layer = layer(&[]);
        assert_eq!(
            format!("{layer:?}"),
            format!("{:?}", ConfigLayer::default()),
            "no flag set must yield an empty layer"
        );
    }

    #[test]
    fn every_flag_reaches_the_layer() {
        let empty = format!("{:?}", ConfigLayer::default());
        for arg in TestCli::command().get_arguments() {
            let Some(long) = arg.get_long().filter(|long| *long != "help") else {
                continue;
            };
            let value = arg
                .get_possible_values()
                .first()
                .map_or_else(|| "1".to_owned(), |value| value.get_name().to_owned());
            let flag = format!("--{long}={value}");
            assert_ne!(format!("{:?}", layer(&[&flag])), empty, "{flag} must reach the layer");
        }
    }

    #[test]
    fn explicit_keeps_command_line_and_env_values_only() {
        const ENV: &str = "DBMCP_TEST_EXPLICIT_ONLY";
        let cmd = || {
            clap::Command::new("t")
                .no_binary_name(true)
                .arg(clap::Arg::new("x").long("x").env(ENV).default_value("default"))
        };
        let value = |matches: &clap::ArgMatches| explicit(matches, "x", matches.get_one::<String>("x"));

        let matches = cmd().try_get_matches_from(Vec::<&str>::new()).expect("parse");
        assert_eq!(value(&matches), None, "default value must be blanked");

        let matches = cmd().try_get_matches_from(["--x", "flag"]).expect("parse");
        assert_eq!(value(&matches).as_deref(), Some("flag"));

        // SAFETY: ENV is unique to this test.
        unsafe {
            std::env::set_var(ENV, "env");
        }
        let matches = cmd().try_get_matches_from(Vec::<&str>::new()).expect("parse");
        // SAFETY: ENV is unique to this test.
        unsafe {
            std::env::remove_var(ENV);
        }
        assert_eq!(value(&matches).as_deref(), Some("env"));
    }

    #[test]
    fn explicit_values_reach_the_resolved_config() {
        let config = resolve(&[
            "--db-backend",
            "postgres",
            "--db-user",
            "pg",
            "--db-name",
            "app",
            "--db-read-only=false",
            "--db-query-timeout",
            "0",
        ])
        .expect("valid");
        assert_eq!(config.database.backend, DatabaseBackend::Postgres);
        assert_eq!(config.database.port, 5432);
        assert_eq!(config.database.user, "pg");
        assert_eq!(config.database.name.as_deref(), Some("app"));
        assert!(!config.database.read_only);
        assert_eq!(config.database.query_timeout, 0);
    }

    #[test]
    fn page_size_range_is_validated_after_merge() {
        for value in ["0", "501"] {
            let errors = resolve(&["--db-page-size", value]).expect_err("out of range");
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ConfigError::OutOfRange { field, .. } if *field == "database.page_size")),
                "{value}: {errors:?}"
            );
        }
        for value in ["1", "500"] {
            let config = resolve(&["--db-page-size", value]).expect("in range");
            assert_eq!(config.database.page_size.to_string(), value);
        }
        assert_eq!(resolve(&[]).expect("default").database.page_size, 100);
    }

    #[test]
    fn clap_rejects_non_integer_page_size() {
        for value in ["-1", "abc"] {
            assert!(try_parse(&["--db-page-size", value]).is_err(), "{value}");
        }
    }

    #[test]
    fn zero_pool_size_and_connection_timeout_are_rejected() {
        let errors = resolve(&["--db-max-pool-size", "0", "--db-connection-timeout", "0"]).expect_err("zero");
        let messages: Vec<String> = errors.iter().map(ToString::to_string).collect();
        assert_eq!(
            messages,
            [
                "database.max_pool_size: must be at least 1",
                "database.connection_timeout: must be at least 1"
            ]
        );
    }

    #[test]
    fn clap_pii_operator_default_is_replace() {
        let config = resolve(&[]).expect("valid");
        assert!(!config.pii.enabled);
        assert_eq!(config.pii.operator, PiiOperator::Replace);
    }

    #[test]
    fn clap_pii_operator_accepts_each_variant() {
        for (raw, expected) in [
            ("replace", PiiOperator::Replace),
            ("mask", PiiOperator::Mask),
            ("redact", PiiOperator::Redact),
            ("hash", PiiOperator::Hash),
        ] {
            let cli = try_parse(&["--pii-operator", raw]).expect("valid operator parses");
            assert_eq!(cli.pii.operator, expected, "operator {raw} must parse");
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
    fn sqlite_without_db_name_is_rejected() {
        let errors = resolve(&["--db-backend", "sqlite"]).expect_err("sqlite without name must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }

    #[test]
    fn missing_ssl_cert_files_are_rejected() {
        let errors = resolve(&[
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
            "--db-ssl-cert",
            "/nonexistent/cert.pem",
            "--db-ssl-key",
            "/nonexistent/key.pem",
        ])
        .expect_err("missing ssl cert files must fail");
        let cert_errors = errors
            .iter()
            .filter(|e| matches!(e, ConfigError::FileNotFound { .. }))
            .count();
        assert_eq!(cert_errors, 3, "expected three FileNotFound errors, got {errors:?}");
    }

    #[test]
    fn sqlite_name_and_ssl_errors_accumulate() {
        let errors = resolve(&[
            "--db-backend",
            "sqlite",
            "--db-ssl",
            "--db-ssl-ca",
            "/nonexistent/ca.pem",
        ])
        .expect_err("multiple errors must accumulate");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::FileNotFound { field: name, path } if *name == "database.ssl_ca" && path == "/nonexistent/ca.pem"))
        );
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
        assert!(resolve(&[]).expect("valid").pii.categories.is_none());
    }

    #[test]
    fn clap_pii_categories_comma_separated() {
        let config = resolve(&["--pii-categories", "financial,government"]).expect("valid");
        assert_eq!(
            config.pii.categories,
            Some(vec![PiiCategory::Financial, PiiCategory::Government])
        );
    }

    #[test]
    fn clap_pii_categories_kebab_digital_identity() {
        let config = resolve(&["--pii-categories", "digital-identity"]).expect("valid");
        assert_eq!(config.pii.categories, Some(vec![PiiCategory::DigitalIdentity]));
    }

    #[test]
    fn clap_rejects_unknown_pii_category() {
        let err = try_parse(&["--pii-categories", "healthcare"]).expect_err("healthcare is not in v1");
        let msg = err.to_string();
        assert!(msg.contains("healthcare"), "error must name the offending value: {msg}");
    }

    #[test]
    fn duplicate_pii_categories_are_rejected() {
        let errors = resolve(&["--pii-categories", "financial,financial"]).expect_err("duplicate");
        assert!(
            errors
                .iter()
                .any(|e| matches!(e, ConfigError::DuplicateListEntry { .. }))
        );
    }

    #[test]
    fn clap_pii_ner_default_off() {
        let config = resolve(&[]).expect("valid");
        assert!(!config.pii.ner_enabled, "ner must default off");
        assert!(config.pii.ner_model.is_none(), "ner model must default unset");
    }

    #[test]
    fn clap_pii_ner_flags_parse_and_convert() {
        let config = resolve(&["--pii-ner", "true", "--pii-ner-model", "/models/ner"]).expect("ner config validates");
        assert!(config.pii.ner_enabled);
        assert_eq!(config.pii.ner_model, Some(PathBuf::from("/models/ner")));
    }

    #[test]
    fn pii_ner_enabled_without_model_is_rejected() {
        let errors = resolve(&["--pii-ner", "true"]).expect_err("ner without model must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::PiiNerModelMissing)));
    }

    #[test]
    fn ssh_disabled_without_any_option() {
        assert!(resolve(&[]).expect("valid").ssh.is_none());
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
            let errors = resolve(args).expect_err("orphan option must fail");
            assert_eq!(errors.len(), 1, "{args:?}");
            assert_eq!(
                errors[0].to_string(),
                "ssh.host: required when other ssh options are set",
                "{args:?}"
            );
        }
    }

    #[test]
    fn ssh_defaults_are_resolved() {
        let config = resolve(&["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"])
            .expect("valid")
            .ssh
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
        let config = resolve(&[
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
        ])
        .expect("valid")
        .ssh
        .expect("tunnel enabled");
        assert_eq!(config.port, 2222);
        assert_eq!(config.known_hosts, Some(home().join(".ssh").join("custom_hosts")));
        assert_eq!(config.strict_host_key_checking, StrictHostKeyChecking::AcceptNew);
        assert_eq!(config.keepalive_interval, Duration::ZERO);
        assert_eq!(config.local_port, Some(4000));
    }

    #[test]
    fn ssh_key_path_is_tilde_expanded() {
        let errors = resolve(&[
            "--ssh-host",
            "bastion",
            "--ssh-user",
            "deploy",
            "--ssh-key",
            "~/.ssh/missing_key",
        ])
        .expect_err("missing key file must fail");
        let expected = home().join(".ssh").join("missing_key");
        assert!(
            errors.iter().any(|e| matches!(
                e,
                ConfigError::FileNotFound { field: name, path } if *name == "ssh.key" && *path == expected.display().to_string()
            )),
            "{errors:?}"
        );
    }

    #[test]
    fn ssh_known_hosts_unresolved_without_home() {
        let errors = resolve_with_home(
            &["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"],
            None,
        )
        .expect_err("no home must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshKnownHosts)));

        let config = resolve_with_home(
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
        .ssh
        .expect("tunnel enabled");
        assert_eq!(config.known_hosts, None);
    }

    #[test]
    fn ssh_missing_user_and_auth_accumulate() {
        let errors = resolve(&["--ssh-host", "bastion"]).expect_err("must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshUser)));
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSshAuth)));
    }

    #[test]
    fn ssh_rejected_for_sqlite_backend() {
        let errors = resolve(&[
            "--db-backend",
            "sqlite",
            "--db-name",
            ":memory:",
            "--ssh-host",
            "bastion",
            "--ssh-user",
            "deploy",
            "--ssh-password",
            "pw",
        ])
        .expect_err("sqlite must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::SshSqliteUnsupported)));
    }

    #[test]
    fn ssh_empty_host_is_rejected() {
        let errors = resolve(&["--ssh-host", "", "--ssh-user", "deploy", "--ssh-password", "pw"])
            .expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptySshHost)));
    }

    #[test]
    fn ssh_port_zero_and_local_port_zero_are_rejected() {
        let base = ["--ssh-host", "bastion", "--ssh-user", "deploy", "--ssh-password", "pw"];
        for (flag, field) in [("--ssh-port", "ssh.port"), ("--ssh-local-port", "ssh.local_port")] {
            let mut args = base.to_vec();
            args.extend([flag, "0"]);
            let errors = resolve(&args).expect_err("zero port");
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(e, ConfigError::BelowMinimum { field: f, .. } if *f == field)),
                "{flag}: {errors:?}"
            );
        }
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
        let flags = [
            ("--db-ssl", "ssl"),
            ("--db-ssl-verify-cert", "ssl_verify_cert"),
            ("--db-read-only", "read_only"),
            ("--pii", "enabled"),
            ("--pii-ner", "ner_enabled"),
        ];
        for (flag, id) in flags {
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
                let matches = TestCli::command()
                    .try_get_matches_from(&args)
                    .unwrap_or_else(|e| panic!("{args:?} must parse: {e}"));
                assert_eq!(matches.get_one::<bool>(id), Some(&expected), "{args:?}");
            }
            for value in ["", "disabled", "nope", "2"] {
                let arg = format!("{flag}={value}");
                assert!(
                    TestCli::command().try_get_matches_from([&arg]).is_err(),
                    "{arg:?} must be rejected"
                );
            }
        }
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
