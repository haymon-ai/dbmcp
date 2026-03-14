//! MySQL/MariaDB backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `MySQL` and `MariaDB` databases
//! using sqlx's `MySqlPool`.

use crate::config::Config;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::{backtick_escape, validate_identifier};
use crate::error::AppError;
use serde_json::{Map, Value, json};
use sqlx::mysql::{MySqlPoolOptions, MySqlRow};
use sqlx::{Column, Executor, MySqlPool, Row};
use std::collections::HashMap;
use tracing::{error, info};

/// MySQL/MariaDB database backend.
#[derive(Clone)]
pub struct MysqlBackend {
    pool: MySqlPool,
    pub read_only: bool,
}

impl std::fmt::Debug for MysqlBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MysqlBackend")
            .field("read_only", &self.read_only)
            .finish_non_exhaustive()
    }
}

impl MysqlBackend {
    /// Creates a new `MySQL` backend from configuration.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Connection`] if the connection fails.
    pub async fn new(config: &Config) -> Result<Self, AppError> {
        let url = format!(
            "mysql://{}:{}@{}:{}/{}",
            config.db_user,
            config.db_password,
            config.db_host,
            config.db_port,
            config.db_name.as_deref().unwrap_or("")
        );

        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect(&url)
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to MySQL: {e}")))?;

        info!(
            "MySQL connection pool initialized: {}@{}:{}/{} (max size: {})",
            config.db_user,
            config.db_host,
            config.db_port,
            config.db_name.as_deref().unwrap_or(""),
            config.max_pool_size
        );

        let backend = Self {
            pool,
            read_only: config.read_only,
        };

        if config.read_only {
            backend.warn_if_file_privilege().await;
        }

        Ok(backend)
    }

    async fn warn_if_file_privilege(&self) {
        let result: Result<(), AppError> = async {
            let current_user: Option<String> = sqlx::query_scalar("SELECT CURRENT_USER()")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

            let Some(current_user) = current_user else {
                return Ok(());
            };

            let quoted_user = if let Some((user, host)) = current_user.split_once('@') {
                format!("'{user}'@'{host}'")
            } else {
                format!("'{current_user}'")
            };

            let grants: Vec<String> = sqlx::query_scalar(&format!("SHOW GRANTS FOR {quoted_user}"))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;

            let has_file_priv = grants.iter().any(|grant| {
                let upper = grant.to_uppercase();
                upper.contains("FILE") && upper.contains("ON *.*")
            });

            if has_file_priv {
                error!(
                    "Connected database user has the global FILE privilege. \
                     Revoke FILE for the database user you are connecting as."
                );
            }

            Ok(())
        }
        .await;

        if let Err(e) = result {
            tracing::debug!("Unable to determine whether FILE privilege is enabled: {e}");
        }
    }

    /// Executes raw SQL and converts rows to JSON maps.
    ///
    /// Uses the text protocol via `Executor::fetch_all(&str)` instead of prepared
    /// statements, because `MySQL` 9+ doesn't support SHOW commands as prepared
    /// statements, and the text protocol returns all values as strings.
    async fn query_to_json(
        &self,
        sql: &str,
        database: Option<&str>,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        // Acquire a single connection so USE and the query run on the same session
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Connection(e.to_string()))?;

        // Switch database if needed
        if let Some(db) = database {
            validate_identifier(db)?;
            let use_sql = format!("USE {}", backtick_escape(db));
            conn.execute(use_sql.as_str())
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        }

        let rows: Vec<MySqlRow> = conn
            .fetch_all(sql)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        let mut results = Vec::new();
        for row in &rows {
            let mut map = Map::new();
            for col in row.columns() {
                let name = col.name().to_string();
                let idx = col.ordinal();
                // MySQL may return columns with BINARY flag where try_get::<String>
                // fails. Fall through: String → i64 → f64 → bool → Vec<u8> → Null.
                let value = if let Ok(s) = row.try_get::<String, _>(idx) {
                    Value::String(s)
                } else if let Ok(n) = row.try_get::<i64, _>(idx) {
                    Value::Number(n.into())
                } else if let Ok(f) = row.try_get::<f64, _>(idx) {
                    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
                } else if let Ok(b) = row.try_get::<bool, _>(idx) {
                    Value::Bool(b)
                } else if let Ok(bytes) = row.try_get::<Vec<u8>, _>(idx) {
                    Value::String(String::from_utf8_lossy(&bytes).into_owned())
                } else {
                    Value::Null
                };
                map.insert(name, value);
            }
            results.push(map);
        }

        Ok(results)
    }
}

impl DatabaseBackend for MysqlBackend {
    async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let results = self
            .query_to_json(
                "SELECT SCHEMA_NAME AS name FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
                None,
            )
            .await?;
        Ok(results
            .into_iter()
            .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
            .collect())
    }

    async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        validate_identifier(database)?;
        let sql = format!(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = '{database}' ORDER BY TABLE_NAME"
        );
        let results = self.query_to_json(&sql, None).await?;
        Ok(results
            .into_iter()
            .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
            .collect())
    }

    async fn get_table_schema(&self, database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(database)?;
        validate_identifier(table)?;

        let sql = format!(
            "DESCRIBE {}.{}",
            backtick_escape(database),
            backtick_escape(table)
        );
        let results = self.query_to_json(&sql, None).await?;

        if results.is_empty() {
            return Err(AppError::TableNotFound(format!("{database}.{table}")));
        }

        let mut schema: HashMap<String, Value> = HashMap::new();
        for row in &results {
            if let Some(col_name) = row.get("Field").and_then(|v| v.as_str()) {
                schema.insert(
                    col_name.to_string(),
                    json!({
                        "type": row.get("Type").unwrap_or(&Value::Null),
                        "nullable": row.get("Null").and_then(|v| v.as_str()).is_some_and(|s| s.to_uppercase() == "YES"),
                        "key": row.get("Key").unwrap_or(&Value::Null),
                        "default": row.get("Default").unwrap_or(&Value::Null),
                        "extra": row.get("Extra").unwrap_or(&Value::Null),
                    }),
                );
            }
        }

        Ok(json!(schema))
    }

    async fn get_table_schema_with_relations(
        &self,
        database: &str,
        table: &str,
    ) -> Result<Value, AppError> {
        validate_identifier(database)?;
        validate_identifier(table)?;

        // 1. Get basic schema
        let describe_sql = format!(
            "DESCRIBE {}.{}",
            backtick_escape(database),
            backtick_escape(table)
        );
        let schema_results = self.query_to_json(&describe_sql, None).await?;

        if schema_results.is_empty() {
            return Err(AppError::TableNotFound(format!("{database}.{table}")));
        }

        let mut columns: HashMap<String, Value> = HashMap::new();
        for row in &schema_results {
            if let Some(col_name) = row.get("Field").and_then(|v| v.as_str()) {
                columns.insert(
                    col_name.to_string(),
                    json!({
                        "type": row.get("Type").unwrap_or(&Value::Null),
                        "nullable": row.get("Null").and_then(|v| v.as_str()).is_some_and(|s| s.to_uppercase() == "YES"),
                        "key": row.get("Key").unwrap_or(&Value::Null),
                        "default": row.get("Default").unwrap_or(&Value::Null),
                        "extra": row.get("Extra").unwrap_or(&Value::Null),
                        "foreign_key": null,
                    }),
                );
            }
        }

        // 2. Get FK relationships
        let fk_sql = r"
            SELECT
                kcu.COLUMN_NAME as column_name,
                kcu.CONSTRAINT_NAME as constraint_name,
                kcu.REFERENCED_TABLE_NAME as referenced_table,
                kcu.REFERENCED_COLUMN_NAME as referenced_column,
                rc.UPDATE_RULE as on_update,
                rc.DELETE_RULE as on_delete
            FROM information_schema.KEY_COLUMN_USAGE kcu
            INNER JOIN information_schema.REFERENTIAL_CONSTRAINTS rc
                ON kcu.CONSTRAINT_NAME = rc.CONSTRAINT_NAME
                AND kcu.CONSTRAINT_SCHEMA = rc.CONSTRAINT_SCHEMA
            WHERE kcu.TABLE_SCHEMA = ?
              AND kcu.TABLE_NAME = ?
              AND kcu.REFERENCED_TABLE_NAME IS NOT NULL
            ORDER BY kcu.CONSTRAINT_NAME, kcu.ORDINAL_POSITION
        ";

        let fk_rows: Vec<MySqlRow> = sqlx::query(fk_sql)
            .bind(database)
            .bind(table)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;

        for fk_row in &fk_rows {
            let col_name: Option<String> = fk_row.try_get("column_name").ok();
            if let Some(col_name) = col_name
                && let Some(col_info) = columns.get_mut(&col_name)
                && let Some(obj) = col_info.as_object_mut()
            {
                let constraint_name: Option<String> = fk_row.try_get("constraint_name").ok();
                let referenced_table: Option<String> = fk_row.try_get("referenced_table").ok();
                let referenced_column: Option<String> = fk_row.try_get("referenced_column").ok();
                let on_update: Option<String> = fk_row.try_get("on_update").ok();
                let on_delete: Option<String> = fk_row.try_get("on_delete").ok();
                obj.insert(
                    "foreign_key".to_string(),
                    json!({
                        "constraint_name": constraint_name,
                        "referenced_table": referenced_table,
                        "referenced_column": referenced_column,
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

    async fn execute_query(
        &self,
        sql: &str,
        database: Option<&str>,
    ) -> Result<Vec<Map<String, Value>>, AppError> {
        self.query_to_json(sql, database).await
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        // Check existence — use Vec<u8> because MySQL 9 returns BINARY columns
        let exists: Option<Vec<u8>> = sqlx::query_scalar(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        if exists.is_some() {
            return Ok(json!({
                "status": "exists",
                "message": format!("Database '{name}' already exists."),
                "database_name": name,
            }));
        }

        sqlx::query(&format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            backtick_escape(name)
        ))
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Query(e.to_string()))?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' created successfully."),
            "database_name": name,
        }))
    }

    fn dialect(&self) -> Box<dyn sqlparser::dialect::Dialect> {
        Box::new(sqlparser::dialect::MySqlDialect {})
    }

    fn read_only(&self) -> bool {
        self.read_only
    }
}
