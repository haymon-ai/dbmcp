//! MCP tool: `write`.

use std::borrow::Cow;
use std::sync::Arc;

use dbmcp_server::types::{WriteRequest, WriteResponse};
use dbmcp_server::{input_schema, output_schema};
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, JsonObject, ToolAnnotations};

use crate::MysqlHandler;

const NAME: &str = "write";
const TITLE: &str = "Write";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/write/default.pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/write/default.md");

/// Marker type for the `write` MCP tool.
pub(crate) struct WriteTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for WriteTool<PINNED> {
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
        Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        )
    }
}

impl<const PINNED: bool> AsyncTool<MysqlHandler> for WriteTool<PINNED> {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write(params.query, params.database).await
    }
}
