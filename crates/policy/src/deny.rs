//! Relations and statements denied while column rules exist.
//!
//! They expose values outside the columns lineage tracks: column statistics,
//! statement text of other sessions, locked key values, binary logs, raw
//! pages, large objects, server settings and state, and `SQLite` virtual and
//! shadow tables. `MySQL` `SHOW` forms are allowlisted instead. Names compare
//! ASCII case-insensitively.

use dbmcp_sql_lineage::{Dialect, RelationKind, RelationUse, Show, WrittenName};

/// How a deny entry matches a relation name.
enum Name {
    /// Every relation in the schema.
    Any,
    /// Exactly this name.
    Is(&'static str),
    /// Names starting with this prefix.
    Prefix(&'static str),
    /// Names containing this text.
    Contains(&'static str),
}

/// `PostgreSQL` relations denied in any schema (`pg_stat_statements` lives in
/// whichever schema the extension was installed into).
const POSTGRES: &[&str] = &[
    "pg_statistic",
    "pg_stats",
    "pg_stats_ext",
    "pg_stats_ext_exprs",
    "pg_statistic_ext_data",
    "pg_stat_activity",
    "pg_stat_statements",
    "pg_largeobject",
    "pg_largeobject_metadata",
    "pg_settings",
    "pg_file_settings",
    "pg_db_role_setting",
];

/// `MySQL` and `MariaDB` relations denied, by schema.
const MYSQL: &[(&str, Name)] = &[
    ("information_schema", Name::Is("column_statistics")),
    ("information_schema", Name::Is("processlist")),
    ("information_schema", Name::Is("innodb_trx")),
    ("information_schema", Name::Is("innodb_locks")),
    // Words of `FULLTEXT`-indexed columns.
    ("information_schema", Name::Prefix("innodb_ft_")),
    ("performance_schema", Name::Prefix("events_statements_")),
    ("performance_schema", Name::Is("threads")),
    ("performance_schema", Name::Is("processlist")),
    ("performance_schema", Name::Is("data_locks")),
    ("performance_schema", Name::Is("prepared_statements_instances")),
    ("performance_schema", Name::Is("user_variables_by_thread")),
    // Replication errors quote row values.
    ("performance_schema", Name::Prefix("replication_applier_status")),
    ("information_schema", Name::Is("slave_status")),
    ("sys", Name::Contains("statement")),
    ("sys", Name::Contains("processlist")),
    ("sys", Name::Contains("session")),
    ("sys", Name::Contains("innodb_lock_waits")),
    ("mysql", Name::Any),
];

/// `SQLite` relations denied in any schema, besides every virtual table.
const SQLITE: &[&str] = &[
    "sqlite_stat1",
    "sqlite_stat2",
    "sqlite_stat3",
    "sqlite_stat4",
    "sqlite_dbpage",
];

/// Reports whether `relation` is denied on `dialect`.
pub(crate) fn relation(dialect: Dialect, relation: &RelationUse) -> bool {
    (dialect == Dialect::Sqlite && relation.kind == RelationKind::Virtual)
        || named(dialect, &relation.id.schema, &relation.id.name)
}

/// Returns the written name to report when the deny tables name it.
///
/// Checks two-part `MySQL` and `MariaDB` names, and the last part of a
/// one- to three-part `PostgreSQL` or one- or two-part `SQLite` name. Other
/// names wait for the catalog: their schema or relation kind is unknown.
pub(crate) fn written(dialect: Dialect, name: &WrittenName) -> Option<String> {
    let denied = match (dialect, name.parts.as_slice()) {
        (Dialect::Mysql | Dialect::Mariadb, [schema, relation]) => named(dialect, &schema.value, &relation.value),
        (Dialect::Postgres, [relation] | [_, relation] | [_, _, relation])
        | (Dialect::Sqlite, [relation] | [_, relation]) => named(dialect, "", &relation.value),
        _ => false,
    };
    if !denied {
        return None;
    }
    let parts: Vec<&str> = name.parts.iter().map(|part| part.value.as_str()).collect();
    let mut rendered = String::new();
    for c in parts.join(".").chars() {
        if c.is_control() {
            rendered.extend(c.escape_default());
        } else {
            rendered.push(c);
        }
    }
    Some(rendered)
}

/// Reports whether the deny tables name `schema.name` on `dialect`.
///
/// `PostgreSQL` and `SQLite` deny names in any schema.
fn named(dialect: Dialect, schema: &str, name: &str) -> bool {
    match dialect {
        Dialect::Postgres => POSTGRES.iter().any(|denied| name.eq_ignore_ascii_case(denied)),
        Dialect::Mysql | Dialect::Mariadb => {
            let lower = name.to_ascii_lowercase();
            MYSQL.iter().any(|(denied, entry)| {
                schema.eq_ignore_ascii_case(denied)
                    && match entry {
                        Name::Any => true,
                        Name::Is(denied) => lower == *denied,
                        Name::Prefix(prefix) => lower.starts_with(prefix),
                        Name::Contains(text) => lower.contains(text),
                    }
            })
        }
        Dialect::Sqlite => SQLITE.iter().any(|denied| name.eq_ignore_ascii_case(denied)),
    }
}

/// Reports whether a `SHOW` form is denied on `dialect`.
///
/// Every `PostgreSQL` `SHOW` reads a setting; `MySQL` and `MariaDB` allow
/// the listed [`Show`] forms that replay no values.
pub(crate) fn show(dialect: Dialect, show: Show) -> bool {
    match dialect {
        Dialect::Postgres => true,
        Dialect::Mysql | Dialect::Mariadb => show == Show::Unsupported || show.replays_values(),
        Dialect::Sqlite => false,
    }
}
