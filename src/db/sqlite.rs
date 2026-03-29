//! `SQLite` backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `SQLite` file-based databases.

use crate::config::DatabaseConfig;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use serde_json::{Value, json};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions, SqliteRow};
use sqlx::{Row, SqlitePool};
use sqlx_to_json::RowExt;
use std::collections::HashMap;
use tracing::info;

/// Converts [`DatabaseConfig`] into [`SqliteConnectOptions`].
impl From<&DatabaseConfig> for SqliteConnectOptions {
    fn from(config: &DatabaseConfig) -> Self {
        let name = config.name.as_deref().unwrap_or_default();
        SqliteConnectOptions::new().filename(name)
    }
}

/// `SQLite` file-based database backend.
#[derive(Clone)]
pub struct SqliteBackend {
    pool: SqlitePool,
    pub read_only: bool,
}

impl std::fmt::Debug for SqliteBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SqliteBackend")
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl SqliteBackend {
    /// Creates a lazy in-memory backend for tests.
    #[cfg(test)]
    pub(crate) fn in_memory(read_only: bool) -> Self {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_lazy("sqlite::memory:")
            .expect("in-memory SQLite");
        Self { pool, read_only }
    }

    /// Creates a new `SQLite` backend from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the database file cannot be opened.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let name = config.name.as_deref().unwrap_or_default();
        let pool = SqlitePoolOptions::new()
            .max_connections(1) // SQLite is single-writer
            .connect_with(config.into())
            .await
            .map_err(|e| AppError::Connection(format!("Failed to open SQLite: {e}")))?;

        info!("SQLite connection initialized: {name}");

        Ok(Self {
            pool,
            read_only: config.read_only,
        })
    }
}

impl SqliteBackend {
    /// Wraps `name` in double quotes for safe use in `SQLite` SQL statements.
    ///
    /// Escapes internal double quotes by doubling them.
    fn quote_identifier(name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
}

impl DatabaseBackend for SqliteBackend {
    #[allow(clippy::unused_async)]
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        // SQLite has one database: "main"
        Ok(vec!["main".to_string()])
    }

    async fn list_tables(&self, _database: &str) -> Result<Vec<String>, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_table_schema(&self, _database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(table)?;
        let rows: Vec<SqliteRow> = sqlx::query(&format!("PRAGMA table_info({})", Self::quote_identifier(table)))
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.to_string()));
        }

        let mut schema: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name: String = row.try_get("name").unwrap_or_default();
            let col_type: String = row.try_get("type").unwrap_or_default();
            let notnull: i32 = row.try_get("notnull").unwrap_or(0);
            let default: Option<String> = row.try_get("dflt_value").ok();
            let pk: i32 = row.try_get("pk").unwrap_or(0);
            schema.insert(
                col_name,
                json!({
                    "type": col_type,
                    "nullable": notnull == 0,
                    "key": if pk > 0 { "PRI" } else { "" },
                    "default": default,
                    "extra": Value::Null,
                }),
            );
        }
        Ok(json!(schema))
    }

    async fn get_table_schema_with_relations(&self, database: &str, table: &str) -> Result<Value, AppError> {
        let schema = self.get_table_schema(database, table).await?;
        let mut columns: HashMap<String, Value> = serde_json::from_value(schema).unwrap_or_default();

        // Add null foreign_key to all columns
        for col in columns.values_mut() {
            if let Some(obj) = col.as_object_mut() {
                obj.entry("foreign_key".to_string()).or_insert(Value::Null);
            }
        }

        // Get FK info via PRAGMA
        let fk_rows: Vec<SqliteRow> =
            sqlx::query(&format!("PRAGMA foreign_key_list({})", Self::quote_identifier(table)))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let from_col: String = fk_row.try_get("from").unwrap_or_default();
            if let Some(col_info) = columns.get_mut(&from_col)
                && let Some(obj) = col_info.as_object_mut()
            {
                let ref_table: String = fk_row.try_get("table").unwrap_or_default();
                let ref_col: String = fk_row.try_get("to").unwrap_or_default();
                let on_update: String = fk_row.try_get("on_update").unwrap_or_default();
                let on_delete: String = fk_row.try_get("on_delete").unwrap_or_default();
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": Value::Null,
                        "referenced_table": ref_table,
                        "referenced_column": ref_col,
                        "on_update": on_update,
                        "on_delete": on_delete,
                    }),
                );
            }
        }

        Ok(json!({
            "table_name": table,
            "columns": columns,
        }))
    }

    async fn execute_query(&self, sql: &str, _database: Option<&str>) -> Result<Value, AppError> {
        let rows: Vec<SqliteRow> = sqlx::query(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    #[allow(clippy::unused_async)]
    async fn create_database(&self, _name: &str) -> Result<Value, AppError> {
        Ok(json!({
            "status": "unsupported",
            "message": "SQLite does not support creating databases. Use --db-path to specify the database file.",
        }))
    }

    fn dialect(&self) -> Box<dyn sqlparser::dialect::Dialect> {
        Box::new(sqlparser::dialect::SQLiteDialect {})
    }

    fn read_only(&self) -> bool {
        self.read_only
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseBackend;

    #[test]
    fn quote_identifier_wraps_in_double_quotes() {
        assert_eq!(SqliteBackend::quote_identifier("users"), "\"users\"");
        assert_eq!(SqliteBackend::quote_identifier("eu-docker"), "\"eu-docker\"");
    }

    #[test]
    fn quote_identifier_escapes_double_quotes() {
        assert_eq!(SqliteBackend::quote_identifier("test\"db"), "\"test\"\"db\"");
        assert_eq!(SqliteBackend::quote_identifier("a\"b\"c"), "\"a\"\"b\"\"c\"");
    }

    #[test]
    fn try_from_sets_filename() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some("test.db".into()),
            ..DatabaseConfig::default()
        };
        let opts = SqliteConnectOptions::from(&config);

        assert_eq!(opts.get_filename().to_str().expect("valid path"), "test.db");
    }

    #[test]
    fn try_from_empty_name_defaults() {
        let config = DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: None,
            ..DatabaseConfig::default()
        };
        let opts = SqliteConnectOptions::from(&config);

        // Empty string filename — validated elsewhere by Config::validate()
        assert_eq!(opts.get_filename().to_str().expect("valid path"), "");
    }

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
