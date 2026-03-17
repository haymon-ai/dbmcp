//! `PostgreSQL` backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `PostgreSQL` databases.

use crate::config::Config;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use serde_json::{Map, Value, json};
use sqlx::postgres::{PgPoolOptions, PgRow};
use sqlx::{Column, PgPool, Row};
use std::collections::HashMap;
use tracing::info;

/// `PostgreSQL` database backend.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: PgPool,
    pub read_only: bool,
}

impl std::fmt::Debug for PostgresBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PostgresBackend")
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl PostgresBackend {
    /// Creates a new `PostgreSQL` backend from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &Config) -> Result<Self, AppError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect(&config.database_url)
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to PostgreSQL: {e}")))?;

        info!(
            "PostgreSQL connection pool initialized (max size: {})",
            config.max_pool_size
        );

        Ok(Self {
            pool,
            read_only: config.read_only,
        })
    }
}

impl PostgresBackend {
    /// Wraps `name` in double quotes for safe use in `PostgreSQL` SQL statements.
    ///
    /// Escapes internal double quotes by doubling them.
    fn quote_identifier(name: &str) -> String {
        let escaped = name.replace('"', "\"\"");
        format!("\"{escaped}\"")
    }
}

impl DatabaseBackend for PostgresBackend {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn list_tables(&self, _database: &str) -> Result<Vec<String>, AppError> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    async fn get_table_schema(&self, _database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(table)?;
        let rows: Vec<PgRow> = sqlx::query(
            r"SELECT column_name, data_type, is_nullable, column_default,
                      character_maximum_length
               FROM information_schema.columns
               WHERE table_schema = 'public' AND table_name = $1
               ORDER BY ordinal_position",
        )
        .bind(table)
        .fetch_all(&self.pool)
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

    async fn get_table_schema_with_relations(
        &self,
        database: &str,
        table: &str,
    ) -> Result<Value, AppError> {
        let schema = self.get_table_schema(database, table).await?;
        let mut columns: HashMap<String, Value> =
            serde_json::from_value(schema).unwrap_or_default();

        // Add null foreign_key to all columns
        for col in columns.values_mut() {
            if let Some(obj) = col.as_object_mut() {
                obj.entry("foreign_key".to_string()).or_insert(Value::Null);
            }
        }

        // Get FK info
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
        .fetch_all(&self.pool)
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

    async fn execute_query(
        &self,
        sql: &str,
        _database: Option<&str>,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        let rows: Vec<PgRow> = sqlx::query(sql)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let mut results = Vec::new();
        for row in &rows {
            let mut map = Map::new();
            for col in row.columns() {
                let name = col.name().to_string();
                let val: Option<String> = row.try_get(col.ordinal()).ok();
                map.insert(name, val.map_or(Value::Null, Value::String));
            }
            results.push(map);
        }
        Ok(results)
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        // PostgreSQL CREATE DATABASE can't use parameterized queries
        sqlx::query(&format!("CREATE DATABASE {}", Self::quote_identifier(name)))
            .execute(&self.pool)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_identifier_wraps_in_double_quotes() {
        assert_eq!(PostgresBackend::quote_identifier("users"), "\"users\"");
        assert_eq!(
            PostgresBackend::quote_identifier("eu-docker"),
            "\"eu-docker\""
        );
    }

    #[test]
    fn quote_identifier_escapes_double_quotes() {
        assert_eq!(
            PostgresBackend::quote_identifier("test\"db"),
            "\"test\"\"db\""
        );
        assert_eq!(
            PostgresBackend::quote_identifier("a\"b\"c"),
            "\"a\"\"b\"\"c\""
        );
    }
}
