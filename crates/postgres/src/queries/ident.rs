//! Identifier quoting for server-authored `PostgreSQL` statements.

/// Quotes `value` as a `PostgreSQL` identifier (ANSI double-quote style).
#[must_use]
pub(crate) fn quote(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '"')
}
