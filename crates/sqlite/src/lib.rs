//! `SQLite` backend crate.
//!
//! Provides [`SqliteBackend`] implementing the [`backend::DatabaseBackend`] trait.

pub mod sqlite;

pub use sqlite::SqliteBackend;
