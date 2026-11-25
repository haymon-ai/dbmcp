//! MCP tool: `objects`.

use super::prelude::*;
use crate::types::{ListObjectsResponse, ObjectsRequest};

const NAME: &str = "objects";
const TITLE: &str = "Objects";
const DESCRIPTION: &str = include_str!("../../assets/tools/objects/default.md");

/// Marker type for the `objects` MCP tool.
pub(crate) struct ObjectsTool;

impl ToolBase for ObjectsTool {
    type Parameter = ObjectsRequest;
    type Output = ListObjectsResponse;
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
        Some(input_schema::<Self::Parameter>(false))
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
                .open_world(false),
        )
    }
}

impl AsyncTool<SqliteHandler> for ObjectsTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.objects(params).await
    }
}
