//! MCP tool: `writeQuery`.

use std::borrow::Cow;

use dbmcp_server::types::{PinnedQueryRequest, QueryResponse, UnpinnedQueryRequest};
use dbmcp_sql::Connection as _;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

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
    type Parameter = PinnedQueryRequest;
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
}

impl AsyncTool<MysqlHandler> for PinnedWriteQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, None).await
    }
}

/// Marker type for the `writeQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedWriteQueryTool;

impl ToolBase for UnpinnedWriteQueryTool {
    type Parameter = UnpinnedQueryRequest;
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
}

impl AsyncTool<MysqlHandler> for UnpinnedWriteQueryTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.inner.query, params.database).await
    }
}

impl MysqlHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, query: String, database: Option<String>) -> Result<QueryResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let mut rows = self.connection.fetch_json(query.as_str(), database).await?;
        if let Some(r) = &self.redactor {
            r.apply(&mut rows)?;
        }

        Ok(QueryResponse { rows })
    }
}
