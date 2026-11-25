//! Handler method behind the former `listDatabases` tool.

use dbmcp_server::listing;
use dbmcp_server::types::{ListDatabasesRequest, ListDatabasesResponse};

use super::prelude::*;
use crate::queries;

impl MysqlHandler {
    /// Lists one page of accessible databases.
    ///
    /// # Errors
    ///
    /// Returns an internal-error [`ErrorData`] if the underlying query fails.
    pub async fn list_databases(
        &self,
        ListDatabasesRequest { cursor }: ListDatabasesRequest,
    ) -> Result<ListDatabasesResponse, ErrorData> {
        let (databases, next_cursor) = listing::paginate(cursor, self.config.page_size, async |page| {
            queries::databases::names(&self.connection, page).await
        })
        .await?;

        Ok(ListDatabasesResponse { databases, next_cursor })
    }
}
