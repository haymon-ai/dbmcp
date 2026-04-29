//! MCP tool: `dropTable`.

use std::borrow::Cow;

use dbmcp_server::types::MessageResponse;
use dbmcp_sql::SqlError;

use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::connection::quote_ident;
use crate::types::DropTableRequest;

/// Marker type for the `dropTable` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "dropTable";
    const TITLE: &'static str = "Drop Table";
    const DESCRIPTION: &'static str = r#"Drop a table from the database.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table" → dropTable(table="temp_logs")
✗ "Delete rows from a table" → use writeQuery with DELETE
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
</safety>

<what_it_returns>
A confirmation message with the dropped table name.
</what_it_returns>"#;
}

impl ToolBase for DropTableTool {
    type Parameter = DropTableRequest;
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

impl AsyncTool<SqliteHandler> for DropTableTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_table(params).await?)
    }
}

impl SqliteHandler {
    /// Drops a table from the database.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the backend reports an error.
    pub async fn drop_table(&self, DropTableRequest { table }: DropTableRequest) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        let drop_sql = format!("DROP TABLE {}", quote_ident(&table));
        self.connection.execute(drop_sql.as_str(), None).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
