//! MCP tool: `writeQuery`.

use dbmcp_pii::MaybeRedact as _;
use dbmcp_server::types::WriteQueryResponse;
use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;

use super::prelude::*;
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
    /// Returns [`SqlError`] if the query fails.
    pub async fn write_query(&self, QueryRequest { query }: QueryRequest) -> Result<WriteQueryResponse, ErrorData> {
        let (rows, results) = self.connection.fetch_json_with_results(query.as_str(), None).await?;
        let rows = self.redactor.redact_rows(rows).await?;
        let (rows_affected, last_insert_id) = counters(
            &query,
            results.iter().map(|r| (r.rows_affected(), r.last_insert_rowid())),
        );
        Ok(WriteQueryResponse {
            rows,
            rows_affected,
            last_insert_id,
        })
    }
}

/// Sums affected rows and picks the insert id from the statements that set them.
///
/// `SQLite` keeps both per connection, so other statements still report
/// earlier values. Unparsable SQL reports `0`.
fn counters(sql: &str, results: impl IntoIterator<Item = (u64, i64)>) -> (u64, Option<u64>) {
    let statements = Parser::parse_sql(&SQLiteDialect {}, sql).unwrap_or_default();
    let mut rows_affected = 0;
    let mut last_insert_id = None;
    for (statement, (changes, rowid)) in statements.iter().zip(results) {
        match statement {
            Statement::Insert(_) if changes > 0 => {
                rows_affected += changes;
                last_insert_id = u64::try_from(rowid).ok();
            }
            Statement::Update(_) | Statement::Delete(_) => rows_affected += changes,
            _ => {}
        }
    }
    (rows_affected, last_insert_id)
}

#[cfg(test)]
mod tests {
    use super::counters;

    #[test]
    fn insert_reports_changes_and_rowid() {
        assert_eq!(counters("INSERT INTO t (a) VALUES (1), (2)", [(2, 7)]), (2, Some(7)));
    }

    #[test]
    fn replace_counts_as_insert() {
        assert_eq!(counters("REPLACE INTO t (id) VALUES (3)", [(1, 3)]), (1, Some(3)));
    }

    #[test]
    fn update_and_delete_report_changes_without_rowid() {
        assert_eq!(counters("UPDATE t SET a = 2", [(4, 9)]), (4, None));
        assert_eq!(counters("DELETE FROM t", [(4, 9)]), (4, None));
    }

    #[test]
    fn insert_without_changes_reports_no_rowid() {
        assert_eq!(
            counters("INSERT INTO t (id) VALUES (1) ON CONFLICT DO NOTHING", [(0, 9)]),
            (0, None)
        );
    }

    #[test]
    fn schema_change_ignores_stale_counters() {
        assert_eq!(counters("CREATE TABLE t (a INTEGER)", [(5, 9)]), (0, None));
    }

    #[test]
    fn statements_pair_with_results_in_order() {
        let sql = "INSERT INTO t (a) VALUES (1); CREATE TABLE u (b INTEGER); UPDATE t SET a = 3";
        assert_eq!(counters(sql, [(1, 4), (1, 4), (2, 4)]), (3, Some(4)));
    }

    #[test]
    fn unparsable_sql_reports_nothing() {
        assert_eq!(counters("NOT SQL", [(1, 4)]), (0, None));
    }
}
