//! MCP tool: `create_database`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{CreateDatabaseRequest, MessageResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;

/// Marker type for the `create_database` MCP tool.
pub(crate) struct CreateDatabaseTool;

impl CreateDatabaseTool {
    const NAME: &'static str = "create_database";
    const TITLE: &'static str = "Create Database";
    const DESCRIPTION: &'static str = r#"Create a new database on the connected server.

<usecase>
Use when:
- Setting up a new database for a project or application
- The user asks to create a database
</usecase>

<examples>
✓ "Create a database called analytics" → create_database(database_name="analytics")
✗ "Create a table" → use write_query with CREATE TABLE
</examples>

<important>
Database names must contain only alphanumeric characters and underscores.
</important>

<what_it_returns>
A confirmation message with the created database name.
</what_it_returns>"#;
}

impl ToolBase for CreateDatabaseTool {
    type Parameter = CreateDatabaseRequest;
    type Output = MessageResponse;
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
                .read_only(false)
                .destructive(false)
                .idempotent(false)
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for CreateDatabaseTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.create_database(&params).await?)
    }
}

impl PostgresHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub async fn create_database(&self, request: &CreateDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let name = &request.database_name;
        validate_identifier(name)?;

        // PostgreSQL CREATE DATABASE can't use parameterized queries
        let create_sql = format!("CREATE DATABASE {}", self.connection.quote_identifier(name));
        self.connection.execute(&create_sql, None).await.map_err(|e| {
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
}
