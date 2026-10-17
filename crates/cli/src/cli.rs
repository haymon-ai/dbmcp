//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Arguments`] struct (parsed by clap), the [`Command`]
//! subcommand enum, the [`LogLevel`] selector, and the [`run`] entry
//! point that dispatches to the active subcommand.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

use crate::commands::http::HttpCommand;
use crate::commands::stdio::StdioCommand;
use crate::error::{Error, ErrorKind};

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
#[command(about = "Database MCP Server")]
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

/// Builds the clap command for the binary named `bin` at `version`.
fn command(bin: &'static str, version: &'static str) -> clap::Command {
    Arguments::command().name(bin).version(version)
}

/// Parses CLI arguments, initializes tracing, and dispatches to the active subcommand.
///
/// # Errors
///
/// Returns an error if the command-line arguments are invalid,
/// configuration validation fails, or the selected subcommand fails
/// (transport initialization errors, TCP bind failures, fatal protocol
/// errors).
///
/// `bin` and `version` are the installed binary's name and version, used
/// in `--help`, `--version`, and the `version` subcommand. Must run
/// inside a Tokio runtime. A global tracing subscriber that is
/// already installed is kept.
#[allow(clippy::result_large_err)]
pub async fn run(bin: &'static str, version: &'static str) -> Result<(), Error> {
    let parsed = command(bin, version)
        .try_get_matches()
        .and_then(|mut matches| Arguments::from_arg_matches_mut(&mut matches));
    let arguments = match parsed {
        Ok(arguments) => arguments,
        // `--help` and `--version` arrive as clap errors that print to stdout.
        // Print failures (e.g. a closed pipe) are ignored, as `clap::Error::exit` does.
        Err(error) if !error.use_stderr() => {
            let _ = error.print();
            return Ok(());
        }
        Err(error) => return Err(ErrorKind::from(error).into()),
    };

    // Fails only if a subscriber is already installed; keep that one.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::from(arguments.log_level))
        .with_ansi(false)
        .try_init();

    match arguments.command {
        Command::Version => {
            println!("{bin} {version}");
            Ok(())
        }
        Command::Stdio(cmd) => Ok(cmd.execute().await?),
        Command::Http(cmd) => Ok(cmd.execute().await?),
    }
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::{Arguments, command};

    #[test]
    fn command_uses_given_name_and_version() {
        let cmd = command("dbmcp", "1.2.3");
        assert_eq!(cmd.get_name(), "dbmcp");
        assert_eq!(cmd.get_version(), Some("1.2.3"));
    }

    #[test]
    fn secret_env_values_are_hidden_from_help() {
        let cmd = Arguments::command();
        for subcommand in cmd.get_subcommands() {
            for arg in subcommand.get_arguments() {
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
    }

    fn subcommand_help(name: &str) -> String {
        let mut cmd = Arguments::command();
        cmd.find_subcommand_mut(name)
            .expect("subcommand")
            .render_long_help()
            .to_string()
    }

    fn assert_documents_page_size_flag(help: &str) {
        assert!(
            help.contains("--db-page-size"),
            "help missing --db-page-size flag:\n{help}"
        );
        assert!(
            help.contains("DB_PAGE_SIZE"),
            "help missing DB_PAGE_SIZE env binding:\n{help}"
        );
        assert!(help.contains("[default: 100]"), "help missing [default: 100]:\n{help}");
    }

    #[test]
    fn stdio_help_documents_db_page_size_flag() {
        assert_documents_page_size_flag(&subcommand_help("stdio"));
    }

    #[test]
    fn http_help_documents_db_page_size_flag() {
        assert_documents_page_size_flag(&subcommand_help("http"));
    }
}
