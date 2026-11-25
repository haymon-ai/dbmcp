//! Handler method behind the former `listMaterializedViews` tool.

use dbmcp_server::listing::{self, Cursor};

use super::prelude::*;
use crate::queries;
use crate::types::ListEntriesResponse;

impl PostgresHandler {
    /// Lists one page of user-defined materialized views, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_materialized_views(
        &self,
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::materialized_views::names(&self.connection, page, pattern, database).await,
            async |page, pattern| queries::materialized_views::details(&self.connection, page, pattern, database).await,
        )
        .await
        .map_err(Into::into)
    }
}
