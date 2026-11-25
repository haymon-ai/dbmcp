//! Handler method behind the former `dropTable` tool.

use dbmcp_server::types::MessageResponse;

use super::prelude::*;
use crate::queries;

impl PostgresHandler {
    /// Drops a table from a database.
    ///
    /// Validates identifiers, then executes `DROP TABLE`. When `cascade`
    /// is true the statement uses `CASCADE` to also remove dependent
    /// foreign-key constraints.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// or [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn drop_table(
        &self,
        database: Option<String>,
        table: String,
        cascade: bool,
    ) -> Result<MessageResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        queries::tables::drop(&self.connection, &table, database, cascade).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
