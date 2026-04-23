//! Database-agnostic row-to-JSON conversion and query result traits for sqlx.
//!
//! Provides [`RowExt`] for converting a single database row into a
//! [`Value::Object`] and [`QueryResult`] for extracting affected row
//! counts.  Implementations are provided for
//! [`SqliteRow`](sqlx::sqlite::SqliteRow),
//! [`PgRow`](sqlx::postgres::PgRow), and
//! [`MySqlRow`](sqlx::mysql::MySqlRow).

mod mysql;
mod postgres;
mod sqlite;
mod traits;

pub use traits::{QueryResult, RowExt};
