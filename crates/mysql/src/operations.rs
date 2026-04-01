//! MySQL/MariaDB database query operations.
//!
//! Provides methods for listing databases, tables, executing queries,
//! and creating databases.

use database_mcp_backend::error::AppError;
use database_mcp_backend::identifier::validate_identifier;
use serde_json::{Value, json};
use sqlx::Executor;
use sqlx::mysql::MySqlRow;
use sqlx_to_json::RowExt;

use super::MysqlBackend;

impl MysqlBackend {
    /// Executes raw SQL and converts rows to JSON maps.
    ///
    /// Uses the text protocol via `Executor::fetch_all(&str)` instead of prepared
    /// statements, because `MySQL` 9+ doesn't support SHOW commands as prepared
    /// statements, and the text protocol returns all values as strings.
    pub(super) async fn query_to_json(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
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
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    /// Lists all accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let results = self
            .query_to_json(
                "SELECT SCHEMA_NAME AS name FROM information_schema.SCHEMATA ORDER BY SCHEMA_NAME",
                None,
            )
            .await?;
        let rows = results.as_array().map_or([].as_slice(), Vec::as_slice);
        Ok(rows
            .iter()
            .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
            .collect())
    }

    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        validate_identifier(database)?;
        let sql = format!(
            "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = {} ORDER BY TABLE_NAME",
            Self::quote_string(database)
        );
        let results = self.query_to_json(&sql, None).await?;
        let rows = results.as_array().map_or([].as_slice(), Vec::as_slice);
        Ok(rows
            .iter()
            .filter_map(|row| row.get("name").and_then(|v| v.as_str().map(String::from)))
            .collect())
    }

    /// Executes a SQL query and returns rows as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        self.query_to_json(sql, database).await
    }

    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub async fn create_database(&self, name: &str) -> Result<Value, AppError> {
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
}
