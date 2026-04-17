//! MCP tool: `drop_database`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{DropDatabaseRequest, MessageResponse};
use database_mcp_sql::Connection as _;
use database_mcp_sql::sanitize::{quote_ident, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use sqlparser::dialect::MySqlDialect;

use crate::MysqlHandler;

/// Marker type for the `drop_database` MCP tool.
pub(crate) struct DropDatabaseTool;

impl DropDatabaseTool {
    const NAME: &'static str = "drop_database";
    const TITLE: &'static str = "Drop Database";
    const DESCRIPTION: &'static str = r#"Drop an existing database from the connected server.

<usecase>
Use when:
- Removing a database that is no longer needed
- Cleaning up test or temporary databases
</usecase>

<examples>
✓ "Drop the test_db database" → drop_database(database_name="test_db")
✗ "Drop a table" → use drop_table instead
</examples>

<safety>
IMPORTANT: This permanently deletes the database and ALL its data. This action cannot be undone.
Cannot drop the database you are currently connected to.
</safety>

<what_it_returns>
A confirmation message with the dropped database name.
</what_it_returns>"#;
}

impl ToolBase for DropDatabaseTool {
    type Parameter = DropDatabaseRequest;
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
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        )
    }
}

impl AsyncTool<MysqlHandler> for DropDatabaseTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_database(&params).await?)
    }
}

impl MysqlHandler {
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
    pub async fn drop_database(&self, request: &DropDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }

        let DropDatabaseRequest { database_name } = request;

        validate_ident(database_name)?;

        // Guard: prevent dropping the currently connected database.
        if self
            .connection
            .default_database_name()
            .eq_ignore_ascii_case(database_name)
        {
            return Err(AppError::Query(format!(
                "Cannot drop the currently connected database '{database_name}'."
            )));
        }

        let drop_sql = format!("DROP DATABASE {}", quote_ident(database_name, &MySqlDialect {}));
        self.connection.execute(drop_sql.as_str(), None).await?;

        // Evict the pool for the dropped database so stale connections
        // are not reused.
        self.connection.invalidate(database_name).await;

        Ok(MessageResponse {
            message: format!("Database '{database_name}' dropped successfully."),
        })
    }
}
