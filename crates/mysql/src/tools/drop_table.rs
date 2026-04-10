//! MCP tool: `drop_table`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::MessageResponse;
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};
use sqlx::Executor;

use crate::MysqlHandler;
use crate::types::DropTableRequest;

/// Marker type for the `drop_table` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "drop_table";
    const DESCRIPTION: &'static str = "Drop a table from a database.";
}

impl ToolBase for DropTableTool {
    type Parameter = DropTableRequest;
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

impl AsyncTool<MysqlHandler> for DropTableTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_table(&params).await?)
    }
}

impl MysqlHandler {
    /// Drops a table from a database.
    ///
    /// Switches to the target database with `USE`, then executes
    /// `DROP TABLE`.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::ReadOnlyViolation`] in read-only mode,
    /// [`AppError::InvalidIdentifier`] for invalid names,
    /// or [`AppError::Query`] if the backend reports an error.
    pub async fn drop_table(&self, request: &DropTableRequest) -> Result<MessageResponse, AppError> {
        if self.config.read_only {
            return Err(AppError::ReadOnlyViolation);
        }
        let database = &request.database_name;
        let table = &request.table_name;
        validate_identifier(database)?;
        validate_identifier(table)?;

        let pool = self.pool.clone();
        let db = database.clone();
        let drop_sql = format!("DROP TABLE {}", Self::quote_identifier(table));
        let drop_sql_label = drop_sql.clone();

        execute_with_timeout(self.config.query_timeout, &drop_sql_label, async move {
            let mut conn = pool.acquire().await?;

            let use_sql = format!("USE {}", Self::quote_identifier(&db));
            conn.execute(use_sql.as_str()).await?;

            conn.execute(drop_sql.as_str()).await?;
            Ok::<_, sqlx::Error>(())
        })
        .await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
