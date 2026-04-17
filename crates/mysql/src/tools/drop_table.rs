//! MCP tool: `drop_table`.

use std::borrow::Cow;

use database_mcp_server::types::MessageResponse;
use database_mcp_sql::Connection as _;
use database_mcp_sql::SqlError;
use database_mcp_sql::sanitize::{quote_ident, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use sqlparser::dialect::MySqlDialect;

use crate::MysqlHandler;
use crate::types::DropTableRequest;

/// Marker type for the `drop_table` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "drop_table";
    const TITLE: &'static str = "Drop Table";
    const DESCRIPTION: &'static str = r#"Drop a table from a database.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table from mydb" → drop_table(database_name="mydb", table_name="temp_logs")
✗ "Delete rows from a table" → use write_query with DELETE
✗ "Drop a database" → use drop_database instead
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
        Ok(handler.drop_table(&params).await?)
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
    pub async fn drop_table(&self, request: &DropTableRequest) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        let DropTableRequest {
            database_name,
            table_name,
        } = request;

        validate_ident(database_name)?;
        validate_ident(table_name)?;

        let drop_sql = format!("DROP TABLE {}", quote_ident(table_name, &MySqlDialect {}));
        self.connection.execute(drop_sql.as_str(), Some(database_name)).await?;

        Ok(MessageResponse {
            message: format!("Table '{table_name}' dropped successfully."),
        })
    }
}
