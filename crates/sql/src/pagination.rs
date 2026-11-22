//! Page window planning for server-side `SELECT` pagination.
//!
//! MCP cursor decoding and offset arithmetic live in the server crate.
//! This module owns the SQL side: [`plan`] fits a page window into a
//! caller-owned statement by splicing the statement's own top-level
//! `LIMIT` / `OFFSET` (or appending one). Shapes it cannot splice safely
//! come back as [`Page::Skip`], and the caller cuts the page client-side.
//!
//! The splice never re-renders the statement through `Display`: every
//! byte before the limit keyword is the caller's own, so column names,
//! literals, comments, and hints stay exact.

use sqlparser::ast::{Expr, LimitClause, Offset, OffsetRows, SetExpr, Spanned, Statement, Value, ValueWithSpan};
use sqlparser::dialect::Dialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Location, Token, TokenWithSpan, Tokenizer};

/// How to run one page of a caller-owned statement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Page {
    /// The statement with the page window spliced into its top-level `LIMIT`.
    Merged(String),
    /// The statement cannot be spliced; run it unchanged and cut the page.
    Skip,
}

/// Plans one page window over a caller-owned `SELECT`.
///
/// `limit` is the number of rows the page fetches (lookahead included)
/// and `offset` the number of rows to skip within the statement's own
/// window. The statement's literal `LIMIT L [OFFSET U]` (or the `MySQL`
/// form `LIMIT U, L`) becomes `LIMIT min(limit, L - offset)` with
/// `OFFSET U + offset`. Without a top-level limit, `LIMIT limit OFFSET offset`
/// is appended on a new line, after stripping trailing whitespace and one
/// `;`, so a trailing `-- comment` cannot swallow it.
///
/// Returns [`Page::Skip`] for anything that is not exactly one query, or
/// for shapes the splice cannot handle: `FETCH`, lock clauses, a
/// non-literal or out-of-range limit, `LIMIT NULL`, `LIMIT ALL`, a
/// top-level `VALUES`, and a spliced text that does not re-parse to the
/// original statement with only its limit replaced.
#[must_use]
pub fn plan(sql: &str, dialect: &dyn Dialect, limit: u64, offset: u64) -> Page {
    let Ok(statements) = Parser::parse_sql(dialect, sql) else {
        return Page::Skip;
    };
    let Ok([Statement::Query(mut query)]) = <[Statement; 1]>::try_from(statements) else {
        return Page::Skip;
    };
    if query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
        || matches!(*query.body, SetExpr::Values(_))
    {
        return Page::Skip;
    }
    let Ok(tokens) = Tokenizer::new(dialect, sql).tokenize_with_location() else {
        return Page::Skip;
    };

    // The statement's own window: skip `own_skip`, fetch `own_fetch` (`None` = no limit).
    let (own_skip, own_fetch, keyword) = match &query.limit_clause {
        None if has_top_level_limit_keyword(&tokens) => return Page::Skip,
        None => (0, None, None),
        Some(LimitClause::LimitOffset {
            limit: Some(fetch),
            offset,
            limit_by,
        }) if limit_by.is_empty() => {
            let Some(own_fetch) = literal(fetch) else {
                return Page::Skip;
            };
            let (own_skip, start) = match offset {
                None => (0, fetch.span().start),
                Some(Offset { value, .. }) => {
                    let Some(own_skip) = literal(value) else {
                        return Page::Skip;
                    };
                    (own_skip, fetch.span().start.min(value.span().start))
                }
            };
            (own_skip, Some(own_fetch), Some(start))
        }
        Some(LimitClause::OffsetCommaLimit { offset, limit: fetch }) => {
            let (Some(own_skip), Some(own_fetch)) = (literal(offset), literal(fetch)) else {
                return Page::Skip;
            };
            (own_skip, Some(own_fetch), Some(offset.span().start))
        }
        Some(LimitClause::LimitOffset { .. }) => return Page::Skip,
    };

    let Some(skip) = own_skip.checked_add(offset).filter(|&skip| i64::try_from(skip).is_ok()) else {
        return Page::Skip;
    };
    let fetch = own_fetch.map_or(limit, |own_fetch| limit.min(own_fetch.saturating_sub(offset)));
    if i64::try_from(fetch).is_err() {
        return Page::Skip;
    }

    let merged = match keyword {
        None => {
            let trimmed = sql.trim_end();
            let body = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
            format!("{body}\nLIMIT {fetch} OFFSET {skip}")
        }
        Some(start) => {
            let Some(at) = limit_keyword(&tokens, start).and_then(|location| byte_offset(sql, location)) else {
                return Page::Skip;
            };
            format!("{}LIMIT {fetch} OFFSET {skip}", &sql[..at])
        }
    };

    // Self-check: the spliced text must parse back to the original statement
    // with only its limit replaced.
    query.limit_clause = Some(LimitClause::LimitOffset {
        limit: Some(number(fetch)),
        offset: Some(Offset {
            value: number(skip),
            rows: OffsetRows::None,
        }),
        limit_by: Vec::new(),
    });
    match Parser::parse_sql(dialect, &merged) {
        Ok(reparsed) if reparsed == [Statement::Query(query)] => Page::Merged(merged),
        _ => Page::Skip,
    }
}

/// Reads a plain decimal-digit integer literal that fits in `i64`.
fn literal(expr: &Expr) -> Option<u64> {
    let Expr::Value(ValueWithSpan {
        value: Value::Number(digits, false),
        ..
    }) = expr
    else {
        return None;
    };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse::<u64>().ok().filter(|&value| i64::try_from(value).is_ok())
}

/// Builds an integer literal expression.
fn number(value: u64) -> Expr {
    Expr::value(Value::Number(value.to_string(), false))
}

/// Reports an unquoted `LIMIT`, `OFFSET`, or `FETCH` word outside every parenthesis.
///
/// Catches limit text the AST drops, such as a bare `LIMIT ALL` on `PostgreSQL`.
pub(crate) fn has_top_level_limit_keyword(tokens: &[TokenWithSpan]) -> bool {
    let mut depth = 0_usize;
    for TokenWithSpan { token, .. } in tokens {
        match token {
            Token::LParen => depth += 1,
            Token::RParen => depth = depth.saturating_sub(1),
            Token::Word(word)
                if depth == 0
                    && word.quote_style.is_none()
                    && matches!(word.keyword, Keyword::LIMIT | Keyword::OFFSET | Keyword::FETCH) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Finds the `LIMIT` / `OFFSET` keyword that opens a limit clause.
///
/// `start` is the earliest limit expression start; the keyword is the last
/// token before it that is not whitespace or a comment.
pub(crate) fn limit_keyword(tokens: &[TokenWithSpan], start: Location) -> Option<Location> {
    let TokenWithSpan { token, span } = tokens
        .iter()
        .take_while(|token| token.span.start < start)
        .filter(|token| !matches!(token.token, Token::Whitespace(_)))
        .last()?;
    match token {
        Token::Word(word) if matches!(word.keyword, Keyword::LIMIT | Keyword::OFFSET) => Some(span.start),
        _ => None,
    }
}

/// Converts a tokenizer [`Location`] (1-based line and char column) into a byte offset.
pub(crate) fn byte_offset(sql: &str, location: Location) -> Option<usize> {
    let (mut line, mut column) = (1, 1);
    for (at, ch) in sql.char_indices() {
        if line == location.line && column == location.column {
            return Some(at);
        }
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
    use sqlparser::parser::Parser;

    use super::{Page, plan};

    const MYSQL: &dyn Dialect = &MySqlDialect {};
    const POSTGRES: &dyn Dialect = &PostgreSqlDialect {};
    const SQLITE: &dyn Dialect = &SQLiteDialect {};

    const ALL: &[&dyn Dialect] = &[MYSQL, POSTGRES, SQLITE];
    const COMMA: &[&dyn Dialect] = &[MYSQL, SQLITE];
    const PG: &[&dyn Dialect] = &[POSTGRES];
    const MY: &[&dyn Dialect] = &[MYSQL];
    const MY_PG: &[&dyn Dialect] = &[MYSQL, POSTGRES];
    const PG_LITE: &[&dyn Dialect] = &[POSTGRES, SQLITE];

    /// `(dialects, sql, limit, offset, expected)`.
    type Merge = (&'static [&'static dyn Dialect], &'static str, u64, u64, &'static str);

    /// `(dialects, sql, limit, offset)`.
    type Skip = (&'static [&'static dyn Dialect], &'static str, u64, u64);

    const MERGES: &[Merge] = &[
        // No top-level limit: append on a new line.
        (
            ALL,
            "SELECT id FROM users ORDER BY id",
            3,
            0,
            "SELECT id FROM users ORDER BY id\nLIMIT 3 OFFSET 0",
        ),
        (ALL, "SELECT 1", 11, 50, "SELECT 1\nLIMIT 11 OFFSET 50"),
        // Trailing `;` and whitespace are stripped before appending.
        (ALL, "SELECT 1;", 2, 0, "SELECT 1\nLIMIT 2 OFFSET 0"),
        (ALL, "SELECT 1 ;", 2, 0, "SELECT 1\nLIMIT 2 OFFSET 0"),
        (ALL, "SELECT 1;   ", 2, 0, "SELECT 1\nLIMIT 2 OFFSET 0"),
        (ALL, "SELECT 1  ", 2, 0, "SELECT 1\nLIMIT 2 OFFSET 0"),
        // A trailing line comment ends at the appended newline.
        (
            ALL,
            "SELECT 1 -- count rows",
            6,
            0,
            "SELECT 1 -- count rows\nLIMIT 6 OFFSET 0",
        ),
        // Own LIMIT, page inside the window.
        (
            ALL,
            "SELECT id FROM t ORDER BY id LIMIT 10",
            3,
            2,
            "SELECT id FROM t ORDER BY id LIMIT 3 OFFSET 2",
        ),
        (
            ALL,
            "SELECT id FROM t LIMIT 10 OFFSET 5",
            3,
            2,
            "SELECT id FROM t LIMIT 3 OFFSET 7",
        ),
        // Own LIMIT smaller than the page (#304): the window is unchanged.
        (
            ALL,
            "SELECT id FROM t LIMIT 2",
            3,
            0,
            "SELECT id FROM t LIMIT 2 OFFSET 0",
        ),
        // Page straddles the end of the window.
        (
            ALL,
            "SELECT id FROM t LIMIT 10",
            3,
            8,
            "SELECT id FROM t LIMIT 2 OFFSET 8",
        ),
        // Page past the end of the window.
        (
            ALL,
            "SELECT id FROM t LIMIT 10",
            3,
            10,
            "SELECT id FROM t LIMIT 0 OFFSET 10",
        ),
        (
            ALL,
            "SELECT id FROM t LIMIT 10",
            3,
            12,
            "SELECT id FROM t LIMIT 0 OFFSET 12",
        ),
        (
            ALL,
            "SELECT id FROM t LIMIT 0 OFFSET 4",
            3,
            0,
            "SELECT id FROM t LIMIT 0 OFFSET 4",
        ),
        // MySQL `LIMIT U, L`.
        (
            COMMA,
            "SELECT id FROM t LIMIT 5, 10",
            3,
            2,
            "SELECT id FROM t LIMIT 3 OFFSET 7",
        ),
        // `OFFSET` before `LIMIT`, and `OFFSET … ROWS`.
        (
            PG,
            "SELECT id FROM t OFFSET 5 LIMIT 10",
            3,
            2,
            "SELECT id FROM t LIMIT 3 OFFSET 7",
        ),
        (
            PG,
            "SELECT id FROM t LIMIT 10 OFFSET 5 ROWS",
            3,
            0,
            "SELECT id FROM t LIMIT 3 OFFSET 5",
        ),
        // Comments inside and after the limit clause.
        (
            ALL,
            "SELECT id FROM t LIMIT /* c */ 10",
            3,
            0,
            "SELECT id FROM t LIMIT 3 OFFSET 0",
        ),
        (
            ALL,
            "SELECT id FROM t LIMIT 10; -- tail",
            3,
            0,
            "SELECT id FROM t LIMIT 3 OFFSET 0",
        ),
        (
            ALL,
            "SELECT id FROM t LIMIT 10 -- tail",
            3,
            0,
            "SELECT id FROM t LIMIT 3 OFFSET 0",
        ),
        // Multibyte text and CRLF before the limit keep their bytes.
        (
            ALL,
            "SELECT 'é日本' AS x\r\nFROM t\r\nLIMIT 10",
            3,
            1,
            "SELECT 'é日本' AS x\r\nFROM t\r\nLIMIT 3 OFFSET 1",
        ),
        // Limit only in a subquery or CTE: append.
        (
            ALL,
            "SELECT * FROM (SELECT id FROM t LIMIT 5) AS s",
            3,
            0,
            "SELECT * FROM (SELECT id FROM t LIMIT 5) AS s\nLIMIT 3 OFFSET 0",
        ),
        (
            ALL,
            "WITH c AS (SELECT id FROM t LIMIT 9) SELECT id FROM c",
            3,
            1,
            "WITH c AS (SELECT id FROM t LIMIT 9) SELECT id FROM c\nLIMIT 3 OFFSET 1",
        ),
        (
            ALL,
            "WITH c AS (SELECT id FROM t LIMIT 9) SELECT id FROM c LIMIT 4",
            3,
            2,
            "WITH c AS (SELECT id FROM t LIMIT 9) SELECT id FROM c LIMIT 2 OFFSET 2",
        ),
        // Top-level UNION without and with its own limit.
        (
            ALL,
            "SELECT id FROM a UNION SELECT id FROM b ORDER BY id",
            3,
            0,
            "SELECT id FROM a UNION SELECT id FROM b ORDER BY id\nLIMIT 3 OFFSET 0",
        ),
        (
            ALL,
            "SELECT id FROM a UNION ALL SELECT id FROM b ORDER BY id LIMIT 10 OFFSET 1",
            3,
            3,
            "SELECT id FROM a UNION ALL SELECT id FROM b ORDER BY id LIMIT 3 OFFSET 4",
        ),
    ];

    #[test]
    fn merges() {
        for &(dialects, sql, limit, offset, expected) in MERGES {
            for &dialect in dialects {
                let page = plan(sql, dialect, limit, offset);
                assert_eq!(
                    page,
                    Page::Merged(expected.to_owned()),
                    "input = {sql:?}, dialect = {dialect:?}",
                );
                let statements = Parser::parse_sql(dialect, expected)
                    .unwrap_or_else(|e| panic!("input = {sql:?}, dialect = {dialect:?}: {e}"));
                assert_eq!(statements.len(), 1, "input = {sql:?}, dialect = {dialect:?}");
            }
        }
    }

    const SKIPS: &[Skip] = &[
        (ALL, "SELECT id FROM t FETCH FIRST 5 ROWS ONLY", 3, 0),
        (
            ALL,
            "SELECT id FROM t ORDER BY id OFFSET 1 ROWS FETCH NEXT 5 ROWS ONLY",
            3,
            0,
        ),
        (MY_PG, "SELECT id FROM t FOR UPDATE", 3, 0),
        (MY_PG, "SELECT id FROM t FOR SHARE", 3, 0),
        (MY_PG, "SELECT id FROM t LIMIT 5 FOR UPDATE", 3, 0),
        (COMMA, "SELECT id FROM t LIMIT ?", 3, 0),
        (PG, "SELECT id FROM t LIMIT $1", 3, 0),
        (ALL, "SELECT id FROM t LIMIT (1+1)", 3, 0),
        (ALL, "SELECT id FROM t LIMIT 1+1", 3, 0),
        (ALL, "SELECT id FROM t LIMIT (SELECT 5)", 3, 0),
        (ALL, "SELECT id FROM t LIMIT 5 OFFSET (SELECT 1)", 3, 0),
        (ALL, "SELECT id FROM t LIMIT NULL", 3, 0),
        (ALL, "SELECT id FROM t LIMIT -1", 3, 0),
        (ALL, "SELECT id FROM t LIMIT 5.5", 3, 0),
        (ALL, "SELECT id FROM t LIMIT 1e2", 3, 0),
        (PG, "SELECT id FROM t LIMIT ALL", 3, 0),
        (PG, "SELECT id FROM t LIMIT ALL OFFSET 5", 3, 0),
        (PG, "SELECT id FROM t OFFSET 5", 3, 0),
        // Out of `u64`, and in `u64` but out of `i64`.
        (ALL, "SELECT id FROM t LIMIT 18446744073709551616", 3, 0),
        (ALL, "SELECT id FROM t LIMIT 9223372036854775808", 3, 0),
        // The merged offset would leave `i64`.
        (ALL, "SELECT id FROM t LIMIT 5 OFFSET 9223372036854775807", 3, 1),
        (ALL, "SELECT id FROM t", 3, u64::MAX),
        // Top-level `VALUES`.
        (PG_LITE, "VALUES (1), (2)", 3, 0),
        (MY, "VALUES ROW(1), ROW(2)", 3, 0),
        // A comment after the `;` keeps the `;`, so the append re-parses
        // as two statements.
        (ALL, "SELECT 1; -- tail", 3, 0),
        // Not exactly one query.
        (ALL, "SELECT 1; SELECT 2", 3, 0),
        (MY, "SHOW TABLES", 3, 0),
        (ALL, "INSERT INTO t (id) VALUES (1)", 3, 0),
        (ALL, "SELECT (", 3, 0),
    ];

    #[test]
    fn skips() {
        for &(dialects, sql, limit, offset) in SKIPS {
            for &dialect in dialects {
                assert_eq!(
                    plan(sql, dialect, limit, offset),
                    Page::Skip,
                    "input = {sql:?}, dialect = {dialect:?}",
                );
            }
        }
    }
}
