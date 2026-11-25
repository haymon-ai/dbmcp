//! MCP tool: `explain`.

use std::borrow::Cow;
use std::sync::Arc;

use dbmcp_server::types::ExplainResponse;
use dbmcp_server::{input_schema, output_schema};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::ExplainRequest;

const NAME: &str = "explain";
const TITLE: &str = "Explain";
const DESCRIPTION: &str = include_str!("../../assets/tools/explain/default.md");

/// Marker type for the `explain` MCP tool.
pub(crate) struct ExplainTool;

impl ToolBase for ExplainTool {
    type Parameter = ExplainRequest;
    type Output = ExplainResponse;
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(true),
        )
    }
}

impl AsyncTool<SqliteHandler> for ExplainTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.explain(params).await
    }
}
