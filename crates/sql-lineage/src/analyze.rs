//! Entry point: parse one statement and resolve its lineage against a catalog.

use crate::catalog::{Catalog, Lexical};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::Lineage;
use crate::resolve::Ctx;

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
    let mut ctx = Ctx::new(dialect, catalog);
    let analysis = ctx.statement(statement)?;
    ctx.finish(analysis)
}
