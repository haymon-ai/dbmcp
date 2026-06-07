//! Error type for lineage extraction.

use thiserror::Error;

/// Failure raised while extracting column lineage from SQL.
///
/// Resolution is fail-closed: any projected column that cannot be bound to a
/// concrete source aborts the whole extraction (see [`LineageError::UnresolvedColumn`]).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum LineageError {
    /// The SQL string could not be parsed by the selected dialect.
    #[error("SQL parse error: {0}")]
    Parse(String),

    /// More than one statement was supplied; only single statements are analyzed.
    #[error("multi-statement SQL is not supported")]
    MultiStatement,

    /// A projected column could not be resolved to a concrete source column.
    #[error("unresolved column `{column}`: {reason}")]
    UnresolvedColumn {
        /// The column reference that could not be resolved.
        column: String,
        /// Why it could not be resolved (e.g. ambiguous, no catalog).
        reason: String,
    },

    /// A referenced table is not present in the supplied schema catalog.
    #[error("unknown table `{table}`")]
    UnknownTable {
        /// The table name that could not be found.
        table: String,
    },

    /// Query nesting exceeded the recursion-depth guard.
    #[error("query nesting depth limit exceeded")]
    DepthLimitExceeded,

    /// A CTE references itself directly or transitively; lineage is undefined.
    #[error("circular CTE reference: `{cte}`")]
    CircularReference {
        /// The CTE name that re-entered resolution.
        cte: String,
    },

    /// A write target's column list does not match its source projection.
    #[error("column count mismatch for `{target}`: expected {expected}, found {found}")]
    ColumnCountMismatch {
        /// The write target table name.
        target: String,
        /// Number of explicit target columns.
        expected: usize,
        /// Number of source-projection columns.
        found: usize,
    },
}
