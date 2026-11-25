//! MCP tool: `listTables`.

use dbmcp_server::listing::{self, Cursor};

use super::prelude::*;
use crate::queries;
use crate::types::{ListEntriesResponse, ListTablesRequest};

const NAME: &str = "listTables";
const TITLE: &str = "List Tables";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/list_tables/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/list_tables/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

/// Marker type for the `listTables` MCP tool.
pub(crate) struct ListTablesTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ListTablesTool<PINNED> {
    type Parameter = ListTablesRequest;
    type Output = ListEntriesResponse;
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

impl<const PINNED: bool> AsyncTool<PostgresHandler> for ListTablesTool<PINNED> {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .list_tables(params.database, params.cursor, params.search, params.detailed)
            .await
    }
}

impl PostgresHandler {
    /// Lists one page of tables in a database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_tables(
        &self,
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
        // Identifier validation runs inside the connection pool — passing the
        // trimmed name straight through avoids a redundant round-trip check.
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::tables::names(&self.connection, page, pattern, database).await,
            async |page, pattern| queries::tables::details(&self.connection, page, pattern, database).await,
        )
        .await
        .map_err(Into::into)
    }
}
