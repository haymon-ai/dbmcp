//! Configuration types for the database-mcp project.
//!
//! Provides [`Config`], [`DatabaseConfig`], [`HttpConfig`],
//! [`DatabaseBackend`], and [`ConfigError`] shared across all workspace crates.

mod config;

pub use config::{Config, ConfigError, DatabaseBackend, DatabaseConfig, HttpConfig};
