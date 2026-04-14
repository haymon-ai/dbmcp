//! MCP tool: `read_query`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::QueryResponse;
use database_mcp_sql::Connection as _;
use database_mcp_sql::validation::validate_read_only_with_dialect;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use serde_json::Value;

use crate::SqliteHandler;
use crate::types::QueryRequest;

/// Marker type for the `read_query` MCP tool.
pub(crate) struct ReadQueryTool;

impl ReadQueryTool {
    const NAME: &'static str = "read_query";
    const TITLE: &'static str = "Read Query";
    const DESCRIPTION: &'static str = r#"Execute a read-only SQL query. Allowed statements: SELECT.

<usecase>
Use when:
- Querying data from tables (SELECT with WHERE, JOIN, GROUP BY, etc.)
- Aggregations: COUNT, SUM, AVG, GROUP BY, HAVING
- Checking data existence or counts
</usecase>

<when_not_to_use>
- Data changes (INSERT, UPDATE, DELETE) → use write_query
- Query performance analysis → use explain_query
- Discovering tables or columns → use list_tables or get_table_schema
</when_not_to_use>

<examples>
✓ "SELECT * FROM users WHERE status = 'active'"
✓ "SELECT COUNT(*) FROM orders GROUP BY region"
✗ "INSERT INTO users ..." → use write_query
✗ "EXPLAIN SELECT ..." → use explain_query for structured analysis
</examples>

<what_it_returns>
A JSON array of row objects, each keyed by column name.
</what_it_returns>"#;
}

impl ToolBase for ReadQueryTool {
    type Parameter = QueryRequest;
    type Output = QueryResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }
}

impl AsyncTool<SqliteHandler> for ReadQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.read_query(&params).await?)
    }
}

impl SqliteHandler {
    /// Executes a read-only SQL query.
    ///
    /// Validates that the query is read-only before executing.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] if the query is not
    /// read-only, or [`AppError::Query`] if the backend reports an error.
    pub async fn read_query(&self, request: &QueryRequest) -> Result<QueryResponse, AppError> {
        validate_read_only_with_dialect(&request.query, &sqlparser::dialect::SQLiteDialect {})?;
        let rows = self.connection.fetch(request.query.as_str(), None).await?;
        Ok(QueryResponse {
            rows: Value::Array(rows),
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use sqlx::SqlitePool;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::sqlite::SqliteRow;
    use sqlx_to_json::RowExt;

    async fn mem_pool() -> SqlitePool {
        SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory SQLite")
    }

    async fn query_json(pool: &SqlitePool, sql: &str) -> Value {
        let rows: Vec<SqliteRow> = sqlx::query(sql).fetch_all(pool).await.expect("query failed");
        Value::Array(rows.iter().map(RowExt::to_json).collect())
    }

    #[tokio::test]
    async fn rows_to_json_array_empty_result() {
        let pool = mem_pool().await;
        sqlx::query("CREATE TABLE t (v INTEGER)").execute(&pool).await.unwrap();

        let rows = query_json(&pool, "SELECT v FROM t").await;
        assert_eq!(rows, Value::Array(vec![]));
    }

    #[tokio::test]
    async fn rows_to_json_array_multiple_rows() {
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
        assert_eq!(rows.as_array().expect("should be array").len(), 2);

        assert_eq!(rows[0]["id"], Value::Number(1.into()));
        assert_eq!(rows[0]["name"], Value::String("alice".into()));
        assert!(rows[0]["score"].is_number());

        assert_eq!(rows[1]["id"], Value::Number(2.into()));
        assert_eq!(rows[1]["name"], Value::String("bob".into()));
    }
}
