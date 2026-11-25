//! `privacy.rules` catalog reads, transaction start, and `EXPLAIN` format pin on `MySQL`.
//!
//! Every function takes the connection the planner holds and returns raw
//! rows; folding them into catalog entries stays in `crate::policy`.

use sqlx::mysql::MySqlDatabaseError;
use sqlx::{AssertSqlSafe, SqlStr, Transaction};

/// Starts a read-only transaction.
const BEGIN_READ: &str = "START TRANSACTION READ ONLY";

/// Starts a read-write transaction.
const BEGIN_WRITE: &str = "START TRANSACTION";

/// Starts the transaction a planned statement runs in; `write` allows writes.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed transaction start.
pub(crate) async fn begin(
    conn: &mut sqlx::MySqlConnection,
    write: bool,
) -> Result<Transaction<'_, sqlx::MySql>, sqlx::Error> {
    let begin = if write { BEGIN_WRITE } else { BEGIN_READ };
    sqlx::Connection::begin_with(conn, SqlStr::from_static(begin)).await
}

/// Reads the default database, `lower_case_table_names`, `sql_mode`, client charset, and server version.
const PROBE: &str = "SELECT CAST(DATABASE() AS CHAR), \
     CAST(@@lower_case_table_names AS SIGNED), CAST(@@sql_mode AS CHAR), \
     CAST(@@character_set_client AS CHAR), CAST(VERSION() AS CHAR)";

/// One row of [`PROBE`]: database, `lower_case_table_names`, `sql_mode`, client charset, `VERSION()`.
pub(crate) type Session = (Option<String>, i64, String, String, String);

/// Reads the session state that decides how names resolve.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn probe(conn: &mut sqlx::MySqlConnection) -> Result<Session, sqlx::Error> {
    sqlx::query_as(PROBE).fetch_one(conn).await
}

/// Finds one written relation by its stored schema and name, compared exactly.
///
/// Text columns are cast: `MySQL` 9 reports `information_schema` text as
/// `VARBINARY`, which `LOWER` leaves alone.
///
/// Lookups union one branch per name (no `OR`, no join with `COLUMNS` or
/// `VIEWS`): both servers read `information_schema` fast only for constant
/// schema and name.
const TABLES_EXACT: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(TABLE_TYPE AS CHAR), CAST(ENGINE AS CHAR) FROM information_schema.TABLES \
     WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// Finds one written relation case-insensitively, both parts bound lowercased.
///
/// Scans every table on the server, so it is used only where an exact
/// comparison would miss: `information_schema` names, and
/// `lower_case_table_names = 2`, which stores names as written but
/// compares them lowercased.
const TABLES_FOLDED: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(TABLE_TYPE AS CHAR), CAST(ENGINE AS CHAR) FROM information_schema.TABLES \
     WHERE LOWER(CAST(TABLE_SCHEMA AS CHAR)) = ? AND LOWER(CAST(TABLE_NAME AS CHAR)) = ?)";

/// How a `TABLES` branch compares a written name.
pub(crate) enum Match {
    /// [`TABLES_EXACT`]: stored schema and name, compared exactly.
    Exact,
    /// [`TABLES_FOLDED`]: both parts lowercased, bound lowercased.
    Folded,
}

/// One row of [`TABLES_EXACT`] / [`TABLES_FOLDED`].
pub(crate) type Table = (String, String, String, Option<String>);

/// Finds each `(match, schema, name)` lookup in `TABLES`, one branch per lookup.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn tables(
    conn: &mut sqlx::MySqlConnection,
    lookups: &[(Match, String, String)],
) -> Result<Vec<Table>, sqlx::Error> {
    let branches: Vec<_> = lookups
        .iter()
        .map(|(branch, ..)| match branch {
            Match::Exact => TABLES_EXACT,
            Match::Folded => TABLES_FOLDED,
        })
        .collect();
    let mut query = sqlx::query_as::<_, Table>(AssertSqlSafe(branches.join(" UNION ALL ")));
    for (_, schema, name) in lookups {
        query = query.bind(schema).bind(name);
    }
    query.fetch_all(conn).await
}

/// One relation's columns, bound with its exact schema and name.
///
/// Also each column's `COLUMN_DEFAULT` text (`NULL` when it has none; see
/// `crate::policy::has_default`) and whether `GENERATION_EXPRESSION` is set.
const COLUMNS_BRANCH: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(COLUMN_NAME AS CHAR), CAST(EXTRA AS CHAR), CAST(ORDINAL_POSITION AS SIGNED), \
     CAST(NULL AS CHAR), CAST(COLUMN_DEFAULT AS CHAR), \
     CAST(COALESCE(GENERATION_EXPRESSION, '') <> '' AS SIGNED), CAST(0 AS SIGNED) \
     FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// One view's body, bound with its exact schema and name, shaped like [`COLUMNS_BRANCH`].
const VIEW_BRANCH: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(NULL AS CHAR), CAST(NULL AS CHAR), CAST(0 AS SIGNED), \
     CAST(VIEW_DEFINITION AS CHAR), CAST(NULL AS CHAR), CAST(0 AS SIGNED), CAST(0 AS SIGNED) \
     FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// One relation's triggers, a row each, bound and shaped like [`COLUMNS_BRANCH`].
///
/// Without the `TRIGGER` privilege on the table the view lists none.
const TRIGGERS_BRANCH: &str = "(SELECT CAST(EVENT_OBJECT_SCHEMA AS CHAR), CAST(EVENT_OBJECT_TABLE AS CHAR), \
     CAST(NULL AS CHAR), CAST(NULL AS CHAR), CAST(0 AS SIGNED), \
     CAST(NULL AS CHAR), CAST(NULL AS CHAR), CAST(0 AS SIGNED), CAST(1 AS SIGNED) \
     FROM information_schema.TRIGGERS WHERE EVENT_OBJECT_SCHEMA = ? AND EVENT_OBJECT_TABLE = ?)";

/// Orders the detail rows by relation, then column position.
const DETAILS_ORDER: &str = " ORDER BY 1, 2, 5";

/// One row of the [`COLUMNS_BRANCH`] / [`VIEW_BRANCH`] / [`TRIGGERS_BRANCH`] union.
pub(crate) type Detail = (
    String,
    String,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

/// Reads the columns and triggers of each `(schema, name, is_view)`, and a view's body.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn details(
    conn: &mut sqlx::MySqlConnection,
    relations: &[(&str, &str, bool)],
) -> Result<Vec<Detail>, sqlx::Error> {
    let mut branches = Vec::new();
    let mut keys = Vec::new();
    for &(schema, name, view) in relations {
        branches.extend([COLUMNS_BRANCH, TRIGGERS_BRANCH]);
        keys.extend([(schema, name), (schema, name)]);
        if view {
            branches.push(VIEW_BRANCH);
            keys.push((schema, name));
        }
    }
    let sql = format!("{}{DETAILS_ORDER}", branches.join(" UNION ALL "));
    let mut query = sqlx::query_as::<_, Detail>(AssertSqlSafe(sql));
    for (schema, name) in keys {
        query = query.bind(schema).bind(name);
    }
    query.fetch_all(conn).await
}

/// Walks the referential actions below the tables of [`ROOT`] branches.
///
/// Per edge: its root's schema and name as bound, the parent's schema,
/// table, and column, the child's, the rule (`CASCADE`, `SET NULL`, `SET
/// DEFAULT`; `InnoDB` rejects the last, read anyway), and whether a parent
/// `DELETE` fires it. Below each reached child table come all its edges,
/// the closure `Relation::cascades` asks for. `UNION` ends the walk on
/// cycles. Names compare in the connection collation, which may over-match
/// (fail closed).
const CASCADES_HEAD: &str = "WITH RECURSIVE \
     edge (ps, pt, pc, cs, ct, cc, rule, deletes) AS (SELECT CAST(k.REFERENCED_TABLE_SCHEMA AS CHAR), \
           CAST(k.REFERENCED_TABLE_NAME AS CHAR), CAST(k.REFERENCED_COLUMN_NAME AS CHAR), \
           CAST(k.TABLE_SCHEMA AS CHAR), CAST(k.TABLE_NAME AS CHAR), CAST(k.COLUMN_NAME AS CHAR), \
           CAST(CASE d.deletes WHEN 1 THEN r.DELETE_RULE ELSE r.UPDATE_RULE END AS CHAR), d.deletes \
         FROM information_schema.REFERENTIAL_CONSTRAINTS AS r \
         JOIN information_schema.KEY_COLUMN_USAGE AS k ON k.CONSTRAINT_SCHEMA = r.CONSTRAINT_SCHEMA \
           AND k.CONSTRAINT_NAME = r.CONSTRAINT_NAME AND k.TABLE_NAME = r.TABLE_NAME \
         JOIN (SELECT 0 AS deletes UNION ALL SELECT 1) AS d \
         WHERE k.REFERENCED_COLUMN_NAME IS NOT NULL), \
     live AS (SELECT * FROM edge WHERE rule IN ('CASCADE', 'SET NULL', 'SET DEFAULT')), \
     roots (rs, rt) AS (";

/// One root table of [`CASCADES_HEAD`], bound with its exact schema and name.
const ROOT: &str = "SELECT CAST(? AS CHAR), CAST(? AS CHAR)";

/// Closes [`CASCADES_HEAD`] after its [`ROOT`] branches.
const CASCADES_TAIL: &str = "), \
     walk (rs, rt, ps, pt, pc, cs, ct, cc, rule, deletes) AS ( \
       SELECT roots.rs, roots.rt, live.* FROM roots JOIN live ON live.ps = roots.rs AND live.pt = roots.rt \
       UNION SELECT w.rs, w.rt, e.* FROM walk AS w JOIN live AS e ON e.ps = w.cs AND e.pt = w.ct) \
     SELECT rs, rt, ps, pt, pc, cs, ct, cc, rule, CAST(deletes AS SIGNED) FROM walk";

/// One edge of the [`CASCADES_HEAD`] walk.
pub(crate) type Edge = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    i64,
);

/// Walks the referential actions below every `(schema, table)` root, in one query.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn cascades(
    conn: &mut sqlx::MySqlConnection,
    roots: &[(&str, &str)],
) -> Result<Vec<Edge>, sqlx::Error> {
    let sql = format!(
        "{CASCADES_HEAD}{}{CASCADES_TAIL}",
        vec![ROOT; roots.len()].join(" UNION ALL ")
    );
    let mut query = sqlx::query_as::<_, Edge>(AssertSqlSafe(sql));
    for (schema, name) in roots {
        query = query.bind(*schema).bind(*name);
    }
    query.fetch_all(conn).await
}

/// One view's current body, bound with its exact schema and name.
const VIEW_CURRENT: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(VIEW_DEFINITION AS CHAR) FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// Reads the current body of each `(schema, name)` view, with its schema and name.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn view_bodies(
    conn: &mut sqlx::MySqlConnection,
    views: &[(&str, &str)],
) -> Result<Vec<(String, String, Option<String>)>, sqlx::Error> {
    let sql = vec![VIEW_CURRENT; views.len()].join(" UNION ALL ");
    let mut query = sqlx::query_as::<_, (String, String, Option<String>)>(AssertSqlSafe(sql));
    for (schema, name) in views {
        query = query.bind(*schema).bind(*name);
    }
    query.fetch_all(conn).await
}

/// Lists the schemas whose lowercased name is in the filter.
const SCHEMAS: &str = "SELECT CAST(SCHEMA_NAME AS CHAR) FROM information_schema.SCHEMATA WHERE ";

/// One schema filter of [`SCHEMAS`], bound lowercased.
const SCHEMAS_ONE: &str = "LOWER(CAST(SCHEMA_NAME AS CHAR)) = ?";

/// Lists the existing schemas whose lowercased name is among the lowercased `schemas`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn schemas(conn: &mut sqlx::MySqlConnection, schemas: &[String]) -> Result<Vec<String>, sqlx::Error> {
    let filter = vec![SCHEMAS_ONE; schemas.len()].join(" OR ");
    let mut query = sqlx::query_scalar::<_, String>(AssertSqlSafe(format!("{SCHEMAS}{filter}")));
    for schema in schemas {
        query = query.bind(schema);
    }
    query.fetch_all(conn).await
}

/// Pins the session's `EXPLAIN FORMAT=JSON` version to 1.
const EXPLAIN_PIN: &str = "SET SESSION explain_json_format_version = 1";

/// Restores the session's `EXPLAIN FORMAT=JSON` version to the server default.
const EXPLAIN_RESTORE: &str = "SET SESSION explain_json_format_version = DEFAULT";

/// Pins (`pin`) or restores this session's `EXPLAIN FORMAT=JSON` version.
///
/// Version 2 (the `MySQL` 9.5+ default) carries the statement text, which the
/// prune cannot read. `MySQL` before 8.3 has no such variable (error 1193)
/// and only speaks version 1. `SET_VAR` cannot scope it to one statement.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed statement other than error 1193.
pub(crate) async fn explain_format(conn: &mut sqlx::MySqlConnection, pin: bool) -> Result<(), sqlx::Error> {
    let set = if pin { EXPLAIN_PIN } else { EXPLAIN_RESTORE };
    match sqlx::query(set).execute(conn).await {
        Ok(_) => Ok(()),
        Err(error) if unknown_variable(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Whether `error` is `ER_UNKNOWN_SYSTEM_VARIABLE` (1193).
fn unknown_variable(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|error| error.try_downcast_ref::<MySqlDatabaseError>())
        .is_some_and(|error| error.number() == 1193)
}
