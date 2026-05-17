//! MCP tool: `writeQuery`.

use std::borrow::Cow;

use dbmcp_server::types::QueryResponse;

use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::QueryRequest;

/// Marker type for the `writeQuery` MCP tool.
pub(crate) struct WriteQueryTool;

impl WriteQueryTool {
    const NAME: &'static str = "writeQuery";
    const TITLE: &'static str = "Write Query";
    const DESCRIPTION: &'static str = include_str!("../../assets/tools/write_query.md");
}

impl ToolBase for WriteQueryTool {
    type Parameter = QueryRequest;
    type Output = QueryResponse;
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
                .open_world(true),
        )
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
        let mut rows = self.connection.fetch_json(query.as_str(), None).await?;
        if let Some(r) = &self.redactor {
            r.apply(&mut rows)?;
        }
        Ok(QueryResponse { rows })
    }
}
