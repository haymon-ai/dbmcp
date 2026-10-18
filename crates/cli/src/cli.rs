//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Arguments`] struct (parsed by clap), the [`Command`]
//! subcommand enum, and the [`run`] entry point that resolves the
//! configuration, initialises tracing, and dispatches to the active
//! subcommand.

use clap::{ArgMatches, CommandFactory, FromArgMatches, Parser, Subcommand};
use dbmcp_config::LogLevel;

use crate::commands::http::HttpCommand;
use crate::commands::stdio::StdioCommand;
use crate::error::{Error, ErrorKind};

/// Maps a [`LogLevel`] onto the matching [`tracing::Level`].
fn tracing_level(level: LogLevel) -> tracing::Level {
    match level {
        LogLevel::Error => tracing::Level::ERROR,
        LogLevel::Warn => tracing::Level::WARN,
        LogLevel::Info => tracing::Level::INFO,
        LogLevel::Debug => tracing::Level::DEBUG,
        LogLevel::Trace => tracing::Level::TRACE,
    }
}

/// Top-level CLI arguments parsed by clap.
#[derive(Debug, Parser)]
#[command(about = "Database MCP Server")]
struct Arguments {
    /// Subcommand selector.
    #[command(subcommand)]
    command: Command,
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

/// Parses CLI arguments, resolves the configuration, initializes tracing, and runs the subcommand.
///
/// # Errors
///
/// Returns an error if the command-line arguments are invalid, the config
/// file cannot be loaded, configuration validation fails, or the selected
/// subcommand fails (transport initialization errors, TCP bind failures,
/// fatal protocol errors).
///
/// `bin` and `version` are the installed binary's name and version, used
/// in `--help`, `--version`, and the `version` subcommand. Must run
/// inside a Tokio runtime. A global tracing subscriber that is
/// already installed is kept.
#[allow(clippy::result_large_err)]
pub async fn run(bin: &'static str, version: &'static str) -> Result<(), Error> {
    // `from_arg_matches` leaves `matches` intact: the layer conversions
    // need each argument's value source afterwards.
    let parsed = command(bin, version)
        .try_get_matches()
        .and_then(|matches| Arguments::from_arg_matches(&matches).map(|arguments| (matches, arguments)));
    let (matches, arguments) = match parsed {
        Ok(parsed) => parsed,
        // `--help` and `--version` arrive as clap errors that print to stdout.
        // Print failures (e.g. a closed pipe) are ignored, as `clap::Error::exit` does.
        Err(error) if !error.use_stderr() => {
            let _ = error.print();
            return Ok(());
        }
        Err(error) => return Err(ErrorKind::from(error).into()),
    };

    match arguments.command {
        Command::Version => {
            println!("{bin} {version}");
            Ok(())
        }
        Command::Stdio(cmd) => {
            let resolved = cmd.load(subcommand_matches(&matches)).map_err(ErrorKind::from)?;
            init_tracing(resolved.log_level);
            Ok(StdioCommand::execute(resolved.config).await?)
        }
        Command::Http(cmd) => {
            let resolved = cmd.load(subcommand_matches(&matches)).map_err(ErrorKind::from)?;
            init_tracing(resolved.log_level);
            Ok(HttpCommand::execute(resolved.config).await?)
        }
    }
}

/// Returns the matches of the subcommand selected in `matches`.
fn subcommand_matches(matches: &ArgMatches) -> &ArgMatches {
    matches
        .subcommand()
        .map(|(_, sub)| sub)
        .expect("subcommand is required")
}

/// Installs the stderr tracing subscriber at `level`.
fn init_tracing(level: LogLevel) {
    // Fails only if a subscriber is already installed; keep that one.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing_level(level))
        .with_ansi(false)
        .try_init();
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::{Arguments, command};

    #[test]
    fn log_level_is_a_subcommand_option() {
        assert!(
            command("dbmcp", "0")
                .try_get_matches_from(["dbmcp", "--log-level", "debug", "stdio"])
                .is_err(),
            "--log-level before the subcommand must not parse"
        );
        for subcommand in ["stdio", "http"] {
            command("dbmcp", "0")
                .try_get_matches_from(["dbmcp", subcommand, "--log-level", "debug"])
                .unwrap_or_else(|e| panic!("{subcommand} --log-level must parse: {e}"));
        }
        for flag in ["--log-level=debug", "--config=dbmcp.yaml"] {
            assert!(
                command("dbmcp", "0")
                    .try_get_matches_from(["dbmcp", "version", flag])
                    .is_err(),
                "version must reject {flag}"
            );
        }
    }

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
