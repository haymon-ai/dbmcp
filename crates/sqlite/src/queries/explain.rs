//! `EXPLAIN QUERY PLAN` statement builder for `explainQuery`.

/// Prefix of every plan request; `SQLite` has no `EXPLAIN ANALYZE`.
const STATEMENT: &str = "EXPLAIN QUERY PLAN";

/// Wraps `query` in `EXPLAIN QUERY PLAN`; builds the text, runs nothing.
#[must_use]
pub(crate) fn statement(query: &str) -> String {
    format!("{STATEMENT} {query}")
}
