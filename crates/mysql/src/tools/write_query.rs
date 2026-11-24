//! MCP tool: `writeQuery`.

use dbmcp_privacy::MaybeRedact as _;
use dbmcp_server::truncate::{Truncation, caps};
use dbmcp_server::types::{QueryRequest, WriteQueryResponse};
use dbmcp_sql::validation::validate_write;
use dbmcp_truncate::{Window, fit};

use super::prelude::*;
use dbmcp_policy::Access;

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

/// Marker type for the `writeQuery` MCP tool.
///
/// `PINNED` serves a database pinned by configuration: the input schema has
/// no `database` field, and the pinned route drops one sent anyway.
pub(crate) struct WriteQueryTool<const PINNED: bool>;

impl<const PINNED: bool> ToolBase for WriteQueryTool<PINNED> {
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
        Some(
            if PINNED {
                DESCRIPTION_PINNED
            } else {
                DESCRIPTION_UNPINNED
            }
            .into(),
        )
    }

    fn annotations() -> Option<ToolAnnotations> {
        Some(annotations())
    }

    fn input_schema() -> Option<Arc<JsonObject>> {
        Some(input_schema::<Self::Parameter>(PINNED))
    }

    fn output_schema() -> Option<Arc<JsonObject>> {
        Some(output_schema::<Self::Output>())
    }
}

impl<const PINNED: bool> AsyncTool<MysqlHandler> for WriteQueryTool<PINNED> {
    async fn invoke(handler: &MysqlHandler, params: Self::Parameter) -> Result<Self::Output, Self::Error> {
        handler.write_query(params.query, params.database).await
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
    pub async fn write_query(&self, query: String, database: Option<String>) -> Result<WriteQueryResponse, ErrorData> {
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

        Ok(WriteQueryResponse {
            rows: fitted.items,
            rows_affected,
            last_insert_id,
            truncation: Truncation::new(fitted.cut, fitted.dropped.saturating_add(window.rest)),
        })
    }
}
