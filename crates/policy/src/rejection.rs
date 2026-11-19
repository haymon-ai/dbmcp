//! Why the planner refused a statement.

use dbmcp_sql_lineage::LineageError;
use thiserror::Error;

/// Most catalog rounds a backend runs before rejecting with [`Rejection::CatalogRounds`].
///
/// Each round fetches the relations one analysis reported missing; a view
/// nests one more round.
pub const CATALOG_ROUNDS: usize = 16;

/// A statement the column policy refuses to run, one variant per gate.
///
/// Messages start with `column policy:` and name only the gate and catalog
/// identifiers (columns, roles, functions, relations, views), never a rule.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Rejection {
    /// Lineage analysis failed, so the statement's columns cannot be checked.
    #[error("column policy: statement could not be analyzed ({})", .0.as_str())]
    Lineage(LineageClass),

    /// A function missing from the dialect's built-in table.
    #[error("column policy: function \"{name}\" is not a known built-in")]
    UnknownFunction {
        /// The function name, case-folded per dialect.
        name: String,
    },

    /// A relation that exposes values outside column policy.
    #[error("column policy: relation \"{relation}\" is denied")]
    DeniedRelation {
        /// The relation, as `schema.name`.
        relation: String,
    },

    /// A statement that exposes values outside column policy.
    #[error("column policy: SHOW {statement} is denied")]
    DeniedStatement {
        /// The first keyword or name after `SHOW`.
        statement: String,
    },

    /// `EXPLAIN` of a statement touching a ruled relation.
    #[error("column policy: EXPLAIN touches ruled relation \"{relation}\"")]
    Explain {
        /// The ruled relation, as `schema.name`.
        relation: String,
    },

    /// `CREATE INDEX` on a relation with a ruled column.
    #[error("column policy: CREATE INDEX on ruled relation \"{relation}\" is denied")]
    RuledIndex {
        /// The ruled relation, as `schema.name`.
        relation: String,
    },

    /// A ruled column used outside the output.
    #[error("column policy: column \"{column}\" (ruled) used in {role}{view}")]
    Reference {
        /// The column, as `schema.table.column`.
        column: String,
        /// The clause, as written in SQL.
        role: &'static str,
        /// ` in view "schema.name"` when the use sits in a view body, else empty.
        view: String,
    },

    /// An output column whose sources carry different rules.
    ///
    /// Names only the position: an unaliased output name is expression text
    /// and may hold literals.
    #[error("column policy: output column {position} combines conflicting column rules")]
    Conflict {
        /// 0-based position in the result set.
        position: usize,
    },

    /// Resolving the statement's relations took more than [`CATALOG_ROUNDS`] catalog rounds.
    #[error("column policy: catalog resolution exceeded {CATALOG_ROUNDS} rounds")]
    CatalogRounds,

    /// A write copying ruled values into a column not ruled the same way.
    #[error("column policy: writing column \"{column}\" would weaken the rules of its source columns")]
    Laundering {
        /// The target column, as `schema.table.column`.
        column: String,
    },

    /// A created column carrying ruled values under a name no rule can match.
    ///
    /// Names only the position: the column's name is expression text.
    #[error(
        "column policy: created column {position} of \"{relation}\" carries ruled values; alias the column so a rule can name it"
    )]
    Unnamed {
        /// The created relation, as `schema.name`.
        relation: String,
        /// 0-based position among the created columns.
        position: usize,
    },

    /// A bare create target that names another relation than the one created.
    ///
    /// The rules were checked against the relation the name resolved to.
    #[error("column policy: create target \"{relation}\" resolves to another relation than it creates")]
    ShadowedTarget {
        /// The relation the name resolved to, as `schema.name`.
        relation: String,
    },
}

impl Rejection {
    /// The gate that raised this rejection, for logs.
    #[must_use]
    pub const fn gate(&self) -> &'static str {
        match self {
            Self::Lineage(_) | Self::CatalogRounds => "lineage",
            Self::UnknownFunction { .. } => "function",
            Self::DeniedRelation { .. } => "relation",
            Self::DeniedStatement { .. } => "statement",
            Self::Explain { .. } => "explain",
            Self::RuledIndex { .. } => "index",
            Self::Reference { .. } => "reference",
            Self::Conflict { .. } => "combine",
            Self::Laundering { .. } | Self::Unnamed { .. } => "laundering",
            Self::ShadowedTarget { .. } => "target",
        }
    }

    /// Logs this rejection and returns it.
    #[must_use]
    pub(crate) fn logged(self) -> Self {
        tracing::info!(target: "dbmcp::policy", gate = self.gate(), "{self}");
        self
    }
}

/// The kind of lineage failure, without the names or SQL it carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineageClass {
    /// A relation could not be resolved.
    UnresolvedRelation,
    /// The SQL did not parse.
    Parse,
    /// More than one statement.
    MultiStatement,
    /// A column was missing, ambiguous, or miscounted.
    Column,
    /// A table function missing from the dialect's table.
    UnknownTableFunction,
    /// A construct lineage does not model.
    Unsupported,
    /// The server lexes SQL differently from the parser.
    LexicalMismatch,
    /// Nesting too deep, or a relation referencing itself.
    Recursion,
}

impl LineageClass {
    /// Describes the failure class in a few words.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnresolvedRelation => "unresolved relation",
            Self::Parse => "parse error",
            Self::MultiStatement => "multiple statements",
            Self::Column => "unresolved column",
            Self::UnknownTableFunction => "unknown table function",
            Self::Unsupported => "unsupported SQL",
            Self::LexicalMismatch => "server SQL lexing differs from the parser",
            Self::Recursion => "nesting or circular reference",
        }
    }
}

/// Maps a lineage error to its rejection and logs it.
///
/// With column rules active every [`LineageError`] rejects the statement;
/// the rejection keeps only the error's class.
#[must_use]
pub(crate) fn reject_lineage(error: &LineageError) -> Rejection {
    let class = match error {
        LineageError::Missing(_) | LineageError::UnknownRelation { .. } => LineageClass::UnresolvedRelation,
        LineageError::Parse(_) => LineageClass::Parse,
        LineageError::MultiStatement => LineageClass::MultiStatement,
        LineageError::ColumnNotFound { .. }
        | LineageError::AmbiguousColumn { .. }
        | LineageError::ColumnCountMismatch { .. } => LineageClass::Column,
        LineageError::UnknownTableFunction { .. } => LineageClass::UnknownTableFunction,
        LineageError::Unsupported(_) => LineageClass::Unsupported,
        LineageError::LexicalMismatch(_) => LineageClass::LexicalMismatch,
        LineageError::DepthLimit | LineageError::CircularReference { .. } => LineageClass::Recursion,
    };
    Rejection::Lineage(class).logged()
}
