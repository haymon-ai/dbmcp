//! Handler method behind the former `dropTable` tool.

use dbmcp_server::types::MessageResponse;

use super::prelude::*;
use crate::queries;

impl MysqlHandler {
    /// Drops a table from a database.
    ///
    /// Executes `DROP TABLE` on the pool for the target database.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// or [`SqlError::Query`] if the backend reports an error.
    ///
    /// [`SqlError::Query`]: dbmcp_sql::SqlError::Query
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn drop_table(&self, database: Option<String>, table: String) -> Result<MessageResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        queries::tables::drop(&self.connection, &table, database).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
