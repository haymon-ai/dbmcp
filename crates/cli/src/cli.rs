//! CLI argument parsing and application bootstrapping.
//!
//! Contains the [`Args`] struct (parsed by clap), the [`Command`]
//! subcommand enum, the [`run`] entry point that dispatches to the active
//! subcommand, and [`start`], which each subcommand calls to resolve its
//! configuration and initialise tracing.

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use dbmcp_config::{Config, ConfigLayer, LogLevel};

use crate::args::{self, SharedArgs};
use crate::error::{Error, ErrorKind};
use crate::http::HttpCommand;
use crate::stdio::StdioCommand;

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
#[command(name = "dbmcp", version, about = "Database MCP Server")]
struct Args {
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

/// Parses CLI arguments and runs the selected subcommand.
///
/// Invalid arguments, `--help`, and `--version` are handled by clap,
/// which prints and exits the process. Must run inside a Tokio runtime.
/// A global tracing subscriber that is already installed is kept.
///
/// # Errors
///
/// Returns an error if the config file cannot be loaded, configuration
/// validation fails, or the selected subcommand fails (transport
/// initialization errors, TCP bind failures, fatal protocol errors).
#[allow(clippy::result_large_err)]
pub async fn run() -> Result<(), Error> {
    match Args::parse().command {
        Command::Version => {
            print!("{}", Args::command().render_version());
            Ok(())
        }
        Command::Stdio(cmd) => Ok(cmd.run().await?),
        Command::Http(cmd) => Ok(cmd.run().await?),
    }
}

/// Loads a subcommand's configuration, initialises tracing, and reports warnings.
///
/// `cli`, `shared`, and `layer` are passed to [`args::load`].
///
/// On success, warnings are logged once tracing runs at the resolved
/// level. On failure, they are printed to stderr ahead of the error.
///
/// # Errors
///
/// Returns [`ErrorKind::Config`] if the configuration cannot be loaded.
#[allow(clippy::result_large_err)]
pub(crate) fn start<C: CommandFactory + FromArgMatches>(
    cli: &C,
    shared: &SharedArgs,
    layer: fn(&C) -> ConfigLayer,
) -> Result<Config, ErrorKind> {
    let mut warnings = Vec::new();
    match args::load(cli, shared, layer, &mut warnings) {
        Ok(resolved) => {
            init_tracing(resolved.log_level);
            for warning in &warnings {
                tracing::warn!("{warning}");
            }
            Ok(resolved.config)
        }
        Err(error) => {
            for warning in &warnings {
                eprintln!("warning: {warning}");
            }
            Err(error.into())
        }
    }
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

    use super::Args;

    #[test]
    fn config_flags_are_grouped_under_config_file_heading() {
        for name in ["stdio", "http"] {
            let help = subcommand_help(name);
            let heading = help.find("Config File:").expect("Config File heading");
            for flag in ["--config <PATH>", "--env <PATH>"] {
                let pos = help.find(flag).unwrap_or_else(|| panic!("{flag} in {name} --help"));
                assert!(heading < pos, "{flag} must follow the Config File heading:\n{help}");
            }
            assert!(help.contains("Path to a YAML config file"));
            assert!(help.contains("Path to a dotenv file; repeatable, later wins. Replaces the .env beside --config"));
            assert!(help.find("Logging:").expect("Logging heading") < help.find("--log-level").expect("--log-level"));
        }
    }

    #[test]
    fn log_level_is_a_subcommand_option() {
        assert!(
            Args::command()
                .try_get_matches_from(["dbmcp", "--log-level", "debug", "stdio"])
                .is_err(),
            "--log-level before the subcommand must not parse"
        );
        for subcommand in ["stdio", "http"] {
            Args::command()
                .try_get_matches_from(["dbmcp", subcommand, "--log-level", "debug"])
                .unwrap_or_else(|e| panic!("{subcommand} --log-level must parse: {e}"));
        }
        for flag in ["--log-level=debug", "--config=dbmcp.yaml"] {
            assert!(
                Args::command()
                    .try_get_matches_from(["dbmcp", "version", flag])
                    .is_err(),
                "version must reject {flag}"
            );
        }
    }

    #[test]
    fn command_uses_binary_name_and_package_version() {
        let cmd = Args::command();
        assert_eq!(cmd.get_name(), "dbmcp");
        assert_eq!(cmd.get_version(), Some(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn secret_env_values_are_hidden_from_help() {
        let cmd = Args::command();
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
        let mut cmd = Args::command();
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
