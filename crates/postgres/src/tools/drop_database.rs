//! Handler method behind the former `dropDatabase` tool.

use dbmcp_server::types::{DropDatabaseRequest, MessageResponse};
use dbmcp_sql::SqlError;

use super::prelude::*;
use crate::queries;

impl PostgresHandler {
    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected (default) database and
    /// evicts the corresponding pool cache entry after a successful drop.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// or [`SqlError::Query`] if the target is the active database
    /// or the backend reports an error.
    pub async fn drop_database(
        &self,
        DropDatabaseRequest { database }: DropDatabaseRequest,
    ) -> Result<MessageResponse, ErrorData> {
        // Guard: prevent dropping the currently connected database.
        if self.connection.default_database_name() == database.as_str() {
            return Err(SqlError::Query(format!("Cannot drop the currently connected database '{database}'.")).into());
        }

        queries::databases::drop(&self.connection, &database).await?;

        self.connection.invalidate(&database).await;

        Ok(MessageResponse {
            message: format!("Database '{database}' dropped successfully."),
        })
    }
}
