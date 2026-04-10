//! MCP tool: `drop_table`.

use std::borrow::Cow;

use database_mcp_server::AppError;
use database_mcp_server::types::MessageResponse;
use database_mcp_sql::identifier::validate_identifier;
use database_mcp_sql::timeout::execute_with_timeout;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::PostgresHandler;
use crate::types::DropTableRequest;

/// Marker type for the `drop_table` MCP tool.
pub(crate) struct DropTableTool;

impl DropTableTool {
    const NAME: &'static str = "drop_table";
    const DESCRIPTION: &'static str = "Drop a table from a database. Checks for foreign key dependencies\nvia the database engine — use `cascade` to force on `PostgreSQL`.";
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

impl AsyncTool<PostgresHandler> for DropTableTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        Ok(handler.drop_table(&params).await?)
    }
}

impl PostgresHandler {
    /// Drops a table from a database.
    ///
    /// Validates identifiers, then executes `DROP TABLE`. When `cascade`
    /// is true the statement uses `CASCADE` to also remove dependent
    /// foreign-key constraints.
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

        let pool = self.get_pool(Some(database)).await?;

        let mut drop_sql = format!("DROP TABLE {}", Self::quote_identifier(table));
        if request.cascade {
            drop_sql.push_str(" CASCADE");
        }

        execute_with_timeout(
            self.config.query_timeout,
            &drop_sql,
            sqlx::query(&drop_sql).execute(&pool),
        )
        .await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
