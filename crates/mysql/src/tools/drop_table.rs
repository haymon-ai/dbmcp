//! MCP tool: `dropTable`.

use std::borrow::Cow;

use dbmcp_server::types::MessageResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::connection::quote_ident;
use crate::types::DropTableRequest;

/// Marker type for the `dropTable` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "dropTable";
    const TITLE: &'static str = "Drop Table";
    const DESCRIPTION: &'static str = r#"Drop a table from a database.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table from mydb" → dropTable(database="mydb", table="temp_logs")
✗ "Delete rows from a table" → use writeQuery with DELETE
✗ "Drop a database" → use dropDatabase instead
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
If the table has foreign key dependencies, the drop will fail — resolve dependencies first.
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

impl AsyncTool<MysqlHandler> for DropTableTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_table(params).await?)
    }
}

impl MysqlHandler {
    /// Drops a table from a database.
    ///
    /// Switches to the target database with `USE`, then executes
    /// `DROP TABLE`.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the backend reports an error.
    pub async fn drop_table(
        &self,
        DropTableRequest { database, table }: DropTableRequest,
    ) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let drop_sql = format!("DROP TABLE {}", quote_ident(&table));
        self.connection.execute(drop_sql.as_str(), database).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
