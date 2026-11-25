//! Handler method behind the former `listTriggers` tool.

use dbmcp_server::listing;
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::queries;
use crate::types::ListTriggersRequest;

impl SqliteHandler {
    /// Lists one page of triggers in the connected database, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_triggers(
        &self,
        ListTriggersRequest {
            cursor,
            search,
            detailed,
        }: ListTriggersRequest,
    ) -> Result<ListEntriesResponse, ErrorData> {
        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::triggers::names(&self.connection, page, pattern).await,
            async |page, pattern| queries::triggers::details(&self.connection, page, pattern).await,
        )
        .await
        .map_err(Into::into)
    }
}
