//! MCP tool: `objects`.

use dbmcp_server::listing;
use dbmcp_server::types::names_only;

use super::prelude::*;
use crate::queries;
use crate::types::{ListObjectsResponse, ObjectKind, ObjectsRequest};

const NAME: &str = "objects";
const TITLE: &str = "Objects";
const DESCRIPTION: &str = include_str!("../../assets/tools/objects.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(true)
        .destructive(false)
        .idempotent(true)
        .open_world(false)
}

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
        Some(annotations())
    }
}

impl AsyncTool<SqliteHandler> for ObjectsTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.objects(params).await
    }
}

impl SqliteHandler {
    /// Lists one page of database objects of the requested kind.
    ///
    /// # Errors
    ///
    /// Returns an invalid-params [`ErrorData`] when `kind: view` gets `search`
    /// or `detail: full`, and an internal-error [`ErrorData`] if the
    /// underlying query fails.
    pub async fn objects(
        &self,
        ObjectsRequest {
            kind,
            search,
            detail,
            cursor,
        }: ObjectsRequest,
    ) -> Result<ListObjectsResponse, ErrorData> {
        let page_size = self.config.page_size;
        let search = search.as_deref();
        let detailed = detail.is_full();
        match kind {
            ObjectKind::Table => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detailed,
                    async |page, pattern| queries::tables::names(&self.connection, page, pattern).await,
                    async |page, pattern| queries::tables::details(&self.connection, page, pattern).await,
                )
                .await
            }
            ObjectKind::View => {
                names_only("view", search, detail)?;
                let (views, next_cursor) = listing::paginate(cursor, page_size, async |page| {
                    queries::views::names(&self.connection, page).await
                })
                .await?;
                return Ok(ListObjectsResponse::brief(views, next_cursor));
            }
            ObjectKind::Trigger => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detailed,
                    async |page, pattern| queries::triggers::names(&self.connection, page, pattern).await,
                    async |page, pattern| queries::triggers::details(&self.connection, page, pattern).await,
                )
                .await
            }
        }
        .map_err(Into::into)
    }
}
