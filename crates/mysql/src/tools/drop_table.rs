//! MCP tool: `dropTable`.

use std::borrow::Cow;

use dbmcp_server::types::MessageResponse;
use dbmcp_sql::Connection as _;
use dbmcp_sql::SqlError;
use rmcp::handler::server::router::tool::{AsyncTool, ToolBase};
use rmcp::model::{ErrorData, ToolAnnotations};

use crate::MysqlHandler;
use crate::connection::quote_ident;
use crate::types::{PinnedDropTableRequest, UnpinnedDropTableRequest};

const NAME: &str = "dropTable";
const TITLE: &str = "Drop Table";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/drop_table/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/drop_table/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
        .open_world(false)
}

/// Marker type for the `dropTable` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedDropTableTool;

impl ToolBase for PinnedDropTableTool {
    type Parameter = PinnedDropTableRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_PINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl AsyncTool<MysqlHandler> for PinnedDropTableTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.drop_table(None, params.table).await
    }
}

/// Marker type for the `dropTable` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedDropTableTool;

impl ToolBase for UnpinnedDropTableTool {
    type Parameter = UnpinnedDropTableRequest;
    type Output = MessageResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(DESCRIPTION_UNPINNED.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl AsyncTool<MysqlHandler> for UnpinnedDropTableTool {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.drop_table(params.database, params.inner.table).await
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
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode,
    /// [`SqlError::InvalidIdentifier`] for invalid names,
    /// or [`SqlError::Query`] if the backend reports an error.
    pub async fn drop_table(&self, database: Option<String>, table: String) -> Result<MessageResponse, ErrorData> {
        if self.config.read_only {
            return Err(SqlError::ReadOnlyViolation.into());
        }

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let drop_sql = format!("DROP TABLE {}", quote_ident(&table));
        self.connection.execute(drop_sql.as_str(), database).await?;

        Ok(MessageResponse {
            message: format!("Table '{table}' dropped successfully."),
        })
    }
}
