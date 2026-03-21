//! `PostgreSQL` backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `PostgreSQL` databases. Supports
//! cross-database operations by maintaining a concurrent cache of connection
//! pools keyed by database name.

use crate::config::DatabaseConfig;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use moka::future::Cache;
use serde_json::{Map, Value, json};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, PgPool, Row, TypeInfo, ValueRef};
use std::collections::HashMap;
use tracing::info;

/// Maximum number of database connection pools to cache (including the default).
const POOL_CACHE_CAPACITY: u64 = 6;

/// `PostgreSQL` database backend.
///
/// All connection pools — including the default — live in a single
/// concurrent cache keyed by database name. No external mutex required.
#[derive(Clone)]
pub struct PostgresBackend {
    config: DatabaseConfig,
    default_db: String,
    pools: Cache<String, PgPool>,
    pub read_only: bool,
}

impl std::fmt::Debug for PostgresBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresBackend")
            .field("read_only", &self.read_only)
            .field("default_db", &self.default_db)
            .finish_non_exhaustive()
    }
}

impl PostgresBackend {
    /// Creates a new `PostgreSQL` backend from configuration.
    ///
    /// Stores a clone of the configuration for constructing connection URLs
    /// to non-default databases at runtime. The initial pool is placed into
    /// the shared cache keyed by the configured database name.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let url = Self::build_connection_url(config);
        let pool = PgPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect(&url)
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to PostgreSQL: {e}")))?;

        info!(
            "PostgreSQL connection pool initialized (max size: {})",
            config.max_pool_size
        );

        // PostgreSQL defaults to a database named after the connecting user.
        let default_db = config.name.clone().unwrap_or_else(|| config.user.clone());

        let pools = Cache::builder()
            .max_capacity(POOL_CACHE_CAPACITY)
            .eviction_listener(|_key, pool: PgPool, _cause| {
                tokio::spawn(async move {
                    pool.close().await;
                });
            })
            .build();

        pools.insert(default_db.clone(), pool).await;

        Ok(Self {
            config: config.clone(),
            default_db,
            pools,
            read_only: config.read_only,
        })
    }
}

impl PostgresBackend {
    /// Builds a sqlx connection URL from individual config fields.
    fn build_connection_url(config: &DatabaseConfig) -> String {
        let password = config.password.as_deref().unwrap_or_default();
        let name = config.name.as_deref().unwrap_or_default();
        let mut url = format!(
            "postgres://{}:{}@{}:{}/{}",
            config.user, password, config.host, config.port, name
        );

        let mut params = Vec::new();
        if config.ssl {
            params.push("sslmode=require".into());
            if let Some(ref ca) = config.ssl_ca {
                params.push(format!("sslrootcert={ca}"));
            }
            if let Some(ref cert) = config.ssl_cert {
                params.push(format!("sslcert={cert}"));
            }
            if let Some(ref key) = config.ssl_key {
                params.push(format!("sslkey={key}"));
            }
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        url
    }

    /// Wraps `name` in double quotes for safe use in `PostgreSQL` SQL statements.
    ///
    /// Escapes internal double quotes by doubling them.
    fn quote_identifier(name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }

    /// Returns a connection pool for the requested database.
    ///
    /// Resolves `None` or empty names to the default pool. On a cache miss
    /// a new pool is created and cached. Evicted pools are closed via the
    /// cache's eviction listener.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::InvalidIdentifier`] if the database name fails
    /// validation, or [`AppError::Connection`] if the new pool cannot connect.
    async fn get_pool(&self, database: Option<&str>) -> Result<PgPool, AppError> {
        let db_key = match database {
            Some(name) if !name.is_empty() => name,
            _ => &self.default_db,
        };

        if let Some(pool) = self.pools.get(db_key).await {
            return Ok(pool);
        }

        // Cache miss — validate then create a new pool.
        validate_identifier(db_key)?;

        let config = self.config.clone();
        let db_key_owned = db_key.to_owned();

        let pool = self
            .pools
            .try_get_with(db_key_owned, async {
                let mut cfg = config;
                cfg.name = Some(db_key.to_owned());
                let url = Self::build_connection_url(&cfg);

                PgPoolOptions::new()
                    .max_connections(cfg.max_pool_size)
                    .connect(&url)
                    .await
                    .map_err(|e| {
                        AppError::Connection(format!("Failed to connect to PostgreSQL database '{db_key}': {e}"))
                    })
            })
            .await
            .map_err(|e| match e.as_ref() {
                AppError::Connection(msg) => AppError::Connection(msg.clone()),
                other => AppError::Connection(other.to_string()),
            })?;

        Ok(pool)
    }
}

impl DatabaseBackend for PostgresBackend {
    // `list_databases` uses the default pool intentionally — `pg_database`
    // is a server-wide catalog that returns all databases regardless of
    // which database the connection targets.
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let pool = self.get_pool(None).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(table)?;
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let rows: Vec<PgRow> = sqlx::query(
            r"SELECT column_name, data_type, is_nullable, column_default,
                      character_maximum_length
               FROM information_schema.columns
               WHERE table_schema = 'public' AND table_name = $1
               ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        if rows.is_empty() {
            return Err(AppError::TableNotFound(table.to_string()));
        }

        let mut schema: HashMap<String, Value> = HashMap::new();
        for row in &rows {
            let col_name: String = row.try_get("column_name").unwrap_or_default();
            let data_type: String = row.try_get("data_type").unwrap_or_default();
            let nullable: String = row.try_get("is_nullable").unwrap_or_default();
            let default: Option<String> = row.try_get("column_default").ok();
            schema.insert(
                col_name,
                json!({
                    "type": data_type,
                    "nullable": nullable.to_uppercase() == "YES",
                    "key": Value::Null,
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

        // Get FK info using the same pool as the schema query
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let fk_rows: Vec<PgRow> = sqlx::query(
            r"SELECT
                kcu.column_name,
                tc.constraint_name,
                ccu.table_name AS referenced_table,
                ccu.column_name AS referenced_column,
                rc.update_rule AS on_update,
                rc.delete_rule AS on_delete
            FROM information_schema.table_constraints tc
            JOIN information_schema.key_column_usage kcu
                ON tc.constraint_name = kcu.constraint_name
                AND tc.table_schema = kcu.table_schema
            JOIN information_schema.constraint_column_usage ccu
                ON ccu.constraint_name = tc.constraint_name
                AND ccu.table_schema = tc.table_schema
            JOIN information_schema.referential_constraints rc
                ON rc.constraint_name = tc.constraint_name
                AND rc.constraint_schema = tc.table_schema
            WHERE tc.constraint_type = 'FOREIGN KEY'
                AND tc.table_name = $1
                AND tc.table_schema = 'public'",
        )
        .bind(table)
        .fetch_all(&pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let col_name: String = fk_row.try_get("column_name").unwrap_or_default();
            if let Some(col_info) = columns.get_mut(&col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": fk_row.try_get::<String, _>("constraint_name").ok(),
                        "referenced_table": fk_row.try_get::<String, _>("referenced_table").ok(),
                        "referenced_column": fk_row.try_get::<String, _>("referenced_column").ok(),
                        "on_update": fk_row.try_get::<String, _>("on_update").ok(),
                        "on_delete": fk_row.try_get::<String, _>("on_delete").ok(),
                    }),
                );
            }
        }

        Ok(json!({
            "table_name": table,
            "columns": columns,
        }))
    }

    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Vec<Map<String, Value>>, AppError> {
        let pool = self.get_pool(database).await?;
        let rows: Vec<PgRow> = sqlx::query(sql)
            .fetch_all(&pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        Ok(rows.iter().map(pg_row_to_json).collect())
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        let pool = self.get_pool(None).await?;

        // PostgreSQL CREATE DATABASE can't use parameterized queries
        sqlx::query(&format!("CREATE DATABASE {}", Self::quote_identifier(name)))
            .execute(&pool)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("already exists") {
                    return AppError::Query(format!("Database '{name}' already exists."));
                }
                AppError::Query(msg)
            })?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' created successfully."),
            "database_name": name,
        }))
    }

    fn dialect(&self) -> Box<dyn sqlparser::dialect::Dialect> {
        Box::new(sqlparser::dialect::PostgreSqlDialect {})
    }

    fn read_only(&self) -> bool {
        self.read_only
    }
}

/// Converts a `PostgreSQL` row to a JSON object with type-aware value extraction.
///
/// Type names are normalized to uppercase because sqlx may return either case
/// depending on query context. Integer types use size-specific Rust types
/// (`i16`, `i32`, `i64`) because sqlx enforces strict type matching for
/// `PostgreSQL`.
fn pg_row_to_json(row: &PgRow) -> Map<String, Value> {
    let columns = row.columns();
    let mut map = Map::with_capacity(columns.len());

    for column in columns {
        let idx = column.ordinal();
        let type_name = column.type_info().name().to_ascii_uppercase();

        let value = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
            Value::Null
        } else {
            match type_name.as_str() {
                "BOOL" => row.try_get::<bool, _>(idx).map(Value::Bool).unwrap_or(Value::Null),

                "INT8" => row
                    .try_get::<i64, _>(idx)
                    .map(|v| Value::Number(v.into()))
                    .unwrap_or(Value::Null),

                "INT4" | "OID" => row
                    .try_get::<i32, _>(idx)
                    .map(|v| Value::Number(i64::from(v).into()))
                    .unwrap_or(Value::Null),

                "INT2" => row
                    .try_get::<i16, _>(idx)
                    .map(|v| Value::Number(i64::from(v).into()))
                    .unwrap_or(Value::Null),

                "FLOAT4" | "FLOAT8" | "NUMERIC" | "MONEY" => row
                    .try_get::<f64, _>(idx)
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::Null, Value::Number),

                "BYTEA" => row
                    .try_get::<Vec<u8>, _>(idx)
                    .map_or(Value::Null, |bytes| Value::String(BASE64.encode(&bytes))),

                "JSON" | "JSONB" => row.try_get::<Value, _>(idx).unwrap_or(Value::Null),

                _ => row.try_get::<String, _>(idx).map(Value::String).unwrap_or(Value::Null),
            }
        };

        map.insert(column.name().to_string(), value);
    }

    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_identifier_wraps_in_double_quotes() {
        assert_eq!(PostgresBackend::quote_identifier("users"), "\"users\"");
        assert_eq!(PostgresBackend::quote_identifier("eu-docker"), "\"eu-docker\"");
    }

    #[test]
    fn quote_identifier_escapes_double_quotes() {
        assert_eq!(PostgresBackend::quote_identifier("test\"db"), "\"test\"\"db\"");
        assert_eq!(PostgresBackend::quote_identifier("a\"b\"c"), "\"a\"\"b\"\"c\"");
    }
}
