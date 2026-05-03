//! Common building blocks reused across transport subcommands.
//!
//! Hosts [`DatabaseArguments`] and [`PiiArguments`] — clap argument groups
//! bundling every `--db-*` and `--pii-*` flag so each is defined exactly
//! once and embedded via `#[command(flatten)]` wherever needed — and
//! [`create_server`], the backend-selection factory that maps a
//! configured [`DatabaseBackend`] onto the matching concrete adapter.

use clap::Args;
use dbmcp_config::{Config, ConfigErrors, DatabaseBackend, DatabaseConfig, PiiConfig, PiiOperator};
use dbmcp_mysql::MysqlHandler;
use dbmcp_postgres::PostgresHandler;
use dbmcp_sqlite::SqliteHandler;
use tracing::info;

pub(crate) use dbmcp_server::Server;

/// Shared database connection flags embedded in transport subcommands.
#[derive(Debug, Args)]
#[command(next_help_heading = "Database")]
pub(crate) struct DatabaseArguments {
    /// Database backend
    #[arg(long = "db-backend", env = "DB_BACKEND", default_value_t = DatabaseConfig::DEFAULT_BACKEND)]
    pub(crate) backend: DatabaseBackend,

    /// Database host
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
}

impl TryFrom<&PiiArguments> for PiiConfig {
    type Error = ConfigErrors;

    fn try_from(args: &PiiArguments) -> Result<Self, Self::Error> {
        let candidate = Self {
            enabled: args.enabled,
            operator: args.operator,
        };
        candidate.validate()?;
        Ok(candidate)
    }
}

/// Logs the read-only banner and builds a [`Server`] for `config`.
///
/// Does **not** establish a database connection. Each adapter defers
/// pool creation until the first tool invocation, allowing the MCP
/// server to start and respond to protocol messages even when the
/// database is unreachable. The caller is expected to pass a fully
/// validated [`Config`].
#[must_use]
pub(crate) fn create_server(config: &Config) -> Server {
    if config.database.read_only {
        info!("Server running in READ-ONLY mode. Write operations are disabled.");
    }

    match config.database.backend {
        DatabaseBackend::Sqlite => SqliteHandler::new(config).into(),
        DatabaseBackend::Postgres => PostgresHandler::new(config).into(),
        DatabaseBackend::Mysql | DatabaseBackend::Mariadb => MysqlHandler::new(config).into(),
    }
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{DatabaseArguments, PiiArguments};
    use dbmcp_config::{ConfigError, DatabaseConfig, PiiConfig, PiiOperator};

    #[derive(Debug, Parser)]
    #[command(no_binary_name = true)]
    struct TestCli {
        #[command(flatten)]
        db: DatabaseArguments,
        #[command(flatten)]
        pii: PiiArguments,
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
            .filter(|e| matches!(e, ConfigError::SslCertNotFound(_, _)))
            .count();
        assert_eq!(cert_errors, 3, "expected three SslCertNotFound errors, got {errors:?}");
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
                .any(|e| matches!(e, ConfigError::SslCertNotFound(name, path) if name == "DB_SSL_CA" && path == "/nonexistent/ca.pem"))
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
}
