//! Configuration types for the dbmcp project.
//!
//! Each concern lives in its own module:
//! - [`config`] — top-level [`Config`] composing the four sections
//! - [`database`] — [`DatabaseConfig`] and [`DatabaseBackend`]
//! - [`http`] — [`HttpConfig`]
//! - [`layer`] — [`ConfigLayer`] partial settings, merged and resolved into [`Config`]
//! - [`loader`] — [`load`] reads the YAML config file and layers it under the CLI
//! - [`log_level`] — [`LogLevel`]
//! - [`origin`] — [`AllowedOrigins`] browser `Origin` allowlist matching
//! - [`pii`] — [`PiiConfig`] and [`PiiOperator`]
//! - [`secret`] — [`Secret`] redacting string wrapper
//! - [`ssh`] — [`SshConfig`] and [`StrictHostKeyChecking`]
//! - [`error`] — [`ConfigError`] and the [`ConfigErrors`] wrapper
//!
//! Database connection is configured via individual variables
//! (`DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`,
//! `DB_BACKEND`) instead of a single DSN URL. Values are resolved with
//! clear precedence: CLI flags > environment variables > config file >
//! defaults. All defaults (backend-aware port, user, host) are resolved by
//! [`ConfigLayer::resolve`] — consumers access plain values directly.
//!
//! # Security
//!
//! The database password, SSH password and key passphrase, and HTTP auth
//! token are stored as [`Secret`], so their values never reach [`Debug`]
//! output.

pub mod config;
pub mod database;
pub mod error;
pub mod http;
pub mod layer;
pub mod loader;
pub mod log_level;
pub mod origin;
pub mod pii;
pub mod secret;
pub mod ssh;
mod value_enum;

pub use config::Config;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use error::{ConfigError, ConfigErrors};
pub use http::HttpConfig;
pub use layer::{ConfigLayer, DatabaseLayer, HttpLayer, PiiLayer, Resolved, SshLayer, Transport};
pub use loader::{LoadError, load};
pub use log_level::LogLevel;
pub use origin::AllowedOrigins;
pub use pii::{PiiCategory, PiiConfig, PiiOperator};
pub use secret::Secret;
pub use ssh::{SshConfig, StrictHostKeyChecking};
