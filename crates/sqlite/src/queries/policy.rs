//! `privacy.rules` catalog reads and transaction start on `SQLite`.
//!
//! Every function takes the connection the planner holds and returns raw
//! rows; folding them into catalog entries stays in `crate::policy`.

use dbmcp_policy::Access;
use serde_json::Value;
use sqlx::{AssertSqlSafe, SqlStr, Transaction};

use super::ident;

/// Starts a read transaction.
const BEGIN_READ: &str = "BEGIN";

/// Starts a write transaction, taking the write lock before anything runs.
const BEGIN_WRITE: &str = "BEGIN IMMEDIATE";

/// Starts the transaction a planned statement runs in under `access`; a write takes the write lock.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed `BEGIN`.
pub(crate) async fn begin(
    conn: &mut sqlx::SqliteConnection,
    access: Access,
) -> Result<Transaction<'_, sqlx::Sqlite>, sqlx::Error> {
    let begin = match access {
        Access::Read => BEGIN_READ,
        Access::Write => BEGIN_WRITE,
    };
    sqlx::Connection::begin_with(conn, SqlStr::from_static(begin)).await
}

/// Lists every schema name, `main` first, in attach order.
const SCHEMAS: &str = "SELECT name FROM pragma_database_list ORDER BY seq";

/// Lists every schema name in search order.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn schemas(conn: &mut sqlx::SqliteConnection) -> Result<Vec<String>, sqlx::Error> {
    sqlx::query_scalar(SCHEMAS).fetch_all(conn).await
}

/// Lists the relations named by the JSON array of names in `?1`, `temp` first.
///
/// `pragma_table_list(name)` searches every schema and matches names
/// case-insensitively, as `SQLite` resolves them. Listing every relation
/// instead costs about 13 ms per call at 2,000 tables; this lookup stays
/// near 0.3 ms whatever the schema size.
const LOOKUP: &str = "SELECT t.schema, t.name, t.type \
     FROM json_each(?1) AS j, pragma_table_list(j.value) AS t \
     JOIN pragma_database_list AS d ON d.name = t.schema \
     ORDER BY t.schema <> 'temp', d.seq";

/// Lists `(schema, name, type)` of each relation named `tables`, in name-resolution order.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn lookup(
    conn: &mut sqlx::SqliteConnection,
    tables: &[&str],
) -> Result<Vec<(String, String, String)>, sqlx::Error> {
    let tables = Value::from(tables.to_vec()).to_string();
    sqlx::query_as(LOOKUP).bind(tables).fetch_all(conn).await
}

/// Lists the columns of each `[schema, name]` pair of the JSON array in `?1`.
///
/// Also whether the server can fill each column: a declared default, or an
/// `INTEGER` primary-key column, which may alias the rowid (over-approximated:
/// composite keys count too).
const COLUMNS: &str = "SELECT j.key, x.name, x.hidden, \
     x.dflt_value IS NOT NULL OR (x.pk > 0 AND upper(x.type) = 'INTEGER') \
     FROM json_each(?1) AS j, \
     pragma_table_xinfo(json_extract(j.value, '$[1]'), json_extract(j.value, '$[0]')) AS x \
     ORDER BY j.key, x.cid";

/// One row of [`COLUMNS`]: pair index, column name, `hidden`, server-fillable.
pub(crate) type Column = (i64, String, i64, bool);

/// Lists the columns of each `(schema, name)` pair; [`Column`]'s first field indexes `pairs`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn columns(
    conn: &mut sqlx::SqliteConnection,
    pairs: &[(&str, &str)],
) -> Result<Vec<Column>, sqlx::Error> {
    let pairs: Vec<Value> = pairs
        .iter()
        .map(|(schema, name)| Value::from(vec![*schema, *name]))
        .collect();
    let pairs = Value::from(pairs).to_string();
    sqlx::query_as(COLUMNS).bind(pairs).fetch_all(conn).await
}

/// Lists the tables of `?1` that a trigger names, the `temp` schema's triggers included.
///
/// `{schema}` is the quoted schema. A temporary trigger may fire on a table
/// of any schema; it is matched by table name alone (fail closed).
const TRIGGERED: &str = "SELECT tbl_name FROM {schema}.sqlite_schema WHERE type = 'trigger' \
     UNION SELECT tbl_name FROM temp.sqlite_schema WHERE type = 'trigger'";

/// Lists the table names, as stored, that a trigger in `schema` or `temp` names.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn triggered(conn: &mut sqlx::SqliteConnection, schema: &str) -> Result<Vec<String>, sqlx::Error> {
    let sql = TRIGGERED.replace("{schema}", &ident::quote(schema));
    sqlx::query_scalar(AssertSqlSafe(sql)).fetch_all(conn).await
}

/// Walks the referential actions below table `?2` of schema `?1`.
///
/// Per edge: parent table and column, child table and column, the action
/// (`CASCADE`, `SET NULL`, `SET DEFAULT`), and whether a parent `DELETE`
/// fires it (else a parent `UPDATE` of the column). A foreign key never
/// leaves its schema; one naming no parent column references the parent's
/// primary key, mapped by position. Table and column names come back as
/// stored, not as the foreign key spells them; a key naming a missing
/// column, which no write gets past, gives no edge. Below each reached
/// child table come all its edges, the closure [`Relation::cascades`](dbmcp_policy::Relation::cascades) asks
/// for. `UNION` ends the walk on cycles.
const CASCADES: &str = "WITH RECURSIVE \
     fk(parent, pcol, child, ccol, rule, deletes) AS (SELECT p.name, \
           CASE WHEN f.\"to\" IS NULL \
             THEN (SELECT k.name FROM pragma_table_info(p.name, ?1) AS k WHERE k.pk = f.seq + 1) \
             ELSE (SELECT k.name FROM pragma_table_info(p.name, ?1) AS k WHERE k.name = f.\"to\" COLLATE NOCASE) END, \
           t.name, (SELECT k.name FROM pragma_table_info(t.name, ?1) AS k WHERE k.name = f.\"from\" COLLATE NOCASE), upper(CASE d.deletes WHEN 1 THEN f.on_delete ELSE f.on_update END), d.deletes \
         FROM pragma_table_list AS t \
         JOIN pragma_foreign_key_list(t.name, t.schema) AS f \
         JOIN pragma_table_list AS p ON p.schema = t.schema AND p.name = f.\"table\" COLLATE NOCASE \
         JOIN (SELECT 0 AS deletes UNION ALL SELECT 1) AS d \
         WHERE t.schema = ?1 AND t.type = 'table'), \
     edge AS (SELECT * FROM fk WHERE pcol IS NOT NULL AND ccol IS NOT NULL AND rule IN ('CASCADE', 'SET NULL', 'SET DEFAULT')), \
     walk AS (SELECT * FROM edge WHERE parent = ?2 COLLATE NOCASE \
       UNION SELECT e.* FROM walk AS w JOIN edge AS e ON e.parent = w.child COLLATE NOCASE) \
     SELECT parent, pcol, child, ccol, rule, deletes FROM walk";

/// One edge of [`CASCADES`]: parent, parent column, child, child column, rule, fired by `DELETE`.
pub(crate) type Edge = (String, String, String, String, String, bool);

/// Walks the referential-action edges below `table` of `schema`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn cascades(
    conn: &mut sqlx::SqliteConnection,
    schema: &str,
    table: &str,
) -> Result<Vec<Edge>, sqlx::Error> {
    sqlx::query_as(CASCADES).bind(schema).bind(table).fetch_all(conn).await
}

/// Reads the `CREATE VIEW` text of view `?1`; `{schema}` is the quoted schema.
const VIEW_BODY: &str = "SELECT sql FROM {schema}.sqlite_schema WHERE type = 'view' AND name = ?1";

/// Reads the `CREATE VIEW` statement of `view` in `schema`; `None` when missing or `NULL`.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn view_body(
    conn: &mut sqlx::SqliteConnection,
    schema: &str,
    view: &str,
) -> Result<Option<String>, sqlx::Error> {
    let sql = VIEW_BODY.replace("{schema}", &ident::quote(schema));
    let create: Option<Option<String>> = sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(view)
        .fetch_optional(conn)
        .await?;
    Ok(create.flatten())
}
