//! Configuration types for the dbmcp project.
//!
//! Each concern lives in its own module:
//! - [`config`] — top-level [`Config`] composing the three sections
//! - [`database`] — [`DatabaseConfig`] and [`DatabaseBackend`]
//! - [`http`] — [`HttpConfig`]
//! - [`pii`] — [`PiiConfig`] and [`PiiOperator`]
//! - [`error`] — [`ConfigError`] and the [`ConfigErrors`] wrapper
//!
//! Database connection is configured via individual variables
//! (`DB_HOST`, `DB_PORT`, `DB_USER`, `DB_PASSWORD`, `DB_NAME`,
//! `DB_BACKEND`) instead of a single DSN URL. Values are resolved with
//! clear precedence: CLI flags > environment variables > defaults. All
//! defaults (backend-aware port, user, host) are resolved at construction
//! time in the binary's `From<&Cli>` conversion — consumers access plain
//! values directly.
//!
//! # Security
//!
//! [`DatabaseConfig`] implements [`Debug`] manually to redact the
//! database password.

pub mod config;
pub mod database;
pub mod error;
pub mod http;
pub mod pii;

pub use config::Config;
pub use database::{DatabaseBackend, DatabaseConfig};
pub use error::{ConfigError, ConfigErrors};
pub use http::HttpConfig;
pub use pii::{PiiConfig, PiiOperator};
