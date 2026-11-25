//! `EXPLAIN` / `ANALYZE` statement builder for `explainQuery`.

use dbmcp_types::database;

/// Prefix of a `MariaDB` plan request that also runs the statement.
///
/// `MariaDB` replaced `EXPLAIN ANALYZE` with the `ANALYZE` statement.
const ANALYZE_MARIADB: &str = "ANALYZE FORMAT=JSON";

/// Prefix of a `MySQL` plan request that also runs the statement.
const ANALYZE: &str = "EXPLAIN ANALYZE";

/// Prefix of a plan request that only plans the statement.
const PLAN: &str = "EXPLAIN FORMAT=JSON";

/// Wraps `query` in `backend`'s plan request, running it when `analyze`; builds the text, runs nothing.
#[must_use]
pub(crate) fn statement(query: &str, analyze: bool, backend: database::Backend) -> String {
    let prefix = match (analyze, backend) {
        (true, database::Backend::Mariadb) => ANALYZE_MARIADB,
        (true, _) => ANALYZE,
        (false, _) => PLAN,
    };
    format!("{prefix} {query}")
}
