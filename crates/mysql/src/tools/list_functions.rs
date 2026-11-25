//! Handler method behind the former `listFunctions` tool.

use dbmcp_server::listing::{self, Cursor};
use dbmcp_server::types::ListEntriesResponse;

use super::prelude::*;
use crate::queries;

impl MysqlHandler {
    /// Lists one page of stored functions, optionally filtered and/or detailed.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if `database` is invalid
    /// or the underlying query fails.
    pub async fn list_functions(
        &self,
        database: Option<String>,
        cursor: Option<Cursor>,
        search: Option<String>,
        detailed: bool,
    ) -> Result<ListEntriesResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.connection.default_database_name());

        listing::list_entries(
            cursor,
            self.config.page_size,
            search.as_deref(),
            detailed,
            async |page, pattern| queries::functions::names(&self.connection, page, pattern, database).await,
            async |page, pattern| queries::functions::details(&self.connection, page, pattern, database).await,
        )
        .await
        .map_err(Into::into)
    }
}
