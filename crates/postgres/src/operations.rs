//! `PostgreSQL` database query operations.
//!
//! Provides methods for listing databases, tables, executing queries,
//! creating databases, dropping databases, and explaining queries.

use database_mcp_server::AppError;
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use database_mcp_sql::validation::validate_read_only_with_dialect;
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
    pub(crate) async fn list_databases(&self) -> Result<Vec<String>, AppError> {
        let pool = self.get_pool(None).await?;
        let sql = "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub(crate) async fn list_tables(&self, database: &str) -> Result<Vec<String>, AppError> {
        let db = if database.is_empty() { None } else { Some(database) };
        let pool = self.get_pool(db).await?;
        let sql = "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(rows.into_iter().map(|r| r.0).collect())
    }

    /// Executes a SQL query and returns rows as JSON.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub(crate) async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        let pool = self.get_pool(database).await?;
        let rows: Vec<PgRow> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query(sql).fetch_all(&pool)).await?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    /// Returns the execution plan for a query.
    ///
    /// When `analyze` is true and read-only mode is enabled, the inner
    /// query is validated to be read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if `analyze` is true,
    /// read-only mode is enabled, and the query is a write statement.
    /// Returns [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn explain_query(&self, database: &str, query: &str, analyze: bool) -> Result<Value, AppError> {
        if analyze && self.config.read_only {
            validate_read_only_with_dialect(query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let pool = self.get_pool(Some(database)).await?;

        let explain_sql = if analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {query}")
        } else {
            format!("EXPLAIN (FORMAT JSON) {query}")
        };

        let rows: Vec<PgRow> = execute_with_timeout(
            self.config.query_timeout,
            &explain_sql,
            sqlx::query(&explain_sql).fetch_all(&pool),
        )
        .await?;

        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
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

        let pool = self.get_pool(None).await?;

        // PostgreSQL CREATE DATABASE can't use parameterized queries
        let create_sql = format!("CREATE DATABASE {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &create_sql,
            sqlx::query(&create_sql).execute(&pool),
        )
        .await
        .map_err(|e| {
            let msg = e.to_string();
            if msg.contains("already exists") {
                return AppError::Query(format!("Database '{name}' already exists."));
            }
            e
        })?;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' created successfully."),
            "database_name": name,
        }))
    }

    /// Drops a table from a database.
    ///
    /// Validates identifiers, then executes `DROP TABLE`. When `cascade`
    /// is true the statement uses `CASCADE` to also remove dependent
    /// foreign-key constraints.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] in read-only mode,
    /// [`AppError::InvalidIdentifier`] for invalid names,
    /// or [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn drop_table(&self, database: &str, table: &str, cascade: bool) -> Result<Value, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        validate_identifier(database)?;
        validate_identifier(table)?;

        let pool = self.get_pool(Some(database)).await?;

        let mut drop_sql = format!("DROP TABLE {}", Self::quote_identifier(table));
        if cascade {
            drop_sql.push_str(" CASCADE");
        }

        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        Ok(json!({
            "status": "success",
            "message": format!("Table '{table}' dropped successfully."),
            "table_name": table,
        }))
    }

    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected (default) database and
    /// evicts the corresponding pool cache entry after a successful drop.
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
        if self.default_db == name {
            return Err(AppError::Query(format!(
                "Cannot drop the currently connected database '{name}'."
            )));
        }

        let pool = self.get_pool(None).await?;

        let drop_sql = format!("DROP DATABASE {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        // Evict the pool for the dropped database so stale connections
        // are not reused.
        self.pools.invalidate(name).await;

        Ok(json!({
            "status": "success",
            "message": format!("Database '{name}' dropped successfully."),
            "database_name": name,
        }))
    }
}
