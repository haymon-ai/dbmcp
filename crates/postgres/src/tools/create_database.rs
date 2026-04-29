//! MCP tool: `createDatabase`.

use std::borrow::Cow;

use dbmcp_server::types::{CreateDatabaseRequest, MessageResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::connection::quote_ident;

/// Marker type for the `createDatabase` MCP tool.
pub(crate) struct CreateDatabaseTool;

impl CreateDatabaseTool {
    const NAME: &'static str = "createDatabase";
    const TITLE: &'static str = "Create Database";
    const DESCRIPTION: &'static str = r#"Create a new database on the connected server.

<usecase>
Use when:
- Setting up a new database for a project or application
- The user asks to create a database
</usecase>

<examples>
✓ "Create a database called analytics" → createDatabase(database="analytics")
✗ "Create a table" → use writeQuery with CREATE TABLE
</examples>

<important>
Database name must be non-empty; backend reserved-character rules apply.
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
        Ok(handler.create_database(params).await?)
    }
}

impl PostgresHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if read-only or the query fails.
    pub async fn create_database(
        &self,
        CreateDatabaseRequest { database }: CreateDatabaseRequest,
    ) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        let create_sql = format!("CREATE DATABASE {}", quote_ident(&database));
        self.connection.execute(create_sql.as_str(), None).await.map_err(|e| {
            let msg = e.to_string();
            if msg.contains("already exists") {
                return SqlError::Query(format!("Database '{database}' already exists."));
            }
            e
        })?;

        Ok(MessageResponse {
            message: format!("Database '{database}' created successfully."),
        })
    }
}
