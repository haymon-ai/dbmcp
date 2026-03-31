//! `PostgreSQL` backend crate.
//!
//! Provides [`PostgresBackend`] implementing the [`backend::DatabaseBackend`] trait.

pub mod postgres;

pub use postgres::PostgresBackend;
