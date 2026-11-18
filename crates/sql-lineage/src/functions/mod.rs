//! Per-dialect tables of built-in functions whose results depend only on their arguments.
//!
//! Each dialect has a hand-reviewed `const` table. A name it lists is
//! recorded as a known [`FunctionUse`](crate::FunctionUse); a scalar,
//! aggregate, or window call it lacks is still analyzed (its sources are the
//! union of its arguments' sources) but recorded as unknown. Table functions
//! carry their output columns, since a table function in `FROM` takes its
//! columns from the table.
//!
//! Built-ins that expose values the parser cannot see are deliberately left
//! out: functions that run SQL text or read whole relations by name, read
//! files, large objects or server settings, report session or server state,
//! use error messages as a value channel, or have side effects.

mod mariadb;
mod mysql;
mod postgres;
mod sqlite;

use sqlparser::ast::Ident;

use crate::dialect::Dialect;

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
const fn col(name: &'static str) -> Column {
    Column { name, visible: true }
}

/// Creates a column that `*` skips.
const fn hidden(name: &'static str) -> Column {
    Column { name, visible: false }
}

/// One dialect's built-in functions, by kind, names lowercase.
#[derive(Debug)]
struct Table {
    /// Scalar functions.
    scalars: &'static [&'static str],
    /// Aggregate functions.
    aggregates: &'static [&'static str],
    /// Window functions.
    windows: &'static [&'static str],
    /// Table functions and their output.
    tables: &'static [(&'static str, Output)],
}

/// Returns the dialect's table.
const fn table(dialect: Dialect) -> &'static Table {
    match dialect {
        Dialect::Postgres => &postgres::TABLE,
        Dialect::Mysql => &mysql::TABLE,
        Dialect::Mariadb => &mariadb::TABLE,
        Dialect::Sqlite => &sqlite::TABLE,
    }
}

/// Looks up a written function name in the dialect's table.
///
/// Only unqualified names match, plus `pg_catalog.`-qualified ones on
/// `PostgreSQL`; a call qualified with any other schema is never known.
/// Matching is by name only: an unqualified call that overload resolution
/// sends to a same-named user function is still reported known, so callers
/// that rely on `known` must rule out such user functions themselves.
pub(crate) fn lookup(dialect: Dialect, parts: &[Ident]) -> Option<Kind> {
    let name = match parts {
        [name] => name,
        [schema, name] if dialect == Dialect::Postgres && dialect.normalize(schema) == "pg_catalog" => name,
        _ => return None,
    };
    let name = dialect.function_name(name);
    let table = table(dialect);
    if table.scalars.contains(&name.as_str()) {
        return Some(Kind::Scalar);
    }
    if table.aggregates.contains(&name.as_str()) {
        return Some(Kind::Aggregate);
    }
    if table.windows.contains(&name.as_str()) {
        return Some(Kind::Window);
    }
    table
        .tables
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, output)| Kind::Table(*output))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dialect's table.
    const DIALECTS: [Dialect; 4] = [Dialect::Postgres, Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite];

    #[test]
    fn names_are_lowercase_and_unique_per_dialect() {
        for dialect in DIALECTS {
            let table = table(dialect);
            let mut names: Vec<&str> = table
                .scalars
                .iter()
                .chain(table.aggregates)
                .chain(table.windows)
                .copied()
                .chain(table.tables.iter().map(|(n, _)| *n))
                .collect();
            for name in &names {
                assert_eq!(*name, name.to_lowercase(), "{dialect:?}: {name}");
            }
            let total = names.len();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), total, "{dialect:?}: duplicate name");
        }
    }

    #[test]
    fn value_exposing_built_ins_are_absent() {
        let absent: &[(Dialect, &[&str])] = &[
            (
                Dialect::Postgres,
                &[
                    "query_to_xml",
                    "table_to_xml",
                    "cursor_to_xml",
                    "schema_to_xml",
                    "database_to_xml",
                    "xmlelement",
                    "xpath",
                    "ts_stat",
                    "ts_rewrite",
                    "lo_get",
                    "loread",
                    "current_setting",
                    "set_config",
                    "pg_read_file",
                    "pg_sleep",
                    "nextval",
                    "dblink",
                ],
            ),
            (
                Dialect::Mysql,
                &[
                    "extractvalue",
                    "updatexml",
                    "load_file",
                    "sleep",
                    "benchmark",
                    "gtid_subset",
                    "last_insert_id",
                    "user",
                    "uuid",
                    "rand",
                ],
            ),
            (
                Dialect::Mariadb,
                &[
                    "extractvalue",
                    "updatexml",
                    "load_file",
                    "sleep",
                    "benchmark",
                    "nextval",
                    "lastval",
                ],
            ),
            (
                Dialect::Sqlite,
                &[
                    "load_extension",
                    "sqlite_offset",
                    "changes",
                    "last_insert_rowid",
                    "random",
                    "readfile",
                    "sqlite_version",
                ],
            ),
        ];
        for (dialect, names) in absent {
            for name in *names {
                assert_eq!(lookup(*dialect, &[Ident::new(*name)]), None, "{dialect:?}: {name}");
            }
        }
    }

    #[test]
    fn only_pg_catalog_qualifies_a_built_in() {
        let lower = Ident::new("lower");
        assert_eq!(
            lookup(Dialect::Postgres, &[Ident::new("pg_catalog"), lower.clone()]),
            Some(Kind::Scalar)
        );
        assert_eq!(lookup(Dialect::Postgres, &[Ident::new("public"), lower.clone()]), None);
        assert_eq!(lookup(Dialect::Mysql, &[Ident::new("pg_catalog"), lower]), None);
    }

    #[test]
    fn quoted_postgres_name_matches_exactly() {
        assert_eq!(lookup(Dialect::Postgres, &[Ident::new("LOWER")]), Some(Kind::Scalar));
        assert_eq!(lookup(Dialect::Postgres, &[Ident::with_quote('"', "LOWER")]), None);
        assert_eq!(
            lookup(Dialect::Mysql, &[Ident::with_quote('`', "LOWER")]),
            Some(Kind::Scalar)
        );
    }
}
