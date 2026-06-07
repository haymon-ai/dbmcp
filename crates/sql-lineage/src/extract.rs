//! Entry point: parse a SQL statement and resolve its column lineage.

use sqlparser::ast::Statement;
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;

use crate::error::LineageError;
use crate::model::Lineage;
use crate::{resolve, statement};

/// Extracts column lineage from a single SQL statement.
///
/// Handles read queries (`SELECT`) and write statements (`INSERT`, `UPDATE`,
/// `DELETE`, `CREATE TABLE AS SELECT`, `MERGE`); writes set [`Lineage::target`].
/// Other statements (`SHOW`, `DESCRIBE`, `USE`, `EXPLAIN`) succeed empty.
///
/// # Errors
/// Returns [`LineageError`] if the SQL fails to parse, contains more than one
/// statement, or any projected column cannot be resolved to a concrete source
/// (fail closed — no partial result).
pub fn extract(sql: &str, dialect: &dyn Dialect) -> Result<Lineage, LineageError> {
    let statements = Parser::parse_sql(dialect, sql).map_err(|e| LineageError::Parse(e.to_string()))?;

    if statements.len() > 1 {
        return Err(LineageError::MultiStatement);
    }
    let Some(stmt) = statements.first() else {
        return Ok(Lineage::empty());
    };

    match stmt {
        Statement::Query(query) => Ok(Lineage::from_columns(resolve::resolve(query)?)),
        Statement::Insert(insert) => statement::extract_insert(insert),
        Statement::Update(update) => statement::extract_update(update),
        Statement::Delete(delete) => statement::extract_delete(delete),
        Statement::CreateTable(ct) => statement::extract_create_table(ct),
        Statement::Merge(merge) => statement::extract_merge(merge),
        _ => Ok(Lineage::empty()),
    }
}
