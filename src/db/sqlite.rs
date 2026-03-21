//! `SQLite` backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `SQLite` file-based databases.

use crate::config::DatabaseConfig;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value, json};
use sqlx::sqlite::{SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row, SqlitePool, TypeInfo, ValueRef};
use std::collections::HashMap;
use tracing::info;

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
    /// Creates a new `SQLite` backend from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the database file cannot be opened.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let name = config.name.as_deref().unwrap_or_default();
        let url = format!("sqlite:{name}");
        let pool = SqlitePoolOptions::new()
            .max_connections(1) // SQLite is single-writer
            .connect(&url)
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

    async fn execute_query(&self, sql: &str, _database: Option<&str>) -> Result<Vec<Map<String, Value>>, AppError> {
        let rows: Vec<SqliteRow> = sqlx::query(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        Ok(rows.iter().map(sqlite_row_to_json).collect())
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

/// Converts a `SQLite` row to a JSON object with type-aware value extraction.
///
/// Uses `column.type_info().name()` to pick the right Rust type for each column.
/// Unknown or `NULL`-declared types (e.g., `COUNT(*)`) fall back to probing:
/// i64 → f64 → String → Null.
fn sqlite_row_to_json(row: &SqliteRow) -> Map<String, Value> {
    let columns = row.columns();
    let mut map = Map::with_capacity(columns.len());

    for column in columns {
        let idx = column.ordinal();
        let type_name = column.type_info().name();

        let value = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
            Value::Null
        } else {
            match type_name {
                "BOOLEAN" | "BOOL" => row.try_get::<bool, _>(idx).map(Value::Bool).unwrap_or(Value::Null),

                "INTEGER" | "INT" | "BIGINT" | "SMALLINT" | "TINYINT" | "MEDIUMINT" => row
                    .try_get::<i64, _>(idx)
                    .map(|v| Value::Number(v.into()))
                    .unwrap_or(Value::Null),

                "REAL" | "FLOAT" | "DOUBLE" | "NUMERIC" => row
                    .try_get::<f64, _>(idx)
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::Null, Value::Number),

                "BLOB" => row
                    .try_get::<Vec<u8>, _>(idx)
                    .map_or(Value::Null, |bytes| Value::String(BASE64.encode(&bytes))),

                "TEXT" | "VARCHAR" | "CHAR" | "CLOB" | "DATE" | "DATETIME" | "TIMESTAMP" | "TIME" => {
                    row.try_get::<String, _>(idx).map(Value::String).unwrap_or(Value::Null)
                }

                // SQLite reports "NULL" type for expressions like COUNT(*), SUM().
                // The value is not null (checked above), so probe: i64 → f64 → String.
                _ => sqlite_dynamic_probe(row, idx),
            }
        };

        map.insert(column.name().to_string(), value);
    }

    map
}

/// Probes a `SQLite` value by trying types in order: i64 → f64 → String → Null.
fn sqlite_dynamic_probe(row: &SqliteRow, idx: usize) -> Value {
    if let Ok(n) = row.try_get::<i64, _>(idx) {
        return Value::Number(n.into());
    }
    if let Ok(f) = row.try_get::<f64, _>(idx) {
        return serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number);
    }
    row.try_get::<String, _>(idx).map(Value::String).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

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

    /// Helper: creates an in-memory `SQLite` pool for unit tests.
    async fn mem_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite")
    }

    /// Helper: runs a query and converts all rows via [`sqlite_row_to_json`].
    async fn query_json(pool: &SqlitePool, sql: &str) -> Vec<Map<String, Value>> {
        let rows: Vec<SqliteRow> = sqlx::query(sql).fetch_all(pool).await.expect("query failed");
        rows.iter().map(sqlite_row_to_json).collect()
    }

    #[tokio::test]
    async fn row_to_json_integer_types() {
        let pool = mem_pool().await;
        let rows = query_json(&pool, "SELECT 42 AS val").await;
        assert_eq!(rows[0]["val"], Value::Number(42.into()));
    }

    #[tokio::test]
    async fn row_to_json_typed_integer_column() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER PRIMARY KEY, n BIGINT)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES (1, 9999999999)")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT id, n FROM t").await;
        assert_eq!(rows[0]["id"], Value::Number(1.into()));
        assert_eq!(rows[0]["n"], Value::Number(9_999_999_999_i64.into()));
    }

    #[tokio::test]
    async fn row_to_json_real_type() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v REAL)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (3.14)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT v FROM t").await;
        let n = rows[0]["v"].as_f64().expect("should be f64");
        assert!((n - 3.14).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn row_to_json_text_type() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (name TEXT)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES ('hello')")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT name FROM t").await;
        assert_eq!(rows[0]["name"], Value::String("hello".into()));
    }

    #[tokio::test]
    async fn row_to_json_boolean_type() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (flag BOOLEAN)")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES (1)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT flag FROM t").await;
        assert_eq!(rows[0]["flag"], Value::Bool(true));
    }

    #[tokio::test]
    async fn row_to_json_null_value() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (NULL)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT v FROM t").await;
        assert_eq!(rows[0]["v"], Value::Null);
    }

    #[tokio::test]
    async fn row_to_json_blob_base64() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (data BLOB)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (X'DEADBEEF')")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT data FROM t").await;
        assert_eq!(rows[0]["data"], Value::String(BASE64.encode(b"\xDE\xAD\xBE\xEF")));
    }

    #[tokio::test]
    async fn row_to_json_count_aggregate() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (id INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (1),(2),(3)")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT COUNT(*) AS cnt FROM t").await;
        assert_eq!(rows[0]["cnt"], Value::Number(3.into()), "COUNT(*) must be a number");
    }

    #[tokio::test]
    async fn row_to_json_sum_aggregate() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v INTEGER)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (10),(20),(30)")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT SUM(v) AS total FROM t").await;
        assert_eq!(rows[0]["total"], Value::Number(60.into()));
    }

    #[tokio::test]
    async fn row_to_json_avg_aggregate() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v REAL)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES (1.0),(2.0),(3.0)")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT AVG(v) AS avg_v FROM t").await;
        let n = rows[0]["avg_v"].as_f64().expect("AVG should be f64");
        assert!((n - 2.0).abs() < f64::EPSILON);
    }

    #[tokio::test]
    async fn row_to_json_date_as_string() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (d DATE)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO t VALUES ('2026-03-20')")
            .execute(&pool)
            .await
            .unwrap();

        let rows = query_json(&pool, "SELECT d FROM t").await;
        assert_eq!(rows[0]["d"], Value::String("2026-03-20".into()));
    }

    #[tokio::test]
    async fn row_to_json_empty_result() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v INTEGER)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT v FROM t").await;
        assert!(rows.is_empty());
    }

    #[tokio::test]
    async fn row_to_json_multiple_columns_and_rows() {
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
        assert_eq!(rows.len(), 2);

        assert_eq!(rows[0]["id"], Value::Number(1.into()));
        assert_eq!(rows[0]["name"], Value::String("alice".into()));
        assert!(rows[0]["score"].is_number());

        assert_eq!(rows[1]["id"], Value::Number(2.into()));
        assert_eq!(rows[1]["name"], Value::String("bob".into()));
    }

    #[tokio::test]
    async fn row_to_json_null_literal_expression() {
        let pool = mem_pool().await;
        let rows = query_json(&pool, "SELECT NULL AS v").await;
        assert_eq!(rows[0]["v"], Value::Null);
    }

    #[tokio::test]
    async fn row_to_json_cast_expression() {
        let pool = mem_pool().await;
        let rows = query_json(&pool, "SELECT CAST(42 AS TEXT) AS v").await;
        assert_eq!(rows[0]["v"], Value::String("42".into()));
    }
}
