//! Logic of the `objects` tool.

use dbmcp_server::listing;
use dbmcp_server::types::names_only;
use rmcp::model::ErrorData;

use crate::SqliteHandler;
use crate::queries;
use crate::types::{ListObjectsResponse, ObjectKind, ObjectsRequest};

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
