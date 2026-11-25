//! MCP tool: `objects`.

use dbmcp_server::types::unavailable_kind;

use super::prelude::*;
use crate::types::{ListObjectsResponse, ObjectKind, ObjectsRequest};

const NAME: &str = "objects";
const TITLE: &str = "Objects";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/objects/default.pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/objects/default.md");

/// Marker type for the `objects` MCP tool.
pub(crate) struct ObjectsTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ObjectsTool<PINNED> {
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
                .read_only(true)
                .destructive(false)
                .idempotent(true)
                .open_world(false),
        )
    }
}

impl<const PINNED: bool> AsyncTool<PostgresHandler> for ObjectsTool<PINNED> {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        if PINNED && params.kind == ObjectKind::Database {
            return Err(unavailable_kind("database", "a database is pinned"));
        }
        handler.objects(params).await
    }
}
