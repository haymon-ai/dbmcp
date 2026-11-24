//! MCP tool: `dropTable`.

use dbmcp_server::types::MessageResponse;

use super::prelude::*;
use crate::connection::quote_ident;
use crate::types::DropTableRequest;

const NAME: &str = "dropTable";
const TITLE: &str = "Drop Table";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/drop_table/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/drop_table/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
        .open_world(false)
}

/// Marker type for the `dropTable` MCP tool.
pub(crate) struct DropTableTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for DropTableTool<PINNED> {
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
        Some(
            if PINNED {
                DESCRIPTION_PINNED
            } else {
                DESCRIPTION_UNPINNED
            }
            .into(),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl<const PINNED: bool> AsyncTool<PostgresHandler> for DropTableTool<PINNED> {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.drop_table(params.database, params.table, params.cascade).await
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
    /// or [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn drop_table(
        &self,
        database: Option<String>,
        table: String,
        cascade: bool,
    ) -> Result<MessageResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let mut drop_sql = format!("DROP TABLE {}", quote_ident(&table));
        if cascade {
            drop_sql.push_str(" CASCADE");
        }

        self.connection.execute(drop_sql.as_str(), database).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
