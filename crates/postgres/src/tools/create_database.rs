//! MCP tool: `createDatabase`.

use std::borrow::Cow;

use dbmcp_server::types::{CreateDatabaseRequest, MessageResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::connection::quote_ident;

/// Marker type for the `createDatabase` MCP tool.
pub(crate) struct CreateDatabaseTool;

impl CreateDatabaseTool {
    const NAME: &'static str = "createDatabase";
    const TITLE: &'static str = "Create Database";
    const DESCRIPTION: &'static str = include_str!("../../assets/tools/create_database.md");
}

impl ToolBase for CreateDatabaseTool {
    type Parameter = CreateDatabaseRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
    }

    fn title() -> Option<String> {
        Some(Self::TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(Self::DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(false)
                .idempotent(false)
                .open_world(false),
        )
    }
}

impl AsyncTool<PostgresHandler> for CreateDatabaseTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.create_database(params).await
    }
}

impl PostgresHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError`] if read-only or the query fails.
    pub async fn create_database(
        &self,
        CreateDatabaseRequest { database }: CreateDatabaseRequest,
    ) -> Result<MessageResponse, ErrorData> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation.into());
        }

        let create_sql = format!("CREATE DATABASE {}", quote_ident(&database));
        self.connection.execute(create_sql.as_str(), None).await.map_err(|e| {
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
