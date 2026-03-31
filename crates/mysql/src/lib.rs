//! MySQL/MariaDB backend crate.
//!
//! Provides [`MysqlBackend`] implementing the [`backend::DatabaseBackend`] trait.

pub mod mysql;

pub use mysql::MysqlBackend;
