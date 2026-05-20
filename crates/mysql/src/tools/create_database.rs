//! MCP tool: `createDatabase`.

use std::borrow::Cow;

use dbmcp_server::types::{CreateDatabaseRequest, MessageResponse};
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::connection::quote_ident;

const NAME: &str = "createDatabase";
const TITLE: &str = "Create Database";
const DESCRIPTION: &str = include_str!("../../assets/tools/create_database/default.md");

/// Marker type for the `createDatabase` MCP tool.
pub(crate) struct CreateDatabaseTool;

impl ToolBase for CreateDatabaseTool {
    type Parameter = CreateDatabaseRequest;
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
                .destructive(false)
                .idempotent(false)
                .open_world(false),
        )
    }
}

impl AsyncTool<MysqlHandler> for CreateDatabaseTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.create_database(params).await
    }
}

impl MysqlHandler {
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

        let exists: Option<String> = self
            .connection
            .fetch_optional(
                sqlx::query(
                    "SELECT CAST(SCHEMA_NAME AS CHAR) \
                     FROM information_schema.SCHEMATA \
                     WHERE SCHEMA_NAME = ?",
                )
                .bind(&database),
                None,
            )
            .await?;

        if exists.is_some() {
            return Ok(MessageResponse {
                message: format!("Database '{database}' already exists."),
            });
        }

        let create_sql = format!("CREATE DATABASE IF NOT EXISTS {}", quote_ident(&database));

        self.connection.execute(create_sql.as_str(), None).await?;

        Ok(MessageResponse {
            message: format!("Database '{database}' created successfully."),
        })
    }
}
