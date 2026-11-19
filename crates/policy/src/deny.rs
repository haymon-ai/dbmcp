//! Relations and statements denied while column rules exist.
//!
//! They expose values outside the columns lineage tracks: column statistics,
//! statement text of other sessions, locked key values, binary logs, raw
//! pages, large objects, server settings and state, and `SQLite` virtual and
//! shadow tables. Names compare ASCII case-insensitively.

use dbmcp_sql_lineage::{Dialect, RelationKind, RelationUse};

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

/// `MySQL` and `MariaDB` `SHOW` forms that echo values or other sessions' SQL.
///
/// `EXPLAIN` and `ANALYZE` are `MariaDB`'s `SHOW EXPLAIN FOR` / `SHOW ANALYZE
/// FOR`, which attach another connection's statement text; `MySQL` rejects them.
const MYSQL_SHOW: &[&str] = &[
    "WARNINGS",
    "ERRORS",
    "ENGINE",
    "PROCESSLIST",
    "BINLOG",
    "RELAYLOG",
    "EXPLAIN",
    "ANALYZE",
    "REPLICA",
    "SLAVE",
];

/// Reports whether `relation` is denied on `dialect`.
pub(crate) fn relation(dialect: Dialect, relation: &RelationUse) -> bool {
    let id = &relation.id;
    match dialect {
        Dialect::Postgres => POSTGRES.iter().any(|name| id.name.eq_ignore_ascii_case(name)),
        Dialect::Mysql | Dialect::Mariadb => {
            let lower = id.name.to_ascii_lowercase();
            MYSQL.iter().any(|(schema, name)| {
                id.schema.eq_ignore_ascii_case(schema)
                    && match name {
                        Name::Any => true,
                        Name::Is(name) => lower == *name,
                        Name::Prefix(prefix) => lower.starts_with(prefix),
                        Name::Contains(text) => lower.contains(text),
                    }
            })
        }
        Dialect::Sqlite => {
            relation.kind == RelationKind::Virtual || SQLITE.iter().any(|name| id.name.eq_ignore_ascii_case(name))
        }
    }
}

/// Reports whether `SHOW <what>` is denied on `dialect`; every `PostgreSQL` `SHOW` reads a setting.
pub(crate) fn show(dialect: Dialect, what: &str) -> bool {
    match dialect {
        Dialect::Postgres => true,
        Dialect::Mysql | Dialect::Mariadb => MYSQL_SHOW.iter().any(|denied| what.eq_ignore_ascii_case(denied)),
        Dialect::Sqlite => false,
    }
}
