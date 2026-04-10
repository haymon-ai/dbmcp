//! MCP tool: `drop_database`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::{DropDatabaseRequest, MessageResponse};
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;

/// Marker type for the `drop_database` MCP tool.
pub(crate) struct DropDatabaseTool;

impl DropDatabaseTool {
    const NAME: &'static str = "drop_database";
    const DESCRIPTION: &'static str = "Drop an existing database. Cannot drop the currently connected database.";
}

impl ToolBase for DropDatabaseTool {
    type Parameter = DropDatabaseRequest;
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
                .destructive(true)
                .idempotent(false)
                .open_world(false),
        )
    }
}

impl AsyncTool<MysqlHandler> for DropDatabaseTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_database(&params).await?)
    }
}

impl MysqlHandler {
    /// Drops an existing database.
    ///
    /// Refuses to drop the currently connected database.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] in read-only mode,
    /// [`AppError::InvalidIdentifier`] for invalid names,
    /// or [`AppError::Query`] if the target is the active database
    /// or the backend reports an error.
    pub async fn drop_database(&self, request: &DropDatabaseRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let name = &request.database_name;
        validate_identifier(name)?;

        // Guard: prevent dropping the currently connected database.
        if let Some(ref active) = self.config.name
            && active.eq_ignore_ascii_case(name)
        {
            return Err(AppError::Query(format!(
                "Cannot drop the currently connected database '{name}'."
            )));
        }

        let pool = self.pool.clone();
        let drop_sql = format!("DROP DATABASE {}", Self::quote_identifier(name));
        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        Ok(MessageResponse {
            message: format!("Database '{name}' dropped successfully."),
        })
    }
}
