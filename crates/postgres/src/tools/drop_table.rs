//! MCP tool: `dropTable`.

use std::borrow::Cow;

use dbmcp_server::types::MessageResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use dbmcp_sql::sanitize::{quote_ident, validate_ident};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use sqlparser::dialect::PostgreSqlDialect;

use crate::PostgresHandler;
use crate::types::DropTableRequest;

/// Marker type for the `dropTable` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "dropTable";
    const TITLE: &'static str = "Drop Table";
    const DESCRIPTION: &'static str = r#"Drop a table from a database. Checks for foreign key dependencies via the database engine.

<usecase>
Use when:
- Removing a table that is no longer needed
- Cleaning up test or temporary tables
</usecase>

<examples>
✓ "Drop the temp_logs table" → dropTable(database="mydb", table="temp_logs")
✓ "Force drop with dependencies" → dropTable(..., cascade=true)
✗ "Delete rows from a table" → use writeQuery with DELETE
✗ "Drop a database" → use dropDatabase instead
</examples>

<safety>
IMPORTANT: This permanently deletes the table and ALL its data. This action cannot be undone.
Set `cascade` to true to also drop dependent foreign key constraints.
Without cascade, the drop will fail if other tables reference this one.
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

impl AsyncTool<PostgresHandler> for DropTableTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_table(params).await?)
    }
}

impl PostgresHandler {
    /// Drops a table from a database.
    ///
    /// Validates identifiers, then executes `DROP TABLE`. When `cascade`
    /// is true the statement uses `CASCADE` to also remove dependent
    /// foreign-key constraints.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the backend reports an error.
    pub async fn drop_table(
        &self,
        DropTableRequest {
            database,
            table,
            cascade,
        }: DropTableRequest,
    ) -> Result<MessageResponse, SqlError> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation);
        }

        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(validate_ident)
            .transpose()?;
        validate_ident(&table)?;

        let mut drop_sql = format!("DROP TABLE {}", quote_ident(&table, &PostgreSqlDialect {}));
        if cascade {
            drop_sql.push_str(" CASCADE");
        }

        self.connection.execute(drop_sql.as_str(), database).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
