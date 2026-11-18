//! Shapes of the per-dialect built-in function tables.

/// The kind of a built-in function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// One value per row.
    Scalar,
    /// One value per group.
    Aggregate,
    /// One value per row over a window.
    Window,
    /// A set of rows usable in `FROM`.
    Table(Output),
}

/// The columns a table function returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Output {
    /// One column named after the function, or after the table alias in `PostgreSQL`.
    Scalar,
    /// Fixed columns, in order.
    Columns(&'static [Column]),
}

/// One output column of a table function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Column {
    /// The column name.
    pub(crate) name: &'static str,
    /// Whether `*` expands to the column.
    pub(crate) visible: bool,
}

/// Creates a column that `*` expands to.
pub(super) const fn col(name: &'static str) -> Column {
    Column { name, visible: true }
}

/// Creates a column that `*` skips.
pub(super) const fn hidden(name: &'static str) -> Column {
    Column { name, visible: false }
}

/// One dialect's built-in functions, by kind, names lowercase.
#[derive(Debug)]
pub(super) struct Table {
    /// Scalar functions.
    pub(super) scalars: &'static [&'static str],
    /// Aggregate functions.
    pub(super) aggregates: &'static [&'static str],
    /// Window functions.
    pub(super) windows: &'static [&'static str],
    /// Table functions and their output.
    pub(super) tables: &'static [(&'static str, Output)],
}
