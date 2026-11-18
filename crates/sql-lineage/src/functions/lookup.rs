//! Function-name lookup in the dialect's built-in table.

use sqlparser::ast::Ident;

use super::table::{Kind, Table};
use super::{mariadb, mysql, postgres, sqlite};
use crate::dialect::Dialect;

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

    /// Built-ins that expose values, by dialect; none may be listed.
    const ABSENT: &[(Dialect, &[&str])] = &[
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
                "parse_ident",
                "to_date",
                "to_timestamp",
                "decode",
                "convert_from",
                "to_number",
                "pg_size_bytes",
                "jsonb_path_query",
                "jsonb_path_exists",
                "json_value",
                "json_query",
                "json_exists",
                "jsonb_set",
                "jsonb_insert",
                "to_tsquery",
                "plainto_tsquery",
                "setweight",
                "ts_filter",
                "ts_headline",
                "json_object_agg_unique",
                "json_objectagg",
                "date_trunc",
                "date_part",
                "timezone",
                "make_date",
                "chr",
                "get_byte",
                "regexp_replace",
                "regexp_matches",
                "format",
                "encode",
                "length",
                "percentile_cont",
                "power",
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
                "uuid_to_bin",
                "bin_to_uuid",
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

    #[test]
    fn value_exposing_built_ins_are_absent() {
        for (dialect, names) in ABSENT {
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
