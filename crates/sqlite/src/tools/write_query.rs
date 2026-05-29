//! MCP tool: `writeQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::QueryResponse;

use super::prelude::*;
use crate::types::QueryRequest;

const NAME: &str = "writeQuery";
const TITLE: &str = "Write Query";
const DESCRIPTION: &str = include_str!("../../assets/tools/write_query.md");

/// Marker type for the `writeQuery` MCP tool.
pub(crate) struct WriteQueryTool;

impl ToolBase for WriteQueryTool {
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
        Some(DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<SqliteHandler> for WriteQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params).await
    }
}

impl SqliteHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, QueryRequest { query }: QueryRequest) -> Result<QueryResponse, ErrorData> {
        let rows = self.connection.fetch_json(query.as_str(), None).await?;
        let rows = self.redactor.redact_rows(rows).await?;
        Ok(QueryResponse { rows })
    }
}
