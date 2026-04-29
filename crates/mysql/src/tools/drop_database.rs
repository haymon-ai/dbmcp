//! MCP tool: `dropDatabase`.

use std::borrow::Cow;

use dbmcp_server::types::{DropDatabaseRequest, MessageResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::connection::quote_ident;

/// Marker type for the `dropDatabase` MCP tool.
pub(crate) struct DropDatabaseTool;

impl DropDatabaseTool {
    const NAME: &'static str = "dropDatabase";
    const TITLE: &'static str = "Drop Database";
    const DESCRIPTION: &'static str = r#"Drop an existing database from the connected server.

<usecase>
Use when:
- Removing a database that is no longer needed
- Cleaning up test or temporary databases
</usecase>

<examples>
✓ "Drop the test_db database" → dropDatabase(database="test_db")
✗ "Drop a table" → use dropTable instead
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
        Ok(handler.drop_database(params).await?)
    }
}

impl MysqlHandler {
    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected database.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the target is the active database
    /// or the backend reports an error.
    pub async fn drop_database(
        &self,
        DropDatabaseRequest { database }: DropDatabaseRequest,
    ) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        // Guard: prevent dropping the currently connected database.
        if self.connection.default_database_name().eq_ignore_ascii_case(&database) {
            return Err(SqlError::Query(format!(
                "Cannot drop the currently connected database '{database}'."
            )));
        }

        let drop_sql = format!("DROP DATABASE {}", quote_ident(&database));
        self.connection.execute(drop_sql.as_str(), None).await?;

        // Evict the pool for the dropped database so stale connections
        // are not reused.
        self.connection.invalidate(&database).await;

        Ok(MessageResponse {
            message: format!("Database '{database}' dropped successfully."),
        })
    }
}
