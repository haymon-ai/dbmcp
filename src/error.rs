//! Application-level error types.
//!
//! Defines the top-level [`Error`] enum used for server startup and
//! transport failures in the binary crate.

use dbmcp_config::ConfigError;

/// Application-level errors for server startup and transport.
///
/// Only instantiated once at program exit, so variant size is irrelevant.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub(crate) enum Error {
    /// MCP transport failed to initialize.
    #[error("transport error: {0}")]
    Transport(#[from] rmcp::service::ServerInitializeError),

    /// Network I/O error (e.g., TCP bind failure).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Configuration validation failed with one or more errors.
    #[error("{}", format_config_errors(.0))]
    Config(Vec<ConfigError>),
}

impl From<Vec<ConfigError>> for Error {
    fn from(errors: Vec<ConfigError>) -> Self {
        Self::Config(errors)
    }
}

fn format_config_errors(errors: &[ConfigError]) -> String {
    let mut s = String::from("configuration validation failed:");
    for error in errors {
        s.push_str("\n  - ");
        s.push_str(&error.to_string());
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_error_display_bullets_each_error() {
        let error = Error::Config(vec![ConfigError::MissingSqliteDbName, ConfigError::EmptyHttpHost]);
        let rendered = error.to_string();
        assert!(rendered.starts_with("configuration validation failed:"));
        assert!(rendered.contains("\n  - DB_NAME (file path) is required for SQLite"));
        assert!(rendered.contains("\n  - HTTP_HOST must not be empty"));
    }

    #[test]
    fn config_error_from_vec() {
        let error: Error = vec![ConfigError::EmptyHttpHost].into();
        assert!(matches!(error, Error::Config(_)));
    }
}
