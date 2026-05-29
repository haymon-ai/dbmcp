//! MCP tool: `writeQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::{QueryRequest, QueryResponse};

use super::prelude::*;

const NAME: &str = "writeQuery";
const TITLE: &str = "Write Query";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/write_query/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/write_query/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
        .open_world(true)
}

/// Marker type for the `writeQuery` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedWriteQueryTool;

impl ToolBase for PinnedWriteQueryTool {
    type Parameter = QueryRequest;
    type Output = QueryResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_PINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for PinnedWriteQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, None).await
    }
}

/// Marker type for the `writeQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedWriteQueryTool;

impl ToolBase for UnpinnedWriteQueryTool {
    type Parameter = QueryRequest;
    type Output = QueryResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_UNPINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(false))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for UnpinnedWriteQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, params.database).await
    }
}

impl PostgresHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, query: String, database: Option<String>) -> Result<QueryResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let rows = self.connection.fetch_json(query.as_str(), database).await?;
        let rows = self.redactor.redact_rows(rows).await?;
        Ok(QueryResponse { rows })
    }
}
