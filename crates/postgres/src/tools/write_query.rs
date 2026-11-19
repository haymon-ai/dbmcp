//! MCP tool: `writeQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::truncate;
use dbmcp_server::types::{QueryRequest, WriteQueryResponse};

use super::prelude::*;
use crate::policy::Access;

const NAME: &str = "writeQuery";
const TITLE: &str = "Write Query";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/write_query/pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/write_query/unpinned.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
        .open_world(true)
}

/// Marker type for the `writeQuery` MCP tool (pinned variant — no `database` field).
pub(crate) struct PinnedWriteQueryTool;

impl ToolBase for PinnedWriteQueryTool {
    type Parameter = QueryRequest;
    type Output = WriteQueryResponse;
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for PinnedWriteQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, None).await
    }
}

/// Marker type for the `writeQuery` MCP tool (unpinned variant — carries `database`).
pub(crate) struct UnpinnedWriteQueryTool;

impl ToolBase for UnpinnedWriteQueryTool {
    type Parameter = QueryRequest;
    type Output = WriteQueryResponse;
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

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(false))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<PostgresHandler> for UnpinnedWriteQueryTool {
    async fn invoke(handler: &PostgresHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, params.database).await
    }
}

impl PostgresHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode, or
    /// [`SqlError`] if the query fails. With column rules, also the
    /// policy's rejection or a withheld database error.
    pub async fn write_query(&self, query: String, database: Option<String>) -> Result<WriteQueryResponse, ErrorData> {
        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());
        let mut window = truncate::Window::new(0, None, self.config.max_cell_chars, self.config.max_response_chars);
        let (rows, rows_affected, last_insert_id) = if let Some(policy) = &self.policy {
            let timeout = self.connection.query_timeout();
            let enforced = self
                .enforce(policy, &query, Access::Write, timeout, database, &mut window)
                .await?;
            (enforced.rows, enforced.rows_affected, enforced.last_insert_id)
        } else {
            let (rows_affected, last_insert_id) = self
                .connection
                .execute_json(query.as_str(), database, |row| window.push(row))
                .await?;
            let rows = self.redactor.redact_rows(window.rows).await?;
            (rows, rows_affected, last_insert_id)
        };
        let mut fitted = truncate::fit(rows, self.config.max_cell_chars, self.config.max_response_chars);
        fitted.omit(window.rest);

        Ok(WriteQueryResponse {
            rows: fitted.rows,
            rows_affected,
            last_insert_id,
            truncation: fitted.truncation,
        })
    }
}
