//! Identifier-safety primitives shared across SQL backends.

/// Quotes `value` as a SQL identifier using `quote`.
///
/// Wraps the value in `quote` and doubles every internal occurrence of `quote`.
#[must_use]
pub fn quote_ident(value: &str, quote: char) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push(quote);
    for ch in value.chars() {
        if ch == quote {
            out.push(quote);
        }
        out.push(ch);
    }
    out.push(quote);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quote_ident_basic_double_quote() {
        assert_eq!(quote_ident("users", '"'), "\"users\"");
        assert_eq!(quote_ident("eu-docker", '"'), "\"eu-docker\"");
        assert_eq!(quote_ident("test\"db", '"'), "\"test\"\"db\"");
    }

    #[test]
    fn quote_ident_basic_backtick() {
        assert_eq!(quote_ident("users", '`'), "`users`");
        assert_eq!(quote_ident("eu-docker", '`'), "`eu-docker`");
        assert_eq!(quote_ident("test`db", '`'), "`test``db`");
    }

    #[test]
    fn quote_ident_only_quote_chars() {
        // Input: "" (2 double-quotes). Each doubled → 4, plus wrapping → 6.
        assert_eq!(quote_ident("\"\"", '"'), "\"\"\"\"\"\"");
        // Input: `` (2 backticks). Each doubled → 4, plus wrapping → 6.
        assert_eq!(quote_ident("``", '`'), "``````");
    }

    #[test]
    fn quote_ident_quote_at_start_and_end() {
        assert_eq!(quote_ident("\"x\"", '"'), "\"\"\"x\"\"\"");
        assert_eq!(quote_ident("`x`", '`'), "```x```");
    }

    #[test]
    fn quote_ident_foreign_quote_passes_through() {
        // Backtick is foreign to ANSI quoting; double-quote is foreign to MySQL.
        assert_eq!(quote_ident("test`db", '"'), "\"test`db\"");
        assert_eq!(quote_ident("test\"db", '`'), "`test\"db`");
    }

    #[test]
    fn quote_ident_empty_string() {
        assert_eq!(quote_ident("", '"'), "\"\"");
        assert_eq!(quote_ident("", '`'), "``");
    }

    #[test]
    fn quote_ident_long_string_completes() {
        let long_name: String = "a".repeat(10_000);
        let quoted = quote_ident(&long_name, '"');
        assert_eq!(quoted.len(), 10_002);
    }

    #[test]
    fn quote_ident_unicode_untouched() {
        assert_eq!(quote_ident("数据", '"'), "\"数据\"");
        assert_eq!(quote_ident("café", '`'), "`café`");
    }

    #[test]
    fn quote_ident_dot_kept_inside_quotes() {
        assert_eq!(quote_ident("schema.table", '"'), "\"schema.table\"");
        assert_eq!(quote_ident("schema.table", '`'), "`schema.table`");
    }
}
