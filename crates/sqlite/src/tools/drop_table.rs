//! MCP tool: `dropTable`.

use dbmcp_server::types::MessageResponse;
use dbmcp_sql::SqlError;

use super::prelude::*;
use crate::connection::quote_ident;
use crate::types::DropTableRequest;

const NAME: &str = "dropTable";
const TITLE: &str = "Drop Table";
const DESCRIPTION: &str = include_str!("../../assets/tools/drop_table.md");

/// Marker type for the `dropTable` MCP tool.
pub(crate) struct DropTableTool;

impl ToolBase for DropTableTool {
    type Parameter = DropTableRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION.into())
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
        handler.drop_table(params).await
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
    pub async fn drop_table(&self, DropTableRequest { table }: DropTableRequest) -> Result<MessageResponse, ErrorData> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation.into());
        }

        let drop_sql = format!("DROP TABLE {}", quote_ident(&table));
        self.connection.execute(drop_sql.as_str(), None).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
