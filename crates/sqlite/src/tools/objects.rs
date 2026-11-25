//! MCP tool: `objects`.

use dbmcp_server::types::names_only;

use super::prelude::*;
use crate::types::{
    ListEntriesResponse, ListTablesRequest, ListTriggersRequest, ListViewsRequest, ObjectKind, ObjectsRequest,
};

const NAME: &str = "objects";
const TITLE: &str = "Objects";
const DESCRIPTION: &str = include_str!("../../assets/tools/objects.md");

/// Marker type for the `objects` MCP tool.
pub(crate) struct ObjectsTool;

impl ToolBase for ObjectsTool {
    type Parameter = ObjectsRequest;
    type Output = ListEntriesResponse;
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
        let ObjectsRequest {
            kind,
            search,
            detail,
            cursor,
        } = params;
        let detailed = detail.is_full();
        match kind {
            ObjectKind::Table => {
                handler
                    .list_tables(ListTablesRequest {
                        cursor,
                        search,
                        detailed,
                    })
                    .await
            }
            ObjectKind::View => {
                names_only("view", search.as_deref(), detail)?;
                handler.list_views(ListViewsRequest { cursor }).await
            }
            ObjectKind::Trigger => {
                handler
                    .list_triggers(ListTriggersRequest {
                        cursor,
                        search,
                        detailed,
                    })
                    .await
            }
        }
    }
}
