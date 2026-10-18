//! Application-level error types.
//!
//! Defines the opaque public [`Error`] returned by [`crate::run`] and the
//! internal [`ErrorKind`] used for server startup and transport failures.

use std::process::ExitCode;

use dbmcp_config::LoadError;

/// Error returned when the CLI fails to start or run the server.
///
/// The underlying cause is private; render it with [`std::fmt::Display`].
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(#[from] ErrorKind);

impl Error {
    /// Returns the process exit code that matches this error.
    ///
    /// Invalid command-line arguments keep clap's usage exit code; every
    /// other failure maps to [`ExitCode::FAILURE`].
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match &self.0 {
            ErrorKind::Arguments(error) => u8::try_from(error.exit_code()).map_or(ExitCode::FAILURE, ExitCode::from),
            _ => ExitCode::FAILURE,
        }
    }
}

/// Internal causes of an [`Error`].
///
/// Only returned once to the `dbmcp` binary's `main` at exit, so variant size is irrelevant.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ErrorKind {
    /// Command-line arguments could not be parsed.
    ///
    /// clap's rendered message ends with a newline, which is trimmed here.
    #[error("{}", .0.to_string().trim_end())]
    Arguments(#[from] clap::Error),

    /// MCP transport failed to initialize.
    #[error("transport error: {0}")]
    Transport(#[from] rmcp::service::ServerInitializeError),

    /// Network I/O error (e.g., TCP bind failure).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The config file could not be loaded or the configuration is invalid.
    #[error(transparent)]
    Config(#[from] LoadError),

    /// Server initialisation failed (e.g. fail-closed NER model load).
    #[error("server initialisation failed: {0}")]
    Init(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbmcp_config::{ConfigError, ConfigErrors};

    #[test]
    fn config_error_display_bullets_each_error() {
        let errors = ConfigErrors::from_vec(vec![
            ConfigError::MissingSqliteDbName,
            ConfigError::FileNotFound("DB_SSL_CA".into(), "/nope".into()),
        ])
        .expect("non-empty");
        let error = ErrorKind::Config(errors.into());
        let rendered = error.to_string();
        assert!(rendered.starts_with("configuration validation failed:"));
        assert!(rendered.contains("\n  - DB_NAME (file path) is required for SQLite"));
        assert!(rendered.contains("\n  - DB_SSL_CA file not found: /nope"));
    }

    #[test]
    fn config_error_from_load_error() {
        let errors: ConfigErrors = ConfigError::MissingSqliteDbName.into();
        let error: ErrorKind = LoadError::from(errors).into();
        assert!(matches!(error, ErrorKind::Config(_)));
    }

    #[test]
    fn argument_error_keeps_clap_exit_code() {
        let clap_error = clap::Error::new(clap::error::ErrorKind::UnknownArgument);
        let error = Error::from(ErrorKind::from(clap_error));
        assert_eq!(error.exit_code(), ExitCode::from(2));
    }

    #[test]
    fn other_errors_exit_with_failure() {
        let errors: ConfigErrors = ConfigError::MissingSqliteDbName.into();
        let error = Error::from(ErrorKind::from(LoadError::from(errors)));
        assert_eq!(error.exit_code(), ExitCode::FAILURE);
    }
}
