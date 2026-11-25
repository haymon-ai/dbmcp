//! MCP tool: `write`.

use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::{WriteRequest, WriteResponse};
use dbmcp_sql::validation::validate_write;
use dbmcp_truncate::{Window, fit};

use super::prelude::*;
use dbmcp_policy::Access;

const NAME: &str = "write";
const TITLE: &str = "Write";
const DESCRIPTION_PINNED: &str = include_str!("../../assets/tools/write/default.pinned.md");
const DESCRIPTION_UNPINNED: &str = include_str!("../../assets/tools/write/default.md");

fn annotations() -> ToolAnnotations {
    ToolAnnotations::new()
        .read_only(false)
        .destructive(true)
        .idempotent(false)
        .open_world(true)
}

/// Marker type for the `write` MCP tool.
pub(crate) struct WriteTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for WriteTool<PINNED> {
    type Parameter = WriteRequest;
    type Output = WriteResponse;
    type Error = ErrorData;

    fn name() -> Cow<'static, str> {
        NAME.into()
    }

    fn title() -> Option<String> {
        Some(TITLE.into())
    }

    fn description() -> Option<Cow<'static, str>> {
        Some(
            if PINNED {
                DESCRIPTION_PINNED
            } else {
                DESCRIPTION_UNPINNED
            }
            .into(),
        )
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }
}

impl<const PINNED: bool> AsyncTool<MysqlHandler> for WriteTool<PINNED> {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write(params.query, params.database).await
    }
}

impl MysqlHandler {
    /// Executes a write SQL query.
    ///
    /// Rejects `USE`: it would change the database of the pooled connection
    /// for later queries.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::UnreadableCharacter`] for SQL the server reads
    /// otherwise, [`SqlError::UseBlocked`] for `USE`,
    /// [`SqlError::ReadOnlyViolation`] in read-only mode, or [`SqlError`]
    /// if the query fails. With column rules, also
    /// [`SqlError::MultiStatement`], [`SqlError::ExecutableCommentBlocked`],
    /// the policy's rejection, or a withheld database error.
    ///
    /// [`SqlError`]: dbmcp_sql::SqlError
    /// [`SqlError::ExecutableCommentBlocked`]: dbmcp_sql::SqlError::ExecutableCommentBlocked
    /// [`SqlError::MultiStatement`]: dbmcp_sql::SqlError::MultiStatement
    /// [`SqlError::ReadOnlyViolation`]: dbmcp_sql::SqlError::ReadOnlyViolation
    /// [`SqlError::UnreadableCharacter`]: dbmcp_sql::SqlError::UnreadableCharacter
    /// [`SqlError::UseBlocked`]: dbmcp_sql::SqlError::UseBlocked
    pub async fn write(&self, query: String, database: Option<String>) -> Result<WriteResponse, ErrorData> {
        validate_write(&query, &sqlparser::dialect::MySqlDialect {})?;

        let database = database.as_deref().map(str::trim).filter(|s| !s.is_empty());

        let caps = caps(self.config.max_cell_chars, self.config.max_response_chars);
        let mut window = Window::new(0, None, caps);
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
            let rows = self
                .redactor
                .redact_rows(window.items)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            (rows, rows_affected, last_insert_id)
        };
        let fitted = fit(rows, caps);

        Ok(WriteResponse {
            rows: fitted.items,
            rows_affected,
            last_insert_id,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
