//! SQL quoting and validation for identifiers and literals.

use database_mcp_server::AppError;
use sqlparser::dialect::Dialect;

/// Wraps `value` in the dialect's identifier quote character.
///
/// Derives the quote character from [`Dialect::identifier_quote_style`],
/// falling back to `"` (ANSI double-quote) when the dialect returns `None`.
/// Escapes internal occurrences of the quote character by doubling them.
#[must_use]
pub fn quote_ident(value: &str, dialect: &impl Dialect) -> String {
    let q = dialect.identifier_quote_style(value).unwrap_or('"');
    let mut out = String::with_capacity(value.len() + 2);
    out.push(q);
    for ch in value.chars() {
        if ch == q {
            out.push(q);
        }
        out.push(ch);
    }
    out.push(q);
    out
}

/// Wraps `value` in single quotes for use as a SQL string literal.
///
/// Escapes backslashes and single quotes by doubling them. Backslash
/// doubling is required for safety under `MySQL`'s default SQL mode,
/// which treats `\` as an escape character inside string literals.
#[must_use]
pub fn quote_literal(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('\'');
    for ch in value.chars() {
        if ch == '\\' {
            out.push('\\');
        } else if ch == '\'' {
            out.push('\'');
        }
        out.push(ch);
    }
    out.push('\'');
    out
}

/// Validates that `name` is a non-empty identifier without control characters.
///
/// # Errors
///
/// Returns [`AppError::InvalidIdentifier`] if the name is empty,
/// whitespace-only, or contains control characters.
pub fn validate_ident(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty() || name.chars().any(char::is_control) {
        return Err(AppError::InvalidIdentifier(name.to_string()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};

    use super::*;

    #[test]
    fn accepts_standard_names() {
        assert!(validate_ident("users").is_ok());
        assert!(validate_ident("my_table").is_ok());
        assert!(validate_ident("DB_123").is_ok());
    }

    #[test]
    fn accepts_hyphenated_names() {
        assert!(validate_ident("eu-docker").is_ok());
        assert!(validate_ident("access-logs").is_ok());
    }

    #[test]
    fn accepts_special_chars() {
        assert!(validate_ident("my.db").is_ok());
        assert!(validate_ident("123db").is_ok());
        assert!(validate_ident("café").is_ok());
        assert!(validate_ident("a b").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert!(validate_ident("").is_err());
    }

    #[test]
    fn rejects_whitespace_only() {
        assert!(validate_ident("   ").is_err());
        assert!(validate_ident("\t").is_err());
    }

    #[test]
    fn rejects_control_chars() {
        assert!(validate_ident("test\x00db").is_err());
        assert!(validate_ident("test\ndb").is_err());
        assert!(validate_ident("test\x1Fdb").is_err());
    }

    #[test]
    fn quote_with_postgres_dialect() {
        let d = PostgreSqlDialect {};
        assert_eq!(quote_ident("users", &d), "\"users\"");
        assert_eq!(quote_ident("eu-docker", &d), "\"eu-docker\"");
        assert_eq!(quote_ident("test\"db", &d), "\"test\"\"db\"");
    }

    #[test]
    fn quote_with_mysql_dialect() {
        let d = MySqlDialect {};
        assert_eq!(quote_ident("users", &d), "`users`");
        assert_eq!(quote_ident("test`db", &d), "`test``db`");
    }

    #[test]
    fn quote_with_sqlite_dialect() {
        let d = SQLiteDialect {};
        assert_eq!(quote_ident("users", &d), "`users`");
        assert_eq!(quote_ident("test`db", &d), "`test``db`");
    }

    #[test]
    fn quote_literal_escapes_single_quotes() {
        assert_eq!(quote_literal("my_db"), "'my_db'");
        assert_eq!(quote_literal(""), "''");
        assert_eq!(quote_literal("it's"), "'it''s'");
        assert_eq!(quote_literal("a'b'c"), "'a''b''c'");
    }

    // === T006: validate_ident boundary tests ===

    #[test]
    fn accepts_long_identifier() {
        let long_name: String = "a".repeat(10_000);
        assert!(validate_ident(&long_name).is_ok());
    }

    #[test]
    fn rejects_mixed_valid_and_control() {
        assert!(validate_ident("valid\x00").is_err());
        assert!(validate_ident("\x01start").is_err());
        assert!(validate_ident("mid\x7Fdle").is_err());
    }

    #[test]
    fn accepts_sql_injection_payload_in_ident() {
        assert!(validate_ident("Robert'; DROP TABLE students;--").is_ok());
    }

    #[test]
    fn accepts_emoji() {
        assert!(validate_ident("🎉").is_ok());
        assert!(validate_ident("table_🔥").is_ok());
    }

    #[test]
    fn accepts_cjk() {
        assert!(validate_ident("数据库").is_ok());
        assert!(validate_ident("テーブル").is_ok());
    }

    // === T007: quote_ident adversarial tests ===

    #[test]
    fn quote_ident_only_backticks_mysql() {
        let d = MySqlDialect {};
        // Input: `` (2 backticks). Each doubled → 4, plus wrapping → 6.
        assert_eq!(quote_ident("``", &d), "``````");
    }

    #[test]
    fn quote_ident_only_double_quotes_postgres() {
        let d = PostgreSqlDialect {};
        // Input: "" (2 double-quotes). Each doubled → 4, plus wrapping → 6.
        assert_eq!(quote_ident("\"\"", &d), "\"\"\"\"\"\"");
    }

    #[test]
    fn quote_ident_quote_at_start_and_end() {
        let mysql = MySqlDialect {};
        // Input: `x` (3 chars). Backticks doubled → ``x`` plus wrapping → 7.
        assert_eq!(quote_ident("`x`", &mysql), "```x```");

        let pg = PostgreSqlDialect {};
        assert_eq!(quote_ident("\"x\"", &pg), "\"\"\"x\"\"\"");
    }

    #[test]
    fn quote_ident_cross_dialect_foreign_quote_passes_through() {
        let mysql = MySqlDialect {};
        assert_eq!(quote_ident("test\"db", &mysql), "`test\"db`");

        let pg = PostgreSqlDialect {};
        assert_eq!(quote_ident("test`db", &pg), "\"test`db\"");
    }

    #[test]
    fn quote_ident_empty_string() {
        let mysql = MySqlDialect {};
        assert_eq!(quote_ident("", &mysql), "``");

        let pg = PostgreSqlDialect {};
        assert_eq!(quote_ident("", &pg), "\"\"");
    }

    #[test]
    fn quote_ident_long_string_completes() {
        let long_name: String = "a".repeat(10_000);
        let pg = PostgreSqlDialect {};
        let quoted = quote_ident(&long_name, &pg);
        assert_eq!(quoted.len(), 10_002);
    }

    // === T008: quote_literal backslash tests ===

    #[test]
    fn quote_literal_trailing_backslash() {
        assert_eq!(quote_literal("test\\"), "'test\\\\'");
    }

    #[test]
    fn quote_literal_single_backslash() {
        assert_eq!(quote_literal("\\"), "'\\\\'");
    }

    #[test]
    fn quote_literal_backslash_then_quote() {
        // Input: \' (2 chars). \ doubled → \\, ' doubled → ''. Wrapped: '\\'''
        assert_eq!(quote_literal("\\'"), "'\\\\'''");
    }

    #[test]
    fn quote_literal_only_backslashes() {
        assert_eq!(quote_literal("\\\\\\"), "'\\\\\\\\\\\\'");
    }

    #[test]
    fn quote_literal_sql_injection_payload() {
        assert_eq!(
            quote_literal("Robert'; DROP TABLE students;--"),
            "'Robert''; DROP TABLE students;--'"
        );
    }

    #[test]
    fn quote_literal_many_quotes_completes() {
        let input: String = "'".repeat(1_000);
        let result = quote_literal(&input);
        assert_eq!(result.len(), 2_002);
    }

    // === T009: quote_literal combined edge cases ===

    #[test]
    fn quote_literal_backslash_and_quotes_mixed() {
        // Input: it\'s (4 chars). \ doubled, ' doubled. Wrapped: 'it\\''s'
        assert_eq!(quote_literal("it\\'s"), "'it\\\\''s'");
    }

    #[test]
    fn quote_literal_no_special_chars() {
        assert_eq!(quote_literal("plain"), "'plain'");
    }

    #[test]
    fn quote_literal_unicode_untouched() {
        assert_eq!(quote_literal("café"), "'café'");
        assert_eq!(quote_literal("数据"), "'数据'");
    }
}
