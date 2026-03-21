//! MySQL/MariaDB backend implementation via sqlx.
//!
//! Implements [`DatabaseBackend`] for `MySQL` and `MariaDB` databases
//! using sqlx's `MySqlPool`.

use crate::config::DatabaseConfig;
use crate::db::backend::DatabaseBackend;
use crate::db::identifier::validate_identifier;
use crate::error::AppError;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::{Map, Value, json};
use sqlx::mysql::{MySqlPoolOptions, MySqlRow};
use sqlx::{Column, Executor, MySqlPool, Row, TypeInfo, ValueRef};
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
    pub async fn new(config: &DatabaseConfig) -> Result<Self, AppError> {
        let url = Self::build_connection_url(config);
        let pool = MySqlPoolOptions::new()
            .max_connections(config.max_pool_size)
            .connect(&url)
            .await
            .map_err(|e| AppError::Connection(format!("Failed to connect to MySQL: {e}")))?;

        info!("MySQL connection pool initialized (max size: {})", config.max_pool_size);

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

    /// Builds a sqlx connection URL from individual config fields.
    fn build_connection_url(config: &DatabaseConfig) -> String {
        let password = config.password.as_deref().unwrap_or_default();
        let name = config.name.as_deref().unwrap_or_default();
        let mut url = format!(
            "mysql://{}:{}@{}:{}/{}",
            config.user, password, config.host, config.port, name
        );

        let mut params = Vec::new();
        if let Some(ref charset) = config.charset {
            params.push(format!("charset={charset}"));
        }

        if config.ssl {
            params.push("ssl-mode=required".into());
            if let Some(ref ca) = config.ssl_ca {
                params.push(format!("ssl-ca={ca}"));
            }
            if let Some(ref cert) = config.ssl_cert {
                params.push(format!("ssl-cert={cert}"));
            }
            if let Some(ref key) = config.ssl_key {
                params.push(format!("ssl-key={key}"));
            }
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        url
    }

    /// Wraps `name` in backticks for safe use in `MySQL` SQL statements.
    ///
    /// Escapes internal backticks by doubling them.
    fn quote_identifier(name: &str) -> String {
        let escaped = name.replace('`', "``");
        format!("`{escaped}`")
    }

    /// Wraps a value in single quotes for use as a SQL string literal.
    ///
    /// Escapes internal single quotes by doubling them.
    fn quote_string(value: &str) -> String {
        let escaped = value.replace('\'', "''");
        format!("'{escaped}'")
    }

    /// Executes raw SQL and converts rows to JSON maps.
    ///
    /// Uses the text protocol via `Executor::fetch_all(&str)` instead of prepared
    /// statements, because `MySQL` 9+ doesn't support SHOW commands as prepared
    /// statements, and the text protocol returns all values as strings.
    async fn query_to_json(&self, sql: &str, database: Option<&str>) -> Result<Vec<Map<String, Value>>, AppError> {
        // Acquire a single connection so USE and the query run on the same session
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| AppError::Connection(e.to_string()))?;

        // Switch database if needed
        if let Some(db) = database {
            validate_identifier(db)?;
            let use_sql = format!("USE {}", Self::quote_identifier(db));
            conn.execute(use_sql.as_str())
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        }

        let rows: Vec<MySqlRow> = conn.fetch_all(sql).await.map_err(|e| AppError::Query(e.to_string()))?;

        Ok(rows.iter().map(mysql_row_to_json).collect())
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
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            Self::quote_string(database)
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
            Self::quote_identifier(database),
            Self::quote_identifier(table)
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

    async fn get_table_schema_with_relations(&self, database: &str, table: &str) -> Result<Value, AppError> {
        validate_identifier(database)?;
        validate_identifier(table)?;

        // 1. Get basic schema
        let describe_sql = format!(
            "DESCRIBE {}.{}",
            Self::quote_identifier(database),
            Self::quote_identifier(table)
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

    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Vec<Map<String, Value>>, AppError> {
        self.query_to_json(sql, database).await
    }

    async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        // Check existence — use Vec<u8> because MySQL 9 returns BINARY columns
        let exists: Option<Vec<u8>> =
            sqlx::query_scalar("SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?")
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
            Self::quote_identifier(name)
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

/// Converts a `MySQL` row to a JSON object with type-aware value extraction.
///
/// Uses `column.type_info().name()` to pick the right Rust type for each column.
/// `MySQL` 9 reports `information_schema` text columns as `VARBINARY`; these
/// are decoded as UTF-8 strings rather than base64.
fn mysql_row_to_json(row: &MySqlRow) -> Map<String, Value> {
    let columns = row.columns();
    let mut map = Map::with_capacity(columns.len());

    for column in columns {
        let idx = column.ordinal();
        let type_name = column.type_info().name();

        let value = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
            Value::Null
        } else {
            match type_name {
                "BOOLEAN" => row.try_get::<bool, _>(idx).map(Value::Bool).unwrap_or(Value::Null),

                "TINYINT" | "SMALLINT" | "INT" | "MEDIUMINT" | "BIGINT" | "TINYINT UNSIGNED" | "SMALLINT UNSIGNED"
                | "INT UNSIGNED" | "MEDIUMINT UNSIGNED" | "YEAR" => row
                    .try_get::<i64, _>(idx)
                    .map(|v| Value::Number(v.into()))
                    .unwrap_or(Value::Null),

                "BIGINT UNSIGNED" => row.try_get::<u64, _>(idx).map_or(Value::Null, |v| {
                    i64::try_from(v)
                        .map_or_else(|_| Value::String(v.to_string()), |signed| Value::Number(signed.into()))
                }),

                "FLOAT" | "DOUBLE" | "DECIMAL" => row
                    .try_get::<f64, _>(idx)
                    .ok()
                    .and_then(serde_json::Number::from_f64)
                    .map_or(Value::Null, Value::Number),

                "JSON" => row.try_get::<Value, _>(idx).unwrap_or(Value::Null),

                // MySQL 9 returns information_schema columns as BINARY/VARBINARY
                // even when they contain valid UTF-8. Try String first, then bytes.
                "BINARY" | "VARBINARY" => row
                    .try_get::<String, _>(idx)
                    .map_or_else(|_| mysql_bytes_to_json(row, idx), Value::String),

                "BLOB" | "TINYBLOB" | "MEDIUMBLOB" | "LONGBLOB" | "BIT" | "GEOMETRY" => mysql_bytes_to_json(row, idx),

                // All other types (VARCHAR, TEXT, DATE, TIME, ENUM, etc.) → String
                _ => row
                    .try_get::<String, _>(idx)
                    .map_or_else(|_| mysql_bytes_to_json(row, idx), Value::String),
            }
        };

        map.insert(column.name().to_string(), value);
    }

    map
}

/// Extracts a `MySQL` binary column as UTF-8 string, falling back to base64.
fn mysql_bytes_to_json(row: &MySqlRow, idx: usize) -> Value {
    row.try_get::<Vec<u8>, _>(idx).map_or(Value::Null, |bytes| {
        String::from_utf8(bytes.clone()).map_or_else(|_| Value::String(BASE64.encode(&bytes)), Value::String)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_identifier_wraps_in_backticks() {
        assert_eq!(MysqlBackend::quote_identifier("users"), "`users`");
        assert_eq!(MysqlBackend::quote_identifier("eu-docker"), "`eu-docker`");
    }

    #[test]
    fn quote_identifier_escapes_backticks() {
        assert_eq!(MysqlBackend::quote_identifier("test`db"), "`test``db`");
        assert_eq!(MysqlBackend::quote_identifier("a`b`c"), "`a``b``c`");
    }
}
