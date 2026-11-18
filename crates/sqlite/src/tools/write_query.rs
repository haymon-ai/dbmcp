//! MCP tool: `writeQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::truncate;
use dbmcp_server::types::WriteQueryResponse;

use super::prelude::*;
use crate::policy::Access;
use crate::types::QueryRequest;

const NAME: &str = "writeQuery";
const TITLE: &str = "Write Query";
const DESCRIPTION: &str = include_str!("../../assets/tools/write_query.md");

/// Marker type for the `writeQuery` MCP tool.
pub(crate) struct WriteQueryTool;

impl ToolBase for WriteQueryTool {
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
        Some(DESCRIPTION.into())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(
            ToolAnnotations::new()
                .read_only(false)
                .destructive(true)
                .idempotent(false)
                .open_world(true),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(true))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl AsyncTool<SqliteHandler> for WriteQueryTool {
    async fn invoke(handler: &SqliteHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params).await
    }
}

impl SqliteHandler {
    /// Executes a write SQL query.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] in read-only mode, or
    /// [`SqlError`] if the query fails. With column rules, also the
    /// policy's rejection or a withheld database error.
    pub async fn write_query(&self, QueryRequest { query }: QueryRequest) -> Result<WriteQueryResponse, ErrorData> {
        let mut window = truncate::Window::new(0, None, self.config.max_cell_chars, self.config.max_response_chars);
        let (rows, rows_affected, last_insert_id) = if let Some(policy) = &self.policy {
            let timeout = self.connection.query_timeout();
            let enforced = self
                .enforce(policy, &query, Access::Write, timeout, &mut window)
                .await?;
            (enforced.rows, enforced.rows_affected, enforced.last_insert_id)
        } else {
            let (rows_affected, last_insert_id) = self
                .connection
                .execute_json(query.as_str(), None, |row| window.push(row))
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
