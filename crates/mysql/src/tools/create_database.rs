//! MCP tool: `create_database`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{CreateDatabaseRequest, MessageResponse};
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `create_database` MCP tool.
pub(crate) struct CreateDatabaseTool;

impl CreateDatabaseTool {
    const NAME: &'static str = "create_database";
    const DESCRIPTION: &'static str = "Create a new database.";
}

impl ToolBase for CreateDatabaseTool {
    type Parameter = CreateDatabaseRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        Self::NAME.into()
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

impl AsyncTool<MysqlHandler> for CreateDatabaseTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.create_database(&params).await?)
    }
}

impl MysqlHandler {
    /// Creates a database if it doesn't exist.
    ///
    /// # Errors
    ///
    /// Returns [`AppError`] if read-only or the query fails.
    pub async fn create_database(&self, request: &CreateDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let name = &request.database_name;
        validate_identifier(name)?;

        let pool = self.pool.clone();

        // Check existence — use Vec<u8> because MySQL 9 returns BINARY columns
        let check_sql = "SELECT SCHEMA_NAME FROM information_schema.SCHEMATA WHERE SCHEMA_NAME = ?";
        let exists: Option<Vec<u8>> = execute_with_timeout(
            self.config.query_timeout,
            check_sql,
            sqlx::query_scalar(check_sql).bind(name).fetch_optional(&pool),
        )
        .await?;

        if exists.is_some() {
            return Ok(MessageResponse {
                message: format!("Database '{name}' already exists."),
            });
        }

        let create_sql = format!("CREATE DATABASE IF NOT EXISTS {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &create_sql,
            sqlx::query(&create_sql).execute(&pool),
        )
        .await?;

        Ok(MessageResponse {
            message: format!("Database '{name}' created successfully."),
        })
    }
}
