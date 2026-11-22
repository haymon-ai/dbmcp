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
/// Identifiers render as Rust string literals (`{:?}`): quoted, with control,
/// invisible, and bidirectional characters, quotes, and backslashes escaped.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum Rejection {
    /// Lineage analysis failed, so the statement's columns cannot be checked.
    #[error("column policy: statement could not be analyzed ({})", .0.as_str())]
    Lineage(LineageClass),

    /// A function missing from the dialect's built-in table.
    #[error("column policy: function {name:?} is not a known built-in")]
    UnknownFunction {
        /// The function name, case-folded per dialect.
        name: String,
    },

    /// A relation that exposes values outside column policy.
    #[error("column policy: relation {relation:?} is denied")]
    DeniedRelation {
        /// The relation as written, or as `schema.name` when resolved.
        relation: String,
    },

    /// A statement that exposes values outside column policy.
    #[error("column policy: this SHOW form is denied")]
    DeniedStatement,

    /// `EXPLAIN` touching a ruled relation, other than a plain JSON or query-plan form.
    #[error("column policy: EXPLAIN touches ruled relation {relation:?}; use explainQuery")]
    Explain {
        /// The ruled relation, as `schema.name`.
        relation: String,
    },

    /// `EXPLAIN ANALYZE` of a statement touching a ruled relation.
    #[error(
        "column policy: EXPLAIN ANALYZE on ruled relation {relation:?} is denied; actual row counts can reveal values, explain without ANALYZE"
    )]
    ExplainAnalyze {
        /// The ruled relation, as `schema.name`.
        relation: String,
    },

    /// `CREATE INDEX` on a relation with a ruled column.
    #[error("column policy: CREATE INDEX on ruled relation {relation:?} is denied")]
    RuledIndex {
        /// The ruled relation, as `schema.name`.
        relation: String,
    },

    /// A ruled column used outside the output.
    #[error(
        "column policy: column {column:?} (ruled) used in {role}{}",
        view.as_ref().map(|view| format!(" in view {view:?}")).unwrap_or_default()
    )]
    Reference {
        /// The column, as `schema.table.column`.
        column: String,
        /// The clause, as written in SQL.
        role: &'static str,
        /// The view, as `schema.name`, when the use sits in a view body.
        view: Option<String>,
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
    #[error("column policy: writing column {column:?} would weaken the rules of its source columns")]
    Laundering {
        /// The target column, as `schema.table.column`.
        column: String,
    },

    /// A write choosing the value of a `hash`-ruled column.
    ///
    /// Its token would sign a value the statement picked.
    #[error("column policy: column {column:?} is hashed and only takes copies of columns hashed the same way, or NULL")]
    Chosen {
        /// The target column, as `schema.table.column`.
        column: String,
    },

    /// A write letting the server fill a `hash`-ruled column.
    ///
    /// A default, generated, or on-update value may be one the statement
    /// steered, and its token would sign it.
    #[error("column policy: column {column:?} is hashed and filled by the database, so this write is refused")]
    Computed {
        /// The target column, as `schema.table.column`.
        column: String,
    },

    /// A write to a relation with triggers or rules while `hash` rules exist.
    ///
    /// The target's own, or those of a table a fired cascade changes: their
    /// bodies may fill a `hash`-ruled column with a value the statement steered.
    #[error(
        "column policy: relation {relation:?} has triggers or rules, which may fill hashed columns, so writes to it are refused"
    )]
    Trigger {
        /// The relation with the trigger or rule, as `schema.name`.
        relation: String,
    },

    /// A created column carrying ruled values unchanged, under a name no rule can match.
    ///
    /// Names only the position: the column's name is expression text.
    #[error(
        "column policy: created column {position} of {relation:?} carries ruled values; alias the column so a rule can name it"
    )]
    Unnamed {
        /// The created relation, as `schema.name`.
        relation: String,
        /// 0-based position among the created columns.
        position: usize,
    },

    /// An unnamed created column whose ruled values would lose their rules.
    ///
    /// [`Rejection::Laundering`] by position: the column's name is
    /// expression text, and an alias would not help a computed value.
    #[error(
        "column policy: writing created column {position} of {relation:?} would weaken the rules of its source columns"
    )]
    UnnamedLaundering {
        /// The created relation, as `schema.name`.
        relation: String,
        /// 0-based position among the created columns.
        position: usize,
    },

    /// An `UPDATE` or `MERGE` through a partitioned table whose partitions are ruled differently.
    ///
    /// A row moved between partitions would be read back under other rules.
    #[error(
        "column policy: UPDATE or MERGE through partitioned table {relation:?} could move rows between differently ruled partitions; target the partition"
    )]
    RowMovement {
        /// The partitioned target, as `schema.name`.
        relation: String,
    },

    /// A bare create target that names another relation than the one created.
    ///
    /// The rules were checked against the relation the name resolved to.
    #[error("column policy: create target {relation:?} resolves to another relation than it creates")]
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
            Self::DeniedStatement => "statement",
            Self::Explain { .. } => "explain",
            Self::ExplainAnalyze { .. } => "explain-analyze",
            Self::RuledIndex { .. } => "index",
            Self::Reference { .. } => "reference",
            Self::Conflict { .. } => "combine",
            Self::Laundering { .. } | Self::Unnamed { .. } | Self::UnnamedLaundering { .. } => "laundering",
            Self::Chosen { .. } => "chosen",
            Self::Computed { .. } => "computed",
            Self::Trigger { .. } => "trigger",
            Self::RowMovement { .. } => "row-movement",
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
