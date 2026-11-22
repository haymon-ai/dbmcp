//! Top-level `ORDER BY` splicing for queries whose plan order could leak.
//!
//! Without `ORDER BY`, a query's rows arrive in the order of the plan the
//! server picks, which can follow an index on any column. [`order_by`]
//! fixes the order to output positions the caller picks. Like
//! [`pagination`](crate::pagination), the splice never re-renders the
//! statement: every byte of the caller's text is kept.

use sqlparser::ast::{
    Expr, LimitClause, Offset, OrderBy, OrderByExpr, OrderByKind, OrderByOptions, SetExpr, Spanned, Statement, Value,
};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;
use sqlparser::tokenizer::Tokenizer;

use crate::pagination::{byte_offset, has_top_level_limit_keyword, limit_keyword};

/// Splices `ORDER BY` over 1-based output `positions` into a query without one.
///
/// The clause goes before the statement's top-level `LIMIT` / `OFFSET`, or
/// is appended on a new line after stripping trailing whitespace and one
/// `;`, so a trailing `-- comment` cannot swallow it.
///
/// Returns `None` when `positions` is empty, when `sql` is not exactly one
/// query, already has a top-level `ORDER BY`, has a `FETCH`, lock, `FOR`,
/// `SETTINGS`, `FORMAT`, or pipe clause, or when the spliced text does not
/// re-parse to the original statement with only the order added.
#[must_use]
pub fn order_by(sql: &str, dialect: &dyn Dialect, positions: &[usize]) -> Option<String> {
    if positions.is_empty() {
        return None;
    }
    let Ok([Statement::Query(mut query)]) = <[Statement; 1]>::try_from(Parser::parse_sql(dialect, sql).ok()?) else {
        return None;
    };
    if query.order_by.is_some()
        || query.fetch.is_some()
        || !query.locks.is_empty()
        || query.for_clause.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
        || !query.pipe_operators.is_empty()
        || matches!(*query.body, SetExpr::Values(_))
    {
        return None;
    }
    let tokens = Tokenizer::new(dialect, sql).tokenize_with_location().ok()?;
    let list = positions.iter().map(usize::to_string).collect::<Vec<_>>().join(", ");

    let start = match &query.limit_clause {
        None if has_top_level_limit_keyword(&tokens) => return None,
        None => None,
        Some(LimitClause::LimitOffset { limit, offset, .. }) => {
            let starts = limit
                .iter()
                .map(|limit| limit.span().start)
                .chain(offset.iter().map(|Offset { value, .. }| value.span().start));
            Some(starts.min()?)
        }
        Some(LimitClause::OffsetCommaLimit { offset, .. }) => Some(offset.span().start),
    };
    let spliced = match start {
        None => {
            let trimmed = sql.trim_end();
            let body = trimmed.strip_suffix(';').unwrap_or(trimmed).trim_end();
            format!("{body}\nORDER BY {list}")
        }
        Some(start) => {
            let at = byte_offset(sql, limit_keyword(&tokens, start)?)?;
            format!("{}ORDER BY {list} {}", &sql[..at], &sql[at..])
        }
    };

    // Self-check: the spliced text must parse back to the original statement
    // with only the order added.
    query.order_by = Some(OrderBy {
        kind: OrderByKind::Expressions(
            positions
                .iter()
                .map(|&position| OrderByExpr {
                    expr: Expr::value(Value::Number(position.to_string(), false)),
                    options: OrderByOptions {
                        sort: None,
                        nulls_first: None,
                    },
                    with_fill: None,
                })
                .collect(),
        ),
        interpolate: None,
    });
    match Parser::parse_sql(dialect, &spliced) {
        Ok(reparsed) if reparsed == [Statement::Query(query)] => Some(spliced),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use sqlparser::dialect::{Dialect, MySqlDialect, PostgreSqlDialect, SQLiteDialect};

    use super::order_by;

    const ALL: &[&dyn Dialect] = &[&MySqlDialect {}, &PostgreSqlDialect {}, &SQLiteDialect {}];

    #[test]
    fn appends_after_the_statement() {
        for dialect in ALL {
            assert_eq!(
                order_by("SELECT id, name FROM users", *dialect, &[1, 2]).as_deref(),
                Some("SELECT id, name FROM users\nORDER BY 1, 2")
            );
            assert_eq!(
                order_by("SELECT id FROM users; ", *dialect, &[1]).as_deref(),
                Some("SELECT id FROM users\nORDER BY 1")
            );
            assert_eq!(
                order_by("SELECT id FROM users -- all", *dialect, &[1]).as_deref(),
                Some("SELECT id FROM users -- all\nORDER BY 1")
            );
            assert_eq!(
                order_by("SELECT id FROM a UNION SELECT id FROM b", *dialect, &[1]).as_deref(),
                Some("SELECT id FROM a UNION SELECT id FROM b\nORDER BY 1")
            );
        }
    }

    #[test]
    fn goes_before_the_limit() {
        for dialect in ALL {
            assert_eq!(
                order_by("SELECT id FROM users\nLIMIT 3 OFFSET 6", *dialect, &[1]).as_deref(),
                Some("SELECT id FROM users\nORDER BY 1 LIMIT 3 OFFSET 6")
            );
        }
        assert_eq!(
            order_by("SELECT id FROM users LIMIT 6, 3", &MySqlDialect {}, &[1]).as_deref(),
            Some("SELECT id FROM users ORDER BY 1 LIMIT 6, 3")
        );
        assert_eq!(
            order_by("SELECT id FROM users OFFSET 6 LIMIT 3", &PostgreSqlDialect {}, &[1]).as_deref(),
            Some("SELECT id FROM users ORDER BY 1 OFFSET 6 LIMIT 3")
        );
    }

    #[test]
    fn refuses_what_it_cannot_splice() {
        for dialect in ALL {
            for sql in [
                "SELECT id FROM users ORDER BY id",
                "SELECT id FROM users; SELECT 1",
                "DELETE FROM users",
                "VALUES (1)",
                "not sql",
            ] {
                assert_eq!(order_by(sql, *dialect, &[1]), None, "{sql}");
            }
            assert_eq!(order_by("SELECT id FROM users", *dialect, &[]), None);
        }
        for sql in [
            "SELECT id FROM users FETCH FIRST 3 ROWS ONLY",
            "SELECT id FROM users FOR UPDATE",
            "SELECT id FROM users LIMIT ALL",
        ] {
            assert_eq!(order_by(sql, &PostgreSqlDialect {}, &[1]), None, "{sql}");
        }
    }
}
