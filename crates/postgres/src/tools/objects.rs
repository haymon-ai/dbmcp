//! MCP tool: `objects`.

use dbmcp_server::types::{ListDatabasesRequest, names_only, unavailable_kind};

use super::prelude::*;
use crate::types::{ListEntriesResponse, ObjectKind, ObjectsRequest};

const NAME: &str = "objects";
const TITLE: &str = "Objects";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/objects/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/objects/unpinned.md");

/// Marker type for the `objects` MCP tool.
pub(crate) struct ObjectsTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for ObjectsTool<PINNED> {
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
        let ObjectsRequest {
            kind,
            search,
            detail,
            cursor,
            database,
        } = params;
        let detailed = detail.is_full();
        match kind {
            ObjectKind::Table => handler.list_tables(database, cursor, search, detailed).await,
            ObjectKind::View => handler.list_views(database, cursor, search, detailed).await,
            ObjectKind::MaterializedView => {
                handler
                    .list_materialized_views(database, cursor, search, detailed)
                    .await
            }
            ObjectKind::Trigger => handler.list_triggers(database, cursor, search, detailed).await,
            ObjectKind::Function => handler.list_functions(database, cursor, search, detailed).await,
            ObjectKind::Procedure => handler.list_procedures(database, cursor, search, detailed).await,
            ObjectKind::Database if PINNED => Err(unavailable_kind("database", "a database is pinned")),
            ObjectKind::Database => {
                names_only("database", search.as_deref(), detail)?;
                let page = handler.list_databases(ListDatabasesRequest { cursor }).await?;
                Ok(ListEntriesResponse::brief(page.databases, page.next_cursor))
            }
        }
    }
}
