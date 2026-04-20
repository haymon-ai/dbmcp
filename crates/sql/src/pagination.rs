//! SQL string helpers for server-side pagination.
//!
//! MCP cursor decoding and offset arithmetic live in the server crate;
//! this module only owns the SQL rewrite that appends a `LIMIT` /
//! `OFFSET` to a caller-owned `SELECT` by wrapping it as a subquery.

/// Wraps a caller-owned `SELECT` as a subquery with `LIMIT` / `OFFSET` appended.
///
/// Strips a single trailing `;` and surrounding whitespace from `sql`
/// before wrapping so the emitted statement remains syntactically valid
/// on every supported backend. The closing parenthesis is placed on a
/// new line so a trailing `-- line comment` in the caller's SQL cannot
/// swallow the wrap's own tokens.
#[must_use]
pub fn with_limit_offset(sql: &str, limit: usize, offset: u64) -> String {
    let trimmed = sql.trim_end();
    let inner = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
    format!("SELECT * FROM ({inner}\n) AS paginated LIMIT {limit} OFFSET {offset}")
}

#[cfg(test)]
mod tests {
    use super::with_limit_offset;

    #[test]
    fn injects_limit_offset_and_alias() {
        assert_eq!(
            with_limit_offset("SELECT id FROM users ORDER BY id", 3, 0),
            "SELECT * FROM (SELECT id FROM users ORDER BY id\n) AS paginated LIMIT 3 OFFSET 0",
        );
    }

    #[test]
    fn emits_requested_limit_and_offset() {
        let wrapped = with_limit_offset("SELECT 1", 11, 50);
        assert!(wrapped.contains(" LIMIT 11 OFFSET 50"), "got: {wrapped}");
    }

    #[test]
    fn strips_trailing_semicolon_and_whitespace() {
        // Each input variant must produce the same wrapped SQL.
        let expected = "SELECT * FROM (SELECT 1\n) AS paginated LIMIT 2 OFFSET 0";
        for input in ["SELECT 1", "SELECT 1;", "SELECT 1 ;", "SELECT 1;   ", "SELECT 1  "] {
            assert_eq!(with_limit_offset(input, 2, 0), expected, "input = {input:?}");
        }
    }

    #[test]
    fn survives_trailing_line_comment() {
        // A trailing `--` line comment must not swallow the wrap's alias or
        // LIMIT/OFFSET tokens. The wrap puts `)` on its own line so the
        // comment terminates at the newline.
        assert_eq!(
            with_limit_offset("SELECT 1 -- count rows", 6, 0),
            "SELECT * FROM (SELECT 1 -- count rows\n) AS paginated LIMIT 6 OFFSET 0",
        );
    }
}
