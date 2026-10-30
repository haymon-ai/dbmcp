//! Application bootstrapping.
//!
//! Contains the [`run`] entry point: parses [`Arguments`], loads the
//! configuration, initialises tracing, and serves the selected transport.

use clap::{CommandFactory, FromArgMatches};
use dbmcp_config::LogLevel;
use tracing::warn;

use crate::args::Arguments;
use crate::error::{Error, ErrorKind};
use crate::server;

/// Builds the clap command for the binary named `bin` at `version`.
fn command(bin: &'static str, version: &'static str) -> clap::Command {
    Arguments::command().name(bin).version(version)
}

/// Parses CLI arguments, loads the config, initializes tracing, and serves the transport.
///
/// # Errors
///
/// Returns an error if the command-line arguments are invalid, the
/// configuration cannot be loaded or fails validation, or the transport fails (transport
/// initialization errors, TCP bind failures, fatal protocol errors).
///
/// # Panics
///
/// Never in practice: the HTTP config is always built alongside the
/// `http` transport.
///
/// `bin` and `version` are the installed binary's name and version, used
/// in `--help` and `--version`. Must run inside a Tokio runtime. A global
/// tracing subscriber that is already installed is kept.
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

    let options = arguments.load_options().map_err(ErrorKind::from)?;
    let loaded = dbmcp_config::load(&options);
    // The log level is a setting, so logging starts once loading is done;
    // a failed load logs its warnings at the default level.
    let (log_level, warnings) = match &loaded {
        Ok(loaded) => (loaded.log_level, &loaded.warnings),
        Err(error) => (LogLevel::default(), &error.warnings),
    };
    // Fails only if a subscriber is already installed; keep that one.
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level(log_level))
        .with_ansi(false)
        .try_init();
    for warning in warnings {
        warn!("{warning}");
    }

    let config = loaded.map_err(ErrorKind::from)?.config;
    Ok(server::serve(&config).await?)
}

/// Maps the configured log level to its tracing level.
fn level(level: LogLevel) -> tracing::Level {
    match level {
        LogLevel::Error => tracing::Level::ERROR,
        LogLevel::Warn => tracing::Level::WARN,
        LogLevel::Info => tracing::Level::INFO,
        LogLevel::Debug => tracing::Level::DEBUG,
        LogLevel::Trace => tracing::Level::TRACE,
    }
}

#[cfg(test)]
mod tests {
    use super::command;

    #[test]
    fn command_uses_given_name_and_version() {
        let cmd = command("dbmcp", "1.2.3");
        assert_eq!(cmd.get_name(), "dbmcp");
        assert_eq!(cmd.get_version(), Some("1.2.3"));
    }
}
