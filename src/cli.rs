//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Arguments`] struct (parsed by clap), the [`Command`]
//! subcommand enum, the [`LogLevel`] selector, and the [`run`] entry
//! point that dispatches to the active subcommand.

use clap::{Parser, Subcommand};

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
        global = true,
        help_heading = "Logging"
    )]
    log_level: LogLevel,
}

/// Top-level subcommand selector.
#[derive(Debug, Subcommand)]
enum Command {
    /// Print version information and exit.
    Version,
    /// Run in stdio mode.
    Stdio(StdioCommand),
    /// Run in HTTP/SSE mode.
    Http(HttpCommand),
}

/// Parses CLI arguments, initializes tracing, and dispatches to the active subcommand.
///
/// # Errors
///
/// Returns an error if configuration validation fails or the selected
/// subcommand fails (transport initialization errors, TCP bind
/// failures, fatal protocol errors).
#[tokio::main]
#[allow(clippy::result_large_err)]
pub(crate) async fn run() -> Result<(), Error> {
    let arguments = Arguments::parse();

    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from(arguments.log_level))
        .with_ansi(false)
        .init();

    match arguments.command {
        Command::Version => {
            println!("{BIN} {VERSION}");
            Ok(())
        }
        Command::Stdio(cmd) => cmd.execute().await,
        Command::Http(cmd) => cmd.execute().await,
    }
}
