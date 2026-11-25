//! Handler method behind the former `createDatabase` tool.

use dbmcp_server::types::{CreateDatabaseRequest, MessageResponse};

use super::prelude::*;
use crate::queries;

impl MysqlHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode, or
    /// [`SqlError`] if the query fails.
    ///
    /// [`SqlError`]: dbmcp_sql::SqlError
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    pub async fn create_database(
        &self,
        CreateDatabaseRequest { database }: CreateDatabaseRequest,
    ) -> Result<MessageResponse, ErrorData> {
        if queries::databases::exists(&self.connection, &database).await? {
            return Ok(MessageResponse {
                message: format!("Database '{database}' already exists."),
            });
        }

        queries::databases::create(&self.connection, &database).await?;

        Ok(MessageResponse {
            message: format!("Database '{database}' created successfully."),
        })
    }
}
