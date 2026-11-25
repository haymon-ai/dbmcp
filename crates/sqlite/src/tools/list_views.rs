//! Handler method behind the former `listViews` tool.

use dbmcp_server::listing;
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::queries;
use crate::types::ListViewsRequest;

impl SqliteHandler {
    /// Lists one page of views in the connected database.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_views(
        &self,
        ListViewsRequest { cursor }: ListViewsRequest,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let (views, next_cursor) = listing::paginate(cursor, self.config.page_size, async |page| {
            queries::views::names(&self.connection, page).await
        })
        .await?;

        Ok(ListEntriesResponse::brief(views, next_cursor))
    }
}
