//! Database-agnostic row-to-JSON conversion for sqlx.
//!
//! Provides the [`RowExt`] trait for converting a single database row
//! into a [`Value::Object`]. Implementations are provided for
//! [`SqliteRow`](sqlx::sqlite::SqliteRow),
//! [`PgRow`](sqlx::postgres::PgRow), and
//! [`MySqlRow`](sqlx::mysql::MySqlRow).

mod mysql;
mod postgres;
mod sqlite;
mod traits;

pub use traits::RowExt;
