//! Resolution of a written function name against the dialect's built-in names.

use sqlparser::ast::{Ident, ObjectName};

use super::{mariadb, mysql, postgres, sqlite};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::resolve::idents;

/// Returns the dialect's built-in function names.
const fn names(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Postgres => postgres::NAMES,
        Dialect::Mysql => mysql::NAMES,
        Dialect::Mariadb => mariadb::NAMES,
        Dialect::Sqlite => sqlite::NAMES,
    }
}

/// Returns a function name part case-folded the way the server resolves it.
fn fold(dialect: Dialect, ident: &Ident) -> String {
    match dialect {
        Dialect::Postgres => dialect.normalize(ident),
        Dialect::Mysql | Dialect::Mariadb | Dialect::Sqlite => ident.value.to_lowercase(),
    }
}

/// Resolves a called function name to its recorded name and whether it is known.
///
/// Known when the dialect lists the unqualified (or, on `PostgreSQL`,
/// `pg_catalog.`-qualified) name. Matching is by name only, so a same-named
/// user function is still reported known.
///
/// # Errors
///
/// Returns [`LineageError::Unsupported`] if a name part is not an identifier.
pub(crate) fn resolve(dialect: Dialect, name: &ObjectName) -> Result<(String, bool), LineageError> {
    let parts: Vec<String> = idents(name)?.iter().map(|p| fold(dialect, p)).collect();
    let known = match parts.as_slice() {
        [name] => names(dialect).contains(&name.as_str()),
        [schema, name] if dialect == Dialect::Postgres && schema == "pg_catalog" => {
            names(dialect).contains(&name.as_str())
        }
        _ => false,
    };
    Ok((parts.join("."), known))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIALECTS: [Dialect; 4] = [Dialect::Postgres, Dialect::Mysql, Dialect::Mariadb, Dialect::Sqlite];

    #[test]
    fn names_are_lowercase_and_unique_per_dialect() {
        for dialect in DIALECTS {
            let mut names = names(dialect).to_vec();
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
                "convert_to",
                "unistr",
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

    /// Resolves a name written as `parts`, returning its recorded name and verdict.
    fn call(dialect: Dialect, parts: &[Ident]) -> (String, bool) {
        resolve(dialect, &ObjectName::from(parts.to_vec())).expect("resolve")
    }

    #[test]
    fn value_exposing_built_ins_are_absent() {
        for (dialect, names) in ABSENT {
            for name in *names {
                assert!(!super::names(*dialect).contains(name), "{dialect:?}: {name}");
            }
        }
    }

    #[test]
    fn only_pg_catalog_qualifies_a_built_in() {
        let lower = Ident::new("lower");
        assert_eq!(
            call(Dialect::Postgres, &[Ident::new("PG_CATALOG"), lower.clone()]),
            ("pg_catalog.lower".to_string(), true)
        );
        assert_eq!(
            call(Dialect::Postgres, &[Ident::new("public"), lower.clone()]),
            ("public.lower".to_string(), false)
        );
        assert_eq!(
            call(Dialect::Mysql, &[Ident::new("pg_catalog"), lower]),
            ("pg_catalog.lower".to_string(), false)
        );
    }

    #[test]
    fn tables_are_per_dialect() {
        let cases = [
            (Dialect::Mysql, "nvl", false),
            (Dialect::Mariadb, "nvl", true),
            (Dialect::Mysql, "json_schema_validation_report", true),
            (Dialect::Mariadb, "json_schema_validation_report", false),
            (Dialect::Postgres, "json_value", false),
            (Dialect::Mysql, "json_value", true),
            (Dialect::Sqlite, "printf", true),
            (Dialect::Postgres, "printf", false),
        ];
        for (dialect, name, known) in cases {
            assert_eq!(call(dialect, &[Ident::new(name)]).1, known, "{dialect:?}: {name}");
        }
    }

    #[test]
    fn other_qualifiers_are_unknown() {
        let lower = Ident::new("lower");
        let cases = [
            (
                Dialect::Postgres,
                vec![Ident::with_quote('"', "pg_catalog"), Ident::with_quote('"', "lower")],
                true,
            ),
            (
                Dialect::Postgres,
                vec![Ident::with_quote('"', "PG_CATALOG"), lower.clone()],
                false,
            ),
            (
                Dialect::Postgres,
                vec![Ident::new("db"), Ident::new("pg_catalog"), lower.clone()],
                false,
            ),
            (Dialect::Mysql, vec![Ident::new("app"), lower.clone()], false),
            (Dialect::Mariadb, vec![Ident::new("pg_catalog"), lower], false),
        ];
        for (dialect, parts, known) in cases {
            assert_eq!(call(dialect, &parts).1, known, "{dialect:?}: {parts:?}");
        }
    }

    #[test]
    fn quoted_name_folds_per_dialect() {
        assert_eq!(
            call(Dialect::Postgres, &[Ident::new("LOWER")]),
            ("lower".to_string(), true)
        );
        assert_eq!(
            call(Dialect::Postgres, &[Ident::with_quote('"', "LOWER")]),
            ("LOWER".to_string(), false)
        );
        assert_eq!(
            call(Dialect::Mysql, &[Ident::with_quote('`', "LOWER")]),
            ("lower".to_string(), true)
        );
    }
}
