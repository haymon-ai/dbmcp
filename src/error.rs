//! Application-level error types.
//!
//! Defines the top-level [`Error`] enum used for server startup and
//! transport failures in the binary crate.

use dbmcp_config::ConfigErrors;

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
    #[error("{}", BulletList(.0))]
    Config(#[from] ConfigErrors),
}

struct BulletList<'a>(&'a ConfigErrors);

impl std::fmt::Display for BulletList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "configuration validation failed:")?;
        for error in self.0.iter() {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbmcp_config::ConfigError;

    #[test]
    fn config_error_display_bullets_each_error() {
        let errors = ConfigErrors::from_vec(vec![
            ConfigError::MissingSqliteDbName,
            ConfigError::SslCertNotFound("DB_SSL_CA".into(), "/nope".into()),
        ])
        .expect("non-empty");
        let error = Error::Config(errors);
        let rendered = error.to_string();
        assert!(rendered.starts_with("configuration validation failed:"));
        assert!(rendered.contains("\n  - DB_NAME (file path) is required for SQLite"));
        assert!(rendered.contains("\n  - DB_SSL_CA file not found: /nope"));
    }

    #[test]
    fn config_error_from_config_errors() {
        let errors: ConfigErrors = ConfigError::MissingSqliteDbName.into();
        let error: Error = errors.into();
        assert!(matches!(error, Error::Config(_)));
    }
}
