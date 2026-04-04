//! `PostgreSQL` database query operations.

use database_mcp_server::AppError;
use database_mcp_sql::identifier::validate_identifier;
use serde_json::{Value, json};
use sqlx::postgres::PgRow;
use sqlx_to_json::RowExt;

use super::PostgresAdapter;

impl PostgresAdapter {
    // `list_databases` uses the default pool intentionally — `pg_database`
    // is a server-wide catalog that returns all databases regardless of
    // which database the connection targets.
    /// Lists all accessible databases.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let pool = self.get_pool(None).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename")
                .fetch_all(&pool)
                .await
                .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Executes a SQL query and returns rows as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        let pool = self.get_pool(database).await?;
        let rows: Vec<PgRow> = sqlx::query(sql)
            .fetch_all(&pool)
            .await
            .map_err(|e| AppError::Query(e.to_string()))?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub async fn create_database(&self, name: &str) -> Result<Value, AppError> {
        if self.config.read_only {
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
}
