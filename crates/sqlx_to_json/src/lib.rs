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

use serde_json::Value;

/// Converts a single database row into a JSON object.
pub trait RowExt {
    /// Converts this row's columns to a JSON object.
    ///
    /// Each column becomes a key in the returned object, with values
    /// converted to the most appropriate JSON type. `NULL` columns
    /// produce [`Value::Null`].
    ///
    /// # Returns
    ///
    /// A [`Value::Object`] where keys are column names and values are
    /// type-appropriate JSON values.
    fn to_json(&self) -> Value;
}
