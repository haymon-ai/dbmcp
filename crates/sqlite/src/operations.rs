//! `SQLite` database query operations.

use super::types::{DropTableRequest, ExplainQueryRequest, QueryRequest};
use database_mcp_server::AppError;
use database_mcp_server::types::{ListTablesResponse, MessageResponse, QueryResponse};
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use database_mcp_sql::validation::validate_read_only_with_dialect;
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx_to_json::RowExt;

use super::SqliteAdapter;

impl SqliteAdapter {
    /// Lists all tables in the connected database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub(crate) async fn list_tables(&self) -> Result<ListTablesResponse, AppError> {
        let pool = self.pool.clone();
        let sql = "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(ListTablesResponse {
            tables: rows.into_iter().map(|r| r.0).collect(),
        })
    }

    /// Drops a table from the database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] in read-only mode,
    /// [`AppError::InvalidIdentifier`] for invalid names,
    /// or [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn drop_table(&self, request: &DropTableRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let table = &request.table_name;
        validate_identifier(table)?;

        let pool = self.pool.clone();
        let drop_sql = format!("DROP TABLE {}", Self::quote_identifier(table));
        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }

    /// Returns the execution plan for a query.
    ///
    /// Always uses `EXPLAIN QUERY PLAN` — `SQLite` does not support
    /// `EXPLAIN ANALYZE`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn explain_query(&self, request: &ExplainQueryRequest) -> Result<QueryResponse, AppError> {
        let pool = self.pool.clone();
        let explain_sql = format!("EXPLAIN QUERY PLAN {}", request.query);
        let rows: Vec<SqliteRow> = execute_with_timeout(
            self.config.query_timeout,
            &explain_sql,
            sqlx::query(&explain_sql).fetch_all(&pool),
        )
        .await?;
        Ok(QueryResponse {
            rows: Value::Array(rows.iter().map(RowExt::to_json).collect()),
        })
    }

    /// Executes a SQL query and returns rows as JSON.
    async fn execute_query(&self, sql: &str) -> Result<Value, AppError> {
        let pool = self.pool.clone();
        let rows: Vec<SqliteRow> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query(sql).fetch_all(&pool)).await?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    /// Executes a read-only SQL query.
    ///
    /// Validates that the query is read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn read_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::SQLiteDialect {})?;
        let rows = self.execute_query(&request.query).await?;
        Ok(QueryResponse { rows })
    }

    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub(crate) async fn write_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        let rows = self.execute_query(&request.query).await?;
        Ok(QueryResponse { rows })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;
    use sqlx::sqlite::SqlitePoolOptions;

    // Row-to-JSON conversion tests live in crates/sqlx_to_json.
    // These tests cover the array-level wrapping done by execute_query.

    /// Helper: creates an in-memory `SQLite` pool for unit tests.
    async fn mem_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite")
    }

    /// Helper: runs a query and converts all rows via [`RowExt::to_json`].
    async fn query_json(pool: &SqlitePool, sql: &str) -> Value {
        let rows: Vec<SqliteRow> = sqlx::query(sql).fetch_all(pool).await.expect("query failed");
        Value::Array(rows.iter().map(RowExt::to_json).collect())
    }

    #[tokio::test]
    async fn execute_query_empty_result() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v INTEGER)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT v FROM t").await;
        assert_eq!(rows, Value::Array(vec![]));
    }

    #[tokio::test]
    async fn execute_query_multiple_rows() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER, name TEXT, score REAL)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES (1, 'alice', 9.5), (2, 'bob', 8.0)")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT id, name, score FROM t ORDER BY id").await;
        assert_eq!(rows.as_array().expect("should be array").len(), 2);

        assert_eq!(rows[0]["id"], Value::Number(1.into()));
        assert_eq!(rows[0]["name"], Value::String("alice".into()));
        assert!(rows[0]["score"].is_number());

        assert_eq!(rows[1]["id"], Value::Number(2.into()));
        assert_eq!(rows[1]["name"], Value::String("bob".into()));
    }
}
