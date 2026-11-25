//! Identifier quoting for server-authored `MySQL` statements.

/// Quotes `value` as a `MySQL` identifier (backtick-wrapped).
#[must_use]
pub(crate) fn quote(value: &str) -> String {
    dbmcp_sql::sanitize::quote_ident(value, '`')
}
