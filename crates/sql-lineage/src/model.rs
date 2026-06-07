//! Public lineage result types.
//!
//! [`TableRef`] and [`SourceRef`] compare and order case-insensitively (ASCII)
//! so set membership matches the catalog's case-insensitive lookup semantics.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Lowercases an ASCII string for case-insensitive comparison.
fn fold(s: &str) -> String {
    s.to_ascii_lowercase()
}

/// A physical table, optionally schema- and database-qualified.
///
/// Equality, ordering, and hashing are case-insensitive (ASCII) on all parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRef {
    /// Optional database/catalog qualifier present in the query.
    pub database: Option<String>,
    /// Optional schema qualifier present in the query.
    pub schema: Option<String>,
    /// Physical table name, as written in the query (original case preserved).
    pub name: String,
}

impl TableRef {
    /// Creates a bare (unqualified) table reference.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            database: None,
            schema: None,
            name: name.into(),
        }
    }

    /// Creates a schema-qualified table reference (no database qualifier).
    pub fn qualified(schema: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            database: None,
            schema: Some(schema.into()),
            name: name.into(),
        }
    }

    /// Starts building a table reference with optional qualifiers.
    pub fn builder(name: impl Into<String>) -> TableRefBuilder {
        TableRefBuilder {
            database: None,
            schema: None,
            name: name.into(),
        }
    }

    /// Returns the case-insensitive comparison key `(database, schema, name)`.
    fn key(&self) -> (Option<String>, Option<String>, String) {
        (
            self.database.as_deref().map(fold),
            self.schema.as_deref().map(fold),
            fold(&self.name),
        )
    }
}

/// Fluent builder for a schema/database-qualified [`TableRef`].
#[derive(Debug, Clone)]
pub struct TableRefBuilder {
    database: Option<String>,
    schema: Option<String>,
    name: String,
}

impl TableRefBuilder {
    /// Sets the schema qualifier.
    #[must_use]
    pub fn with_schema(mut self, schema: impl Into<String>) -> Self {
        self.schema = Some(schema.into());
        self
    }

    /// Sets the database/catalog qualifier.
    #[must_use]
    pub fn with_database(mut self, database: impl Into<String>) -> Self {
        self.database = Some(database.into());
        self
    }

    /// Builds the [`TableRef`].
    #[must_use]
    pub fn build(self) -> TableRef {
        TableRef {
            database: self.database,
            schema: self.schema,
            name: self.name,
        }
    }
}

impl PartialEq for TableRef {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for TableRef {}

impl PartialOrd for TableRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for TableRef {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl std::hash::Hash for TableRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

/// A concrete physical origin of (part of) an output column.
///
/// Equality, ordering, and hashing are case-insensitive (ASCII).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceRef {
    /// The physical table (alias stripped, qualifier preserved).
    pub table: TableRef,
    /// The physical column name on that table.
    pub column: String,
}

impl SourceRef {
    /// Creates a source reference.
    pub fn new(table: TableRef, column: impl Into<String>) -> Self {
        Self {
            table,
            column: column.into(),
        }
    }

    /// Returns the case-insensitive comparison key.
    fn key(&self) -> (Option<String>, Option<String>, String, String) {
        let (db, schema, table) = self.table.key();
        (db, schema, table, fold(&self.column))
    }
}

impl PartialEq for SourceRef {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl Eq for SourceRef {}

impl PartialOrd for SourceRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SourceRef {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

impl std::hash::Hash for SourceRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key().hash(state);
    }
}

/// One column in a query's result set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputColumn {
    /// Output name/alias as seen by the client; `None` for unnamed expressions.
    pub name: Option<String>,
    /// 0-based position in the result set.
    pub position: usize,
    /// Contributing source columns; empty for literals/constants.
    pub sources: BTreeSet<SourceRef>,
}

/// The lineage extracted from a single statement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lineage {
    /// One entry per output column, in projection (or target) order.
    pub columns: Vec<OutputColumn>,
    /// All physical input/source tables referenced by the statement.
    pub tables: BTreeSet<TableRef>,
    /// Target table for a write statement; `None` for a read (`SELECT`).
    pub target: Option<TableRef>,
}

impl Lineage {
    /// Returns an empty lineage (no columns, no tables, no target).
    #[must_use]
    pub fn empty() -> Self {
        Self {
            columns: Vec::new(),
            tables: BTreeSet::new(),
            target: None,
        }
    }

    /// Builds a read lineage from output columns, deriving the source tables.
    #[must_use]
    pub(crate) fn from_columns(columns: Vec<OutputColumn>) -> Self {
        Self {
            tables: tables_of(&columns),
            columns,
            target: None,
        }
    }

    /// Builds a write lineage for `target` from its mapped output columns.
    ///
    /// `tables` accumulates `input_tables` plus every source table feeding a
    /// column; the `target` sink is intentionally excluded.
    #[must_use]
    pub(crate) fn from_write(target: TableRef, columns: Vec<OutputColumn>, input_tables: BTreeSet<TableRef>) -> Self {
        let mut tables = input_tables;
        tables.extend(tables_of(&columns));
        Self {
            columns,
            tables,
            target: Some(target),
        }
    }
}

/// Collects the distinct source tables feeding a set of output columns.
fn tables_of(columns: &[OutputColumn]) -> BTreeSet<TableRef> {
    columns
        .iter()
        .flat_map(|c| c.sources.iter().map(|s| s.table.clone()))
        .collect()
}
