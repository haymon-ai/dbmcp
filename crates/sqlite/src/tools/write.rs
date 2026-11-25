//! MCP tool: `write`.

use std::borrow::Cow;
use std::sync::Arc;

use dbmcp_server::types::WriteResponse;
use dbmcp_server::{input_schema, output_schema};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::SqliteHandler;
use crate::types::WriteRequest;

const NAME: &str = "write";
const TITLE: &str = "Write";
const DESCRIPTION: &str = include_str!("../../assets/tools/write/default.md");

/// Marker type for the `write` MCP tool.
pub(crate) struct WriteTool;

impl ToolBase for WriteTool {
    type Parameter = WriteRequest;
    type Output = WriteResponse;
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
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        )
    }
}

impl AsyncTool<SqliteHandler> for WriteTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write(params).await
    }
}
