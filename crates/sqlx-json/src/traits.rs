//! Row-to-JSON conversion and query-result traits.

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

/// Extracts the affected row count and insert id from a backend query result.
///
/// sqlx's `Database::QueryResult` associated type exposes these as
/// inherent methods on each concrete backend type.  This trait provides
/// a single generic bound so the blanket `Connection` impl can call them
/// without knowing the concrete type.
pub trait QueryResult {
    /// Returns the number of rows affected by the executed statement.
    fn rows_affected(&self) -> u64;

    /// Returns the id the executed statement generated, if the backend knows it.
    fn last_insert_id(&self) -> Option<u64>;
}

impl QueryResult for sqlx::mysql::MySqlQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }

    /// The first `AUTO_INCREMENT` id; `MySQL` reports `0` when none was generated.
    fn last_insert_id(&self) -> Option<u64> {
        Some(self.last_insert_id()).filter(|id| *id != 0)
    }
}

impl QueryResult for sqlx::postgres::PgQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }

    /// Always `None`: `PostgreSQL` returns generated keys through `RETURNING`.
    fn last_insert_id(&self) -> Option<u64> {
        None
    }
}

impl QueryResult for sqlx::sqlite::SqliteQueryResult {
    fn rows_affected(&self) -> u64 {
        self.rows_affected()
    }

    /// Always `None`: `SQLite` keeps the last rowid per connection, not per statement.
    fn last_insert_id(&self) -> Option<u64> {
        None
    }
}
