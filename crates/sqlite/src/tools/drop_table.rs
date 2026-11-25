//! Handler method behind the former `dropTable` tool.

use dbmcp_server::types::MessageResponse;

use super::prelude::*;
use crate::queries;
use crate::types::DropTableRequest;

impl SqliteHandler {
    /// Drops a table from the database.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// or [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn drop_table(&self, DropTableRequest { table }: DropTableRequest) -> Result<MessageResponse, ErrorData> {
        queries::tables::drop(&self.connection, &table).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
