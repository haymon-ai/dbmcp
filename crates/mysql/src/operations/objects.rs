//! Logic of the `objects` tool.

use dbmcp_server::listing;
use dbmcp_server::types::names_only;
use rmcp::model::ErrorData;

use crate::MysqlHandler;
use crate::queries;
use crate::types::{ListObjectsResponse, ObjectKind, ObjectsRequest};

impl MysqlHandler {
    /// Lists one page of database objects of the requested kind.
    ///
    /// A blank `database` targets the active database.
    ///
    /// # Errors
    ///
    /// Returns an invalid-params [`ErrorData`] when `kind: database` gets
    /// `search` or `detail: full`, and an internal-error [`ErrorData`] if
    /// `database` is invalid or the underlying query fails.
    pub async fn objects(
        &self,
        ObjectsRequest {
            kind,
            search,
            detail,
            cursor,
            database,
        }: ObjectsRequest,
    ) -> Result<ListObjectsResponse, ErrorData> {
        let database = database
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.connection.default_database_name());
        let page_size = self.config.page_size;
        let search = search.as_deref();
        match kind {
            ObjectKind::Table => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detail,
                    async |page, pattern| queries::tables::names(&self.connection, page, pattern, database).await,
                    async |page, pattern| queries::tables::details(&self.connection, page, pattern, database).await,
                )
                .await
            }
            ObjectKind::View => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detail,
                    async |page, pattern| queries::views::names(&self.connection, page, pattern, database).await,
                    async |page, pattern| queries::views::details(&self.connection, page, pattern, database).await,
                )
                .await
            }
            ObjectKind::Trigger => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detail,
                    async |page, pattern| queries::triggers::names(&self.connection, page, pattern, database).await,
                    async |page, pattern| queries::triggers::details(&self.connection, page, pattern, database).await,
                )
                .await
            }
            ObjectKind::Function => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detail,
                    async |page, pattern| queries::functions::names(&self.connection, page, pattern, database).await,
                    async |page, pattern| queries::functions::details(&self.connection, page, pattern, database).await,
                )
                .await
            }
            ObjectKind::Procedure => {
                listing::list_objects(
                    cursor,
                    page_size,
                    search,
                    detail,
                    async |page, pattern| queries::procedures::names(&self.connection, page, pattern, database).await,
                    async |page, pattern| queries::procedures::details(&self.connection, page, pattern, database).await,
                )
                .await
            }
            ObjectKind::Database => {
                names_only("database", search, detail)?;
                // Server-wide catalog: the default pool lists every database.
                listing::paginate(cursor, page_size, async |page| {
                    queries::databases::names(&self.connection, page).await
                })
                .await
                .map(|(names, next_cursor)| ListObjectsResponse::brief(names, next_cursor))
            }
        }
        .map_err(Into::into)
    }
}
