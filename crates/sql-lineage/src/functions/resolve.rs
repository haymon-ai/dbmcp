//! Resolution of a written function name against the dialect's built-in names.

use sqlparser::ast::{Ident, ObjectName};

use super::{mariadb, mysql, postgres, sqlite};
use crate::dialect::Dialect;
use crate::error::LineageError;
use crate::model::{FunctionUse, Origin};
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

/// Resolves a called function name to its recorded use.
///
/// The recorded name is the written name with each part case-folded per
/// dialect, joined with `.`. The use is known when the dialect lists the
/// name: only unqualified names match, plus `pg_catalog.`-qualified ones on
/// `PostgreSQL`; a call qualified with any other schema is never known.
/// Matching is by name only: an unqualified call that overload resolution
/// sends to a same-named user function is still reported known, so callers
/// that rely on `known` must rule out such user functions themselves.
///
/// # Errors
///
/// Returns [`LineageError::Unsupported`] if a name part is not an identifier.
pub(crate) fn resolve(dialect: Dialect, name: &ObjectName) -> Result<FunctionUse, LineageError> {
    let parts = idents(name)?;
    let known = match parts.as_slice() {
        [name] => Some(name),
        [schema, name] if dialect == Dialect::Postgres && dialect.normalize(schema) == "pg_catalog" => Some(name),
        _ => None,
    }
    .is_some_and(|name| names(dialect).contains(&fold(dialect, name).as_str()));
    Ok(FunctionUse {
        name: parts.iter().map(|p| fold(dialect, p)).collect::<Vec<_>>().join("."),
        known,
        origin: Origin::Query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every dialect's table.
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
        let used = resolve(dialect, &ObjectName::from(parts.to_vec())).expect("resolve");
        (used.name, used.known)
    }

    #[test]
    fn value_exposing_built_ins_are_absent() {
        for (dialect, names) in ABSENT {
            for name in *names {
                assert!(!call(*dialect, &[Ident::new(*name)]).1, "{dialect:?}: {name}");
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
    fn quoted_postgres_name_matches_exactly() {
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
