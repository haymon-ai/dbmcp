//! `PostgreSQL` database query operations.
//!
//! Provides methods for listing databases, tables, executing queries,
//! creating databases, dropping databases, and explaining queries.

use super::types::DropTableRequest;
use database_mcp_server::AppError;
use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, ListDatabasesResponse, ListTablesRequest,
    ListTablesResponse, MessageResponse, QueryRequest, QueryResponse,
};
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use database_mcp_sql::validation::validate_read_only_with_dialect;
use serde_json::Value;
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
    pub(crate) async fn list_databases(&self) -> Result<ListDatabasesResponse, AppError> {
        let pool = self.get_pool(None).await?;
        let sql = "SELECT datname FROM pg_database WHERE datistemplate = false ORDER BY datname";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(ListDatabasesResponse {
            databases: rows.into_iter().map(|r| r.0).collect(),
        })
    }

    /// Lists all tables in a database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the identifier is invalid or the query fails.
    pub(crate) async fn list_tables(&self, request: &ListTablesRequest) -> Result<ListTablesResponse, AppError> {
        let db = if request.database_name.is_empty() {
            None
        } else {
            Some(request.database_name.as_str())
        };
        let pool = self.get_pool(db).await?;
        let sql = "SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename";
        let rows: Vec<(String,)> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query_as(sql).fetch_all(&pool)).await?;
        Ok(ListTablesResponse {
            tables: rows.into_iter().map(|r| r.0).collect(),
        })
    }

    /// Executes a SQL query and returns rows as JSON.
    async fn execute_query(&self, sql: &str, database: Option<&str>) -> Result<Value, AppError> {
        let pool = self.get_pool(database).await?;
        let rows: Vec<PgRow> =
            execute_with_timeout(self.config.query_timeout, sql, sqlx::query(sql).fetch_all(&pool)).await?;
        Ok(Value::Array(rows.iter().map(RowExt::to_json).collect()))
    }

    /// Executes a read-only SQL query.
    ///
    /// Validates that the query is read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`AppError::Query`] if the backend reports an error.
    pub(crate) async fn read_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::PostgreSqlDialect {})?;
        let db = Some(request.database_name.trim()).filter(|s| !s.is_empty());
        let rows = self.execute_query(&request.query, db).await?;
        Ok(QueryResponse { rows })
    }

    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if the query fails.
    pub(crate) async fn write_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        let db = Some(request.database_name.trim()).filter(|s| !s.is_empty());
        let rows = self.execute_query(&request.query, db).await?;
        Ok(QueryResponse { rows })
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
    pub(crate) async fn explain_query(&self, request: &ExplainQueryRequest) -> Result<QueryResponse, AppError> {
        if request.analyze && self.config.read_only {
            validate_read_only_with_dialect(&request.query, &sqlparser::dialect::PostgreSqlDialect {})?;
        }

        let pool = self.get_pool(Some(&request.database_name)).await?;

        let explain_sql = if request.analyze {
            format!("EXPLAIN (ANALYZE, FORMAT JSON) {}", request.query)
        } else {
            format!("EXPLAIN (FORMAT JSON) {}", request.query)
        };

        let rows: Vec<PgRow> = execute_with_timeout(
            self.config.query_timeout,
            &explain_sql,
            sqlx::query(&explain_sql).fetch_all(&pool),
        )
        .await?;

        Ok(QueryResponse {
            rows: Value::Array(rows.iter().map(RowExt::to_json).collect()),
        })
    }

    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub(crate) async fn create_database(&self, request: &CreateDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let name = &request.database_name;
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

        Ok(MessageResponse {
            message: format!("Database '{name}' created successfully."),
        })
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
    pub(crate) async fn drop_table(&self, request: &DropTableRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let database = &request.database_name;
        let table = &request.table_name;
        validate_identifier(database)?;
        validate_identifier(table)?;

        let pool = self.get_pool(Some(database)).await?;

        let mut drop_sql = format!("DROP TABLE {}", Self::quote_identifier(table));
        if request.cascade {
            drop_sql.push_str(" CASCADE");
        }

        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
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
    pub(crate) async fn drop_database(&self, request: &DropDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let name = &request.database_name;
        validate_identifier(name)?;

        // Guard: prevent dropping the currently connected database.
        if self.default_db == *name {
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

        Ok(MessageResponse {
            message: format!("Database '{name}' dropped successfully."),
        })
    }
}
