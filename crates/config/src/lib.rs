//! Configuration types for the dbmcp project.
//!
//! Each concern lives in its own module:
//! - [`config`] — top-level [`Config`] composing the four sections
//! - [`database`] — [`DatabaseConfig`] and [`DatabaseBackend`]
//! - [`dotenv`] — [`Dotenv`] variables read from `.env` files
//! - [`http`] — [`HttpConfig`]
//! - [`layer`] — [`ConfigLayer`] partial settings, merged and resolved into [`Config`]
//! - [`loader`] — [`Sources`] reads the config and env files and layers them under the CLI
//! - [`log_level`] — [`LogLevel`]
//! - [`origin`] — [`AllowedOrigins`] browser `Origin` allowlist matching
//! - [`pii`] — [`PiiConfig`] and [`PiiOperator`]
//! - [`provenance`] — [`Provenance`]: the [`Source`] of each value, used to annotate errors
//! - [`ssh`] — [`SshConfig`] and [`StrictHostKeyChecking`]
//! - [`warning`] — [`ConfigWarning`] non-fatal loading problems
//! - [`error`] — [`ConfigError`] and the [`ConfigErrors`] wrapper
//!
//! Database connection is configured via individual variables
//! (`DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`,
//! `DB_BACKEND`) instead of a single DSN URL. Values are resolved with
//! clear precedence: CLI flags > environment variables > dotenv files >
//! config file > defaults. All defaults (backend-aware port, user, host) are resolved by
//! [`ConfigLayer::resolve`] — consumers access plain values directly.
//!
//! # Security
//!
//! The database password, SSH password and key passphrase, and HTTP auth
//! token are stored as [`SecretString`], so their values never reach
//! [`Debug`] output and are zeroized on drop.

pub mod config;
pub mod database;
pub mod dotenv;
pub mod error;
pub mod http;
mod interpolate;
pub mod layer;
pub mod loader;
pub mod log_level;
pub mod origin;
pub mod pii;
pub mod provenance;
pub mod ssh;
mod value_enum;
pub mod warning;
mod yaml;

pub use config::Config;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use dotenv::{Dotenv, DotenvVar};
pub use error::{ConfigError, ConfigErrors};
pub use http::HttpConfig;
pub use layer::{ConfigLayer, DatabaseLayer, HttpLayer, PiiLayer, Resolved, SshLayer, Transport};
pub use loader::{LoadError, Sources};
pub use log_level::LogLevel;
pub use origin::AllowedOrigins;
pub use pii::{PiiCategory, PiiConfig, PiiOperator};
pub use provenance::{Provenance, Route, Source};
pub use secrecy::{ExposeSecret, SecretString};
pub use ssh::{SshConfig, StrictHostKeyChecking};
pub use warning::ConfigWarning;

/// Helpers shared by this crate's unit tests.
#[cfg(test)]
mod test_support {
    use std::path::PathBuf;

    /// Returns a fixed home directory for `~` expansion tests.
    pub(crate) fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }
}
