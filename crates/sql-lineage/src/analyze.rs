//! Entry point: parse one statement and resolve its lineage against a catalog.

use sqlparser::ast::Statement;

use crate::catalog::{Catalog, Lexical};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{Lineage, OutputColumn, StatementKind};
use crate::resolve::{Ctes, Ctx};

/// Analyzes the column lineage of a single SQL statement.
///
/// Every relation name is looked up in `catalog` by its written form. Names
/// the catalog lacks are reported together as [`LineageError::Missing`]; the
/// caller inserts an [`Entry`](crate::Entry) for each and calls again.
///
/// # Errors
/// Returns [`LineageError`] when the catalog reports a lexical mismatch, the
/// SQL fails to parse or holds more than one statement, a name is missing or
/// absent, a column cannot be bound, or the statement uses a construct that
/// lineage does not model (fail closed, no partial result).
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
    let Statement::Query(query) = statement else {
        return Err(LineageError::Unsupported("non-query statement".to_string()));
    };

    let mut ctx = Ctx::new(dialect, catalog);
    let result = ctx.query(query, None, &Ctes::root());
    if !ctx.missing.is_empty() && result.as_ref().map_or_else(LineageError::is_soft, |_| true) {
        return Err(LineageError::Missing(ctx.missing));
    }
    let cols = result?;

    Ok(Lineage {
        columns: cols
            .into_iter()
            .enumerate()
            .map(|(position, col)| OutputColumn {
                name: col.name,
                position,
                sources: col.sources,
            })
            .collect(),
        references: ctx.references,
        relations: ctx.relations,
        functions: ctx.functions,
        target: None,
        statement: StatementKind::Query,
    })
}
