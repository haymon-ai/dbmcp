//! Configuration loading and types for the dbmcp project.
//!
//! [`load`] builds a validated [`Config`] from the process env, an optional
//! env file, and an optional YAML file. Each concern lives in its own
//! module:
//! - [`options`] — [`LoadOptions`], the loader inputs
//! - [`loader`] — [`load`], [`Loaded`], and [`LoadError`]
//! - [`env`](mod@env) — the table pairing each YAML key path with its `DBMCP_*` name
//! - [`source`] — [`Source`], where a setting's value came from
//! - [`log`] — [`LogLevel`] selector
//! - [`config`] — top-level [`Config`] composing the transport and four sections
//! - [`database`] — [`DatabaseConfig`] and [`DatabaseBackend`]
//! - [`http`] — [`HttpConfig`]
//! - [`origin`] — [`AllowedOrigins`] browser `Origin` allowlist matching
//! - [`pii`] — [`PiiConfig`] and [`PiiOperator`]
//! - [`secret`] — [`Secret`] redacting string wrapper
//! - [`ssh`] — [`SshConfig`] and [`StrictHostKeyChecking`]
//! - [`transport`] — [`Transport`] MCP transport selector
//! - [`error`] — [`ConfigError`] and the [`ConfigErrors`] wrapper
//!
//! Settings resolve with this precedence: non-empty process env >
//! non-empty env file value > YAML value > default. All defaults
//! (backend-aware port, user, host) are resolved by the loader, so
//! consumers access plain values directly.
//!
//! # Security
//!
//! The database password, SSH password and key passphrase, and HTTP auth
//! token are stored as [`Secret`], so their values never reach [`Debug`]
//! output.

pub mod config;
pub mod database;
mod dotenv;
pub mod env;
pub mod error;
pub mod http;
mod interpolate;
pub mod loader;
pub mod log;
mod node;
pub mod options;
pub mod origin;
pub mod pii;
pub mod secret;
pub mod source;
pub mod ssh;
mod template;
pub mod transport;

pub use config::Config;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use error::{ConfigError, ConfigErrors};
pub use http::HttpConfig;
pub use loader::{LoadError, Loaded, load};
pub use log::LogLevel;
pub use options::LoadOptions;
pub use origin::AllowedOrigins;
pub use pii::{PiiCategory, PiiConfig, PiiOperator};
pub use secret::Secret;
pub use source::Source;
pub use ssh::{SshConfig, StrictHostKeyChecking};
pub use transport::Transport;
