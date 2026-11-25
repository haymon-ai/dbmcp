//! Identifier quoting for server-authored `SQLite` statements.

/// Quotes `value` as a `SQLite` identifier (ANSI double-quote style).
#[must_use]
pub(crate) fn quote(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '"')
}
