//! Application-level error types.
//!
//! Defines the top-level [`Error`] enum used for server startup and
//! transport failures in the binary crate.

/// Application-level errors for server startup and transport.
///
/// Only instantiated once at program exit, so variant size is irrelevant.
#[derive(Debug, thiserror::Error)]
#[allow(clippy::large_enum_variant)]
pub enum Error {
    /// Database backend initialization failed.
    #[error(transparent)]
    Backend(#[from] database_mcp_backend::AppError),

    /// MCP transport failed to initialize.
    #[error("transport error: {0}")]
    Transport(#[from] rmcp::service::ServerInitializeError),

    /// Network I/O error (e.g., TCP bind failure).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// Missing or invalid configuration at runtime.
    #[error("{0}")]
    Config(String),
}
