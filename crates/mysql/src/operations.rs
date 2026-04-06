//! MySQL/MariaDB database query operations.
//!
//! Provides methods for listing databases, tables, executing queries,
//! creating databases, and dropping databases.

use database_mcp_server::AppError;
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use serde_json::{Value, json};
use sqlx::Executor;
use sqlx::mysql::MySqlRow;
use sqlx_to_json::RowExt;

use super::MysqlAdapter;

impl MysqlAdapter {
    /// Executes raw SQL and converts rows to JSON maps.
    ///
    /// Uses the text protocol via `Executor::fetch_all(&str)` instead of prepared
    /// statements, because `MySQL` 9+ doesn't support SHOW commands as prepared
    /// statements, and the text protocol returns all values as strings.
    pub(crate) async fn query_to_json(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        // Validate before entering the timeout scope so validation errors
        // are not confused with timeouts.
        if let Some(db) = database {
            validate_identifier(db)?;
        }

        let pool = self.pool.clone();
        let db = database.map(String::from);
        let sql_owned = sql.to_string();

        // The timeout wraps the entire acquire → USE → fetch sequence
        // because from the caller's perspective, this is one operation.
        execute_with_timeout(self.config.query_timeout, sql, async move {
            let mut conn = pool.acquire().await?;

            if let Some(db) = &db {
                let use_sql = format!("USE {}", Self::quote_identifier(db));
                conn.execute(use_sql.as_str()).await?;
            }

            let rows: Vec<MySqlRow> = conn.fetch_all(sql_owned.as_str()).await?;
            Ok::<_, sqlx::Error>(Value::Array(rows.iter().map(RowExt::to_json).collect()))
        })
        .await
    }

    /// Lists all accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub(crate) async fn list_databases(&self) -> Result<Vec<String>, AppError> {
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
    pub(crate) async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
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
    pub(crate) async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        self.query_to_json(sql, database).await
    }

    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub(crate) async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        // Check existence — use Vec<u8> because MySQL 9 returns BINARY columns
        let check_sql = "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?";
        let exists: Option<Vec<u8>> = execute_with_timeout(
            self.config.query_timeout,
            check_sql,
            sqlx::query_scalar(check_sql).bind(name).fetch_optional(&self.pool),
        )
        .await?;

        if exists.is_some() {
            return Ok(json!({
                "status": "exists",
                "message": format!("Database '{name}' already exists."),
                "database_name": name,
            }));
        }

        let create_sql = format!("CREATE DATABASE IF NOT EXISTS {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &create_sql,
            sqlx::query(&create_sql).execute(&self.pool),
        )
        .await?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' created successfully."),
            "database_name": name,
        }))
    }

    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] in read-only mode,
    /// [`AppError::InvalidIdentifier`] for invalid names,
    /// or [`AppError::Query`] if the target is the active database
    /// or the backend reports an error.
    pub(crate) async fn drop_database(&self, name: &str) -> Result<Value, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(name)?;

        // Guard: prevent dropping the currently connected database.
        if let Some(ref active) = self.config.name
            && active.eq_ignore_ascii_case(name)
        {
            return Err(AppError::Query(format!(
                "Cannot drop the currently connected database '{name}'."
            )));
        }

        let drop_sql = format!("DROP DATABASE {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&self.pool),
        )
        .await?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' dropped successfully."),
            "database_name": name,
        }))
    }
}
