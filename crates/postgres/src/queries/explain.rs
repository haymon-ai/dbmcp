//! `EXPLAIN (FORMAT JSON)` statement builder for `explainQuery`.

/// Prefix of a plan request that also runs the statement.
const ANALYZE: &str = "EXPLAIN (ANALYZE, FORMAT JSON)";

/// Prefix of a plan request that only plans the statement.
const PLAN: &str = "EXPLAIN (FORMAT JSON)";

/// Wraps `query` in `EXPLAIN`, with `ANALYZE` when `analyze`; builds the text, runs nothing.
#[must_use]
pub(crate) fn statement(query: &str, analyze: bool) -> String {
    let prefix = if analyze { ANALYZE } else { PLAN };
    format!("{prefix} {query}")
}
