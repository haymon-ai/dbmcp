//! Configuration loading and types for the dbmcp project.
//!
//! [`load`] builds a validated [`Config`] from the process env, an optional
//! env file, and an optional YAML file. The crate's interface is [`load`],
//! its inputs ([`LoadOptions`], [`home_dir`]), its outcome ([`Loaded`],
//! [`LoadError`]), and the plain configuration types; the setting table,
//! sources, and diagnostics stay internal.
//!
//! Settings resolve with this precedence: non-empty process env >
//! non-empty env file value > YAML value > default. All defaults
//! (backend-aware port, user, host) are resolved by the loader, so
//! consumers access plain values directly.
//!
//! # Security
//!
//! The database password, SSH password and key passphrase, and HTTP auth
//! token are stored as [`secrecy::SecretString`], so their values never
//! reach [`Debug`] output.

mod config;
mod database;
mod dotenv;
mod env;
mod error;
mod http;
mod interpolate;
mod loader;
mod log;
mod node;
mod options;
mod origin;
mod pii;
mod schema;
mod settings;
mod source;
mod ssh;
mod template;
mod transport;

pub use config::Config;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use http::HttpConfig;
pub use loader::{LoadError, Loaded, load};
pub use log::LogLevel;
pub use options::{LoadOptions, home_dir};
pub use origin::AllowedOrigins;
pub use pii::{PiiCategory, PiiConfig, PiiOperator};
pub use ssh::{SshConfig, StrictHostKeyChecking};
pub use transport::Transport;
