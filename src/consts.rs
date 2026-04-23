//! Compile-time constants for the dbmcp binary.

/// The name of the binary, derived from `Cargo.toml` at compile time.
pub(crate) const BIN: &str = env!("CARGO_PKG_NAME");

/// The current version, derived from `Cargo.toml` at compile time.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
