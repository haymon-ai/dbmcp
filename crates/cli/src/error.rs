//! Application-level error types.
//!
//! Defines the opaque public [`Error`] returned by [`crate::run`] and the
//! internal [`ErrorKind`] used for server startup and transport failures.

use dbmcp_config::LoadError;

/// Error returned when the CLI fails to start or run the server.
///
/// The underlying cause is private; render it with [`std::fmt::Display`].
#[derive(Debug, thiserror::Error)]
#[error(transparent)]
pub struct Error(#[from] ErrorKind);

/// Internal causes of an [`Error`].
///
/// Only returned once to the `dbmcp` binary's `main` at exit, so variant size is irrelevant.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum ErrorKind {
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
            ConfigError::FileNotFound {
                field: "database.ssl_ca",
                path: "/nope".into(),
            },
        ])
        .expect("non-empty");
        let error = ErrorKind::Config(errors.into());
        let rendered = error.to_string();
        assert!(rendered.starts_with("configuration validation failed:"));
        assert!(rendered.contains("\n  - database.name: required (file path) for the sqlite backend"));
        assert!(rendered.contains("\n  - database.ssl_ca: file not found: /nope"));
    }

    #[test]
    fn config_error_from_load_error() {
        let errors: ConfigErrors = ConfigError::MissingSqliteDbName.into();
        let error: ErrorKind = LoadError::from(errors).into();
        assert!(matches!(error, ErrorKind::Config(_)));
    }
}
