//! MCP tool: `read`.

use dbmcp_server::types::{ReadRequest, ReadResponse};

use super::prelude::*;

const NAME: &str = "read";
const TITLE: &str = "Read";
const DESCRIPTION: &str = include_str!("../../assets/tools/read/default.md");

/// Marker type for the `read` MCP tool.
pub(crate) struct ReadTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ReadTool<PINNED> {
    type Parameter = ReadRequest;
    type Output = ReadResponse;
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
        Some(input_schema::<Self::Parameter>(PINNED))
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

impl<const PINNED: bool> AsyncTool<PostgresHandler> for ReadTool<PINNED> {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler
            .read(params.query, params.database, params.cursor, params.timeout)
            .await
    }
}
