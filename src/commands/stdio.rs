//! Stdio transport command.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use clap::Parser;
use dbmcp_config::{Config, ConfigError, ConfigErrors, DatabaseConfig, PiiConfig};
use rmcp::ServiceExt;
use tracing::{error, info};

use crate::commands::common::{self, DatabaseArguments, PiiArguments};
use crate::error::Error;

/// Runs the MCP server in stdio mode.
#[derive(Debug, Parser)]
pub(crate) struct StdioCommand {
    /// Shared database connection flags.
    #[command(flatten)]
    database: DatabaseArguments,

    /// Shared PII flags.
    #[command(flatten)]
    pii: PiiArguments,
}

impl TryFrom<&StdioCommand> for Config {
    type Error = ConfigErrors;

    fn try_from(cmd: &StdioCommand) -> Result<Self, Self::Error> {
        match (DatabaseConfig::try_from(&cmd.database), PiiConfig::try_from(&cmd.pii)) {
            (Ok(database), Ok(pii)) => Ok(Self {
                database,
                http: None,
                pii,
            }),
            (database, pii) => {
                let mut errors: Vec<ConfigError> = Vec::new();
                if let Err(e) = database {
                    errors.extend(e);
                }
                if let Err(e) = pii {
                    errors.extend(e);
                }
                Err(ConfigErrors::from_vec(errors).expect("non-Ok branch implies at least one Err"))
            }
        }
    }
}

impl StdioCommand {
    /// Builds the database configuration, server, and runs the stdio transport.
    ///
    /// Serves JSON-RPC over stdin/stdout.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation fails, the stdio
    /// transport fails to initialize, or the server encounters a fatal
    /// protocol error.
    pub(crate) async fn execute(&self) -> Result<(), Error> {
        let config = Config::try_from(self)?;
        let server = common::create_server(&config);

        info!("Starting MCP server via stdio transport...");
        let transport = rmcp::transport::io::stdio();
        let running = server.serve(transport).await?;
        if let Err(join_error) = running.waiting().await {
            error!("stdio server task terminated abnormally: {join_error}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbmcp_config::DatabaseBackend;

    #[track_caller]
    fn parse(args: &[&str]) -> StdioCommand {
        StdioCommand::try_parse_from(args).expect("valid stdio command")
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        let cmd = parse(&["_"]);
        assert!(cmd.database.read_only);
    }

    #[test]
    fn db_query_timeout_zero_passes_through() {
        let cmd = parse(&["_", "--db-query-timeout", "0"]);
        let config = DatabaseConfig::try_from(&cmd.database).expect("valid db args");
        assert_eq!(config.query_timeout, Some(0));
    }

    #[test]
    fn db_args_populate_database_config() {
        let cmd = parse(&["_", "--db-backend", "postgres", "--db-user", "pg", "--db-name", "app"]);
        assert_eq!(cmd.database.backend, DatabaseBackend::Postgres);
        assert_eq!(cmd.database.user.as_deref(), Some("pg"));
        assert_eq!(cmd.database.name.as_deref(), Some("app"));

        let config = DatabaseConfig::try_from(&cmd.database).expect("valid postgres args");
        assert_eq!(config.backend, DatabaseBackend::Postgres);
        assert_eq!(config.user, "pg");
        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn try_from_database_arguments_propagates_validation_errors() {
        // SQLite without --db-name must fail validation inside the TryFrom impl,
        // surfacing `ConfigError::MissingSqliteDbName` to the caller.
        let cmd = parse(&["_", "--db-backend", "sqlite"]);
        let errors = DatabaseConfig::try_from(&cmd.database).expect_err("sqlite without --db-name must be rejected");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }

    #[test]
    fn top_level_try_from_accumulates_across_sections_in_db_pii_order() {
        // PiiConfig has no rules today, so this exercises the database-section
        // accumulation reaching the top-level join. When pii gains a rule that
        // fires on default args, widen the assertion to verify db→pii ordering.
        let cmd = parse(&[
            "_",
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
        let errors = Config::try_from(&cmd).expect_err("multi-section misconfig must fail");
        assert_eq!(errors.len(), 4);
        assert!(matches!(errors[0], ConfigError::MissingSqliteDbName));
        assert!(matches!(&errors[1], ConfigError::SslCertNotFound(name, _) if name == "DB_SSL_CA"));
        assert!(matches!(&errors[2], ConfigError::SslCertNotFound(name, _) if name == "DB_SSL_CERT"));
        assert!(matches!(&errors[3], ConfigError::SslCertNotFound(name, _) if name == "DB_SSL_KEY"));
    }
}
