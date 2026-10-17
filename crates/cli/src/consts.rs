//! Compile-time constants for the dbmcp binary.

/// The installed binary name, printed by the `version` subcommand.
pub(crate) const BIN: &str = "dbmcp";

/// The current version, derived from `Cargo.toml` at compile time.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
