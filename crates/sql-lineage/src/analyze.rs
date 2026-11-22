//! Entry point: parse one statement and resolve its lineage against a catalog.

use crate::catalog::{Catalog, Lexical};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::Lineage;
use crate::resolve::{Ctx, unsupported};
use sqlparser::tokenizer::{Token, Whitespace};

/// Analyzes the column lineage of a single SQL statement.
///
/// Every relation name is looked up in `catalog` by its written form. Names
/// the catalog lacks are reported together as [`LineageError::Missing`]; the
/// caller inserts an [`Entry`](crate::Entry) for each and calls again.
///
/// # Errors
/// Returns [`LineageError`] when the catalog reports a lexical mismatch, the
/// SQL fails to parse or holds more than one statement, a name is missing or
/// absent, a column cannot be bound, or the statement (or a construct in it)
/// is outside the modeled tiers (fail closed, no partial result).
pub fn analyze(sql: &str, dialect: Dialect, catalog: &Catalog) -> Result<Lineage, LineageError> {
    if let Lexical::Mismatch(reason) = catalog.lexical() {
        return Err(LineageError::LexicalMismatch(reason.clone()));
    }
    let statements = dialect.parse(sql).map_err(|e| LineageError::Parse(e.to_string()))?;
    let statement = match statements.as_slice() {
        [statement] => statement,
        [] => return Err(LineageError::Parse("no statement".to_string())),
        _ => return Err(LineageError::MultiStatement),
    };
    if optimizer_hint(sql, dialect)? {
        return Err(unsupported("optimizer hint comment"));
    }
    let mut ctx = Ctx::new(sql, dialect, catalog);
    let analysis = ctx.statement(statement)?;
    ctx.finish(analysis)
}

/// Returns whether `sql` holds an optimizer-hint comment anywhere.
///
/// A hint is a comment whose text starts with `[ASCII alphanumeric]*+`, the
/// form sqlparser reads as one. `pg_hint_plan` reads the first block comment
/// wherever it sits, so every comment counts, not only those in the AST.
fn optimizer_hint(sql: &str, dialect: Dialect) -> Result<bool, LineageError> {
    let tokens = dialect.tokenize(sql).map_err(|e| LineageError::Parse(e.to_string()))?;
    Ok(
        tokens.iter().any(|token| {
            let Token::Whitespace(
                Whitespace::MultiLineComment(text) | Whitespace::SingleLineComment { comment: text, .. },
            ) = token
            else {
                return false;
            };
            text.split_once('+')
                .is_some_and(|(prefix, _)| prefix.bytes().all(|b| b.is_ascii_alphanumeric()))
        }),
    )
}
