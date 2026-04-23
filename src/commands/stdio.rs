//! Stdio transport command.
//!
//! Runs the MCP server over stdin/stdout for use with Claude Desktop,
//! Cursor, and other MCP clients that communicate via stdio.

use clap::Parser;
use dbmcp_config::DatabaseConfig;
use rmcp::ServiceExt;
use tracing::{error, info};

use crate::commands::common::{self, DatabaseArguments};
use crate::error::Error;

/// Runs the MCP server in stdio mode.
#[derive(Debug, Parser)]
pub(crate) struct StdioCommand {
    /// Shared database connection flags.
    #[command(flatten)]
    db_arguments: DatabaseArguments,
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
        let db_config = DatabaseConfig::try_from(&self.db_arguments)?;
        let server = common::create_server(&db_config);

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
    use dbmcp_config::{ConfigError, DatabaseBackend};

    #[track_caller]
    fn parse(args: &[&str]) -> StdioCommand {
        StdioCommand::try_parse_from(args).expect("valid stdio command")
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        let cmd = parse(&["_"]);
        assert!(cmd.db_arguments.read_only);
    }

    #[test]
    fn db_query_timeout_zero_passes_through() {
        let cmd = parse(&["_", "--db-query-timeout", "0"]);
        let config = DatabaseConfig::try_from(&cmd.db_arguments).expect("valid db args");
        assert_eq!(config.query_timeout, Some(0));
    }

    #[test]
    fn db_args_populate_database_config() {
        let cmd = parse(&["_", "--db-backend", "postgres", "--db-user", "pg", "--db-name", "app"]);
        assert_eq!(cmd.db_arguments.backend, DatabaseBackend::Postgres);
        assert_eq!(cmd.db_arguments.user.as_deref(), Some("pg"));
        assert_eq!(cmd.db_arguments.name.as_deref(), Some("app"));

        let config = DatabaseConfig::try_from(&cmd.db_arguments).expect("valid postgres args");
        assert_eq!(config.backend, DatabaseBackend::Postgres);
        assert_eq!(config.user, "pg");
        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn try_from_database_arguments_propagates_validation_errors() {
        // SQLite without --db-name must fail validation inside the TryFrom impl,
        // surfacing `ConfigError::MissingSqliteDbName` to the caller.
        let cmd = parse(&["_", "--db-backend", "sqlite"]);
        let errors =
            DatabaseConfig::try_from(&cmd.db_arguments).expect_err("sqlite without --db-name must be rejected");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::MissingSqliteDbName)));
    }
}
