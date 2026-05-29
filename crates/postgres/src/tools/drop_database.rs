//! MCP tool: `dropDatabase`.

use dbmcp_server::types::{DropDatabaseRequest, MessageResponse};
use dbmcp_sql::SqlError;

use super::prelude::*;
use crate::connection::quote_ident;

const NAME: &str = "dropDatabase";
const TITLE: &str = "Drop Database";
const DESCRIPTION: &str = include_str!("../../assets/tools/drop_database/default.md");

/// Marker type for the `dropDatabase` MCP tool.
pub(crate) struct DropDatabaseTool;

impl ToolBase for DropDatabaseTool {
    type Parameter = DropDatabaseRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>())
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for DropDatabaseTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.drop_database(params).await
    }
}

impl PostgresHandler {
    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected (default) database and
    /// evicts the corresponding pool cache entry after a successful drop.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the target is the active database
    /// or the backend reports an error.
    pub async fn drop_database(
        &self,
        DropDatabaseRequest { database }: DropDatabaseRequest,
    ) -> Result<MessageResponse, ErrorData> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation.into());
        }

        // Guard: prevent dropping the currently connected database.
        if self.connection.default_database_name() == database.as_str() {
            return Err(SqlError::Query(format!("Cannot drop the currently connected database '{database}'.")).into());
        }

        let drop_sql = format!("DROP DATABASE {}", quote_ident(&database));
        self.connection.execute(drop_sql.as_str(), None).await?;

        self.connection.invalidate(&database).await;

        Ok(MessageResponse {
            message: format!("Database '{database}' dropped successfully."),
        })
    }
}
