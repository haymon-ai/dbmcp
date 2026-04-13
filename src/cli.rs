//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Arguments`] struct (parsed by clap), the [`Command`]
//! subcommand enum, the [`LogLevel`] selector, and the [`run`] entry
//! point that dispatches to the active subcommand.

use clap::{Parser, Subcommand};
use std::process::ExitCode;

use crate::commands::http::HttpCommand;
use crate::commands::stdio::StdioCommand;
use crate::consts::{BIN, VERSION};
use crate::error::Error;

/// Log severity levels for the MCP server.
///
/// Maps directly to [`tracing::Level`] variants. Used as a
/// [`clap::ValueEnum`] for type-safe CLI argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum LogLevel {
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
#[derive(Debug, Parser)]
#[command(name = "database-mcp", about = "Database MCP Server", version)]
struct Arguments {
    /// Subcommand selector.
    #[command(subcommand)]
    command: Command,

    /// Log level
    #[arg(
        long = "log-level",
        env = "LOG_LEVEL",
        default_value_t = LogLevel::Info,
        ignore_case = true,
        global = true
    )]
    log_level: LogLevel,
}

/// Top-level subcommand selector.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print version information and exit.
    Version,
    /// Run in stdio mode (default).
    Stdio(StdioCommand),
    /// Run in HTTP/SSE mode.
    Http(HttpCommand),
}

/// Parses CLI arguments, initializes tracing, and dispatches to the active subcommand.
///
/// # Errors
///
/// Returns an error if the selected subcommand fails (e.g. transport
/// initialization errors, TCP bind failures, fatal protocol errors).
#[tokio::main]
#[allow(clippy::result_large_err)]
pub(crate) async fn run() -> Result<ExitCode, Error> {
    let arguments = Arguments::parse();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from(arguments.log_level))
        .with_ansi(false)
        .init();

    match arguments.command {
        Command::Version => {
            println!("{BIN} {VERSION}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Stdio(cmd) => cmd.execute().await,
        Command::Http(cmd) => cmd.execute().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::common::DatabaseArguments;
    use database_mcp_config::{DatabaseBackend, DatabaseConfig};

    fn parse(args: &[&str]) -> Arguments {
        Arguments::try_parse_from(args).unwrap()
    }

    fn stdio_db(args: &Arguments) -> &DatabaseArguments {
        match &args.command {
            Command::Stdio(cmd) => &cmd.db_arguments,
            _ => panic!("expected stdio subcommand"),
        }
    }

    fn http_db(args: &Arguments) -> &DatabaseArguments {
        match &args.command {
            Command::Http(cmd) => &cmd.db_arguments,
            _ => panic!("expected http subcommand"),
        }
    }

    #[test]
    fn no_subcommand_is_rejected() {
        let err = Arguments::try_parse_from([BIN]).expect_err("subcommand is required");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
        );
    }

    #[test]
    fn db_backend_before_subcommand_rejected() {
        assert!(Arguments::try_parse_from([BIN, "--db-backend", "mysql", "http"]).is_err());
        assert!(Arguments::try_parse_from([BIN, "--db-backend", "mysql", "stdio"]).is_err());
    }

    #[test]
    fn db_read_only_defaults_to_true() {
        let args = parse(&[BIN, "stdio"]);
        assert!(stdio_db(&args).read_only);
    }

    #[test]
    fn db_connection_timeout_zero_rejected() {
        assert!(Arguments::try_parse_from([BIN, "stdio", "--db-connection-timeout", "0"]).is_err());
    }

    #[test]
    fn db_query_timeout_zero_passes_through() {
        let args = parse(&[BIN, "stdio", "--db-query-timeout", "0"]);
        let config = DatabaseConfig::try_from(stdio_db(&args)).unwrap();
        assert_eq!(config.query_timeout, Some(0));
    }

    #[test]
    fn db_backend_after_http_subcommand() {
        let args = parse(&[BIN, "http", "--db-backend", "mysql"]);
        assert_eq!(http_db(&args).backend, DatabaseBackend::Mysql);
        assert!(matches!(args.command, Command::Http(_)));
    }

    #[test]
    fn stdio_subcommand_db_args() {
        let args = parse(&[
            BIN,
            "stdio",
            "--db-backend",
            "postgres",
            "--db-user",
            "pg",
            "--db-name",
            "app",
        ]);
        let db = stdio_db(&args);
        assert_eq!(db.backend, DatabaseBackend::Postgres);
        assert_eq!(db.user.as_deref(), Some("pg"));
        assert_eq!(db.name.as_deref(), Some("app"));

        let config = DatabaseConfig::try_from(db).unwrap();
        assert_eq!(config.backend, DatabaseBackend::Postgres);
        assert_eq!(config.user, "pg");
        assert_eq!(config.name.as_deref(), Some("app"));
    }

    #[test]
    fn version_subcommand() {
        let args = parse(&[BIN, "version"]);
        assert!(matches!(args.command, Command::Version));
    }
}
