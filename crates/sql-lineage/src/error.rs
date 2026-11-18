//! Error type for lineage analysis.

use thiserror::Error;

use crate::catalog::WrittenName;

/// Failure raised while analyzing column lineage from SQL.
///
/// Analysis is fail-closed: any name, column, or construct that cannot be
/// bound to a concrete source aborts the whole analysis.
/// [`LineageError::Missing`] is not a failure but the catalog-loop signal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LineageError {
    /// Relation names absent from the catalog snapshot; insert them and call again.
    #[error("relations missing from the catalog: {}", join(.0))]
    Missing(Vec<WrittenName>),

    /// The SQL string could not be parsed by the selected dialect.
    #[error("SQL parse error: {0}")]
    Parse(String),

    /// More than one statement was supplied; only single statements are analyzed.
    #[error("multi-statement SQL is not supported")]
    MultiStatement,

    /// A relation the catalog marks as absent.
    #[error("unknown relation `{name}`")]
    UnknownRelation {
        /// The relation name as written.
        name: String,
    },

    /// A column reference that no visible relation provides.
    #[error("column `{column}` not found")]
    ColumnNotFound {
        /// The column reference as written.
        column: String,
    },

    /// A column reference that more than one visible relation provides.
    #[error("column reference `{column}` is ambiguous")]
    AmbiguousColumn {
        /// The column reference as written.
        column: String,
    },

    /// A table function in `FROM` that the dialect's function table lacks.
    #[error("unknown table function `{name}`")]
    UnknownTableFunction {
        /// The function name as written.
        name: String,
    },

    /// A statement, clause, or expression kind that lineage does not model.
    #[error("unsupported SQL: {0}")]
    Unsupported(String),

    /// The server tokenizes SQL differently from the parser.
    #[error("server SQL lexing differs from the parser: {0}")]
    LexicalMismatch(String),

    /// Nesting exceeded the depth guard.
    #[error("nesting depth limit exceeded")]
    DepthLimit,

    /// A relation references itself directly or transitively.
    #[error("circular reference: `{name}`")]
    CircularReference {
        /// The relation that re-entered resolution.
        name: String,
    },

    /// A column list does not match the columns it names.
    #[error("column count mismatch for `{relation}`: expected {expected}, found {found}")]
    ColumnCountMismatch {
        /// The relation whose columns are being named.
        relation: String,
        /// Number of columns available: an upper bound for alias lists, exact for a view body.
        expected: usize,
        /// Number of columns supplied.
        found: usize,
    },
}

fn join(names: &[WrittenName]) -> String {
    names.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
