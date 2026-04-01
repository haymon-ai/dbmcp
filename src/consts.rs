//! Compile-time constants for the database-mcp binary.

/// The name of the binary, derived from `Cargo.toml` at compile time.
pub const BIN: &str = env!("CARGO_PKG_NAME");

/// The current version, derived from `Cargo.toml` at compile time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
