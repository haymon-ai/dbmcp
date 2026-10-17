//! Compile-time constants for the dbmcp CLI.

/// The installed binary name, used as clap command name and in `version` output.
pub(crate) const BIN: &str = "dbmcp";

/// The current version, derived from `Cargo.toml` at compile time.
pub(crate) const VERSION: &str = env!("CARGO_PKG_VERSION");
