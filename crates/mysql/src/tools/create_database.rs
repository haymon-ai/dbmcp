//! MCP tool: `create_database`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{CreateDatabaseRequest, MessageResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::identifier::validate_identifier;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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
If the database already exists, returns a message indicating so without error.
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

impl AsyncTool<MysqlHandler> for CreateDatabaseTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.create_database(&params).await?)
    }
}

impl MysqlHandler {
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

        let check_sql = format!(
            "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = {}",
            self.connection.quote_string(name),
        );
        let exists = self.connection.fetch_optional(check_sql.as_str(), None).await?;

        if exists.is_some() {
            return Ok(MessageResponse {
                message: format!("Database '{name}' already exists."),
            });
        }

        let create_sql = format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            self.connection.quote_identifier(name)
        );
        self.connection.execute(create_sql.as_str(), None).await?;

        Ok(MessageResponse {
            message: format!("Database '{name}' created successfully."),
        })
    }
}
