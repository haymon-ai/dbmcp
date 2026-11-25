//! Handler method behind the former `createDatabase` tool.

use dbmcp_server::types::{CreateDatabaseRequest, MessageResponse};
use dbmcp_sql::SqlError;

use super::prelude::*;
use crate::queries;

impl PostgresHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode, or
    /// [`SqlError`] if the query fails.
    pub async fn create_database(
        &self,
        CreateDatabaseRequest { database }: CreateDatabaseRequest,
    ) -> Result<MessageResponse, ErrorData> {
        queries::databases::create(&self.connection, &database)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("already exists") {
                    return SqlError::Query(format!("Database '{database}' already exists."));
                }
                e
            })?;

        Ok(MessageResponse {
            message: format!("Database '{database}' created successfully."),
        })
    }
}
