//! Column rule (`pii.rules`) enforcement on `PostgreSQL`.
//!
//! With column rules, `readQuery`, `writeQuery`, and `explainQuery` run each
//! statement through [`PostgresHandler::enforce`]. On one pooled connection
//! of the target database, inside one transaction (read-only for reads) and
//! under one deadline, it:
//!
//! 1. probes the session: database, creation schema, and whether
//!    `standard_conforming_strings` lets the parser read literals as the
//!    server does,
//! 2. analyzes the statement's lineage, resolving the names it reports
//!    missing with `to_regclass` (so the session's `search_path` decides, as
//!    it will when the statement runs), columns from `pg_attribute`, view
//!    bodies from `pg_get_viewdef`, triggers and rules from `pg_trigger` and
//!    `pg_rewrite`, and referential actions from `pg_constraint`, for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds,
//! 3. plans it with [`Analyzed::plan`]; a rejection runs nothing,
//! 4. executes it, refusing a result that does not fit the plan before any
//!    row is kept or a write commits,
//! 5. refuses the result when a view body it planned from has changed, or
//!    a ruled relation joined the inheritance tree of a table it read,
//! 6. commits a write or rolls a read back.
//!
//! `CREATE INDEX` runs outside the transaction once planned, since its
//! `CONCURRENTLY` form cannot run inside one.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors outside the value-free `SQLSTATE`
//! classes, `COMMIT` included, are replaced by a generic message.

use std::collections::{BTreeSet, HashMap};
use std::mem;

use dbmcp_config::{DatabaseBackend, PiiConfig};
use dbmcp_policy::{
    Analysis, Analyzed, Cascade, CascadeAction, CatalogColumn, Entry, Family, Member, Origin, Planned, Rejection,
    Relation, RelationId, RelationKind, RelationRef, SourceRef, StatementKind, Step, Unfit, WrittenName,
};
use dbmcp_sql::{Connection as _, SqlError};
use dbmcp_truncate::Window;
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlx::postgres::{PgColumn, PgQueryResult, PgRow};
use sqlx::{Column as _, FromRow as _, Row as _, SqlStr};

use crate::PostgresHandler;
use crate::connection::{PostgresConnection, quote_ident};

/// Reads the session state that decides how names resolve.
///
/// The database, `standard_conforming_strings`, the first existing schema
/// of the search path, the search path itself, and the current user.
const PROBE_SQL: &str = "SELECT current_database()::text, \
     current_setting('standard_conforming_strings'), \
     (current_schemas(false))[1]::text, \
     current_setting('search_path'), current_user::text";

/// Resolves each `to_regclass` input of `$1`, with its explicit schema in `$2`.
///
/// Per name, in order: schema, name, `relkind`, OID, view body, column names
/// and numbers (system columns, numbered below zero, come last), whether
/// the explicit schema exists, and its inheritance closure.
///
/// The closure is every descendant (`pg_inherits` children, transitively)
/// and every ancestor of the relation or a descendant: each node's OID,
/// schema, name, and `relkind`, then each `pg_inherits` edge whose child is
/// a node, as child and parent OIDs. `UNION` ends the walk on diamonds. It
/// takes no lock and names no `inhdetachpending`, so a partition pending
/// detach still counts.
///
/// Last, per column, whether the server can fill it (a default, an
/// identity, or a domain type with a default) and whether it is generated,
/// each OR-ed over the relation and every descendant by column name: a
/// row written through the relation may land in any of them.
///
/// Then whether the relation or a descendant has a user trigger (enabled
/// or not) or a non-`SELECT` rule, and the referential actions below them:
/// per edge, the parent's schema, table, and column, the child's, the
/// action (`confupdtype` or `confdeltype`: `c`, `n`, `d`), whether a
/// parent `DELETE` fires it, and whether the child has a user trigger or
/// non-`SELECT` rule. Each foreign key's columns pair by position. The walk
/// starts at every edge whose parent is the relation or a descendant; below
/// each reached child table come all its edges, the closure
/// [`Relation::cascades`] asks for. `UNION` ends it on cycles.
const RESOLVE_SQL: &str = "SELECT s.nspname::text, c.relname::text, c.relkind::text, c.oid::int8, \
     CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid) END, \
     coalesce(cols.names, '{}'), coalesce(cols.numbers, '{}'), \
     to_regnamespace(n.schema) IS NOT NULL, \
     coalesce(tree.oids, '{}'), coalesce(tree.schemas, '{}'), coalesce(tree.names, '{}'), \
     coalesce(tree.relkinds, '{}'), coalesce(tree.children, '{}'), coalesce(tree.parents, '{}'), \
     coalesce(cols.defaults, '{}'), coalesce(cols.generated, '{}'), \
     coalesce(fk.triggers, true), coalesce(fk.parent_schemas, '{}'), coalesce(fk.parent_names, '{}'), \
     coalesce(fk.parent_columns, '{}'), coalesce(fk.child_schemas, '{}'), coalesce(fk.child_names, '{}'), \
     coalesce(fk.child_columns, '{}'), coalesce(fk.actions, '{}'), coalesce(fk.deletes, '{}'), \
     coalesce(fk.child_triggers, '{}') \
     FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS n(name, schema, ord) \
     LEFT JOIN pg_class AS c ON c.oid = to_regclass(n.name) \
     LEFT JOIN pg_namespace AS s ON s.oid = c.relnamespace \
     LEFT JOIN LATERAL (WITH RECURSIVE \
           down(oid) AS (SELECT c.oid UNION SELECT i.inhrelid FROM pg_inherits AS i JOIN down ON i.inhparent = down.oid) \
           SELECT array_agg(a.attname::text ORDER BY a.attnum < 0, a.attnum) AS names, \
           array_agg(a.attnum ORDER BY a.attnum < 0, a.attnum) AS numbers, \
           array_agg(f.fills ORDER BY a.attnum < 0, a.attnum) AS defaults, \
           array_agg(f.generated ORDER BY a.attnum < 0, a.attnum) AS generated \
           FROM pg_attribute AS a \
           CROSS JOIN LATERAL (SELECT coalesce(bool_or(d.atthasdef OR d.attidentity <> '' \
                 OR (t.typtype = 'd' AND t.typdefaultbin IS NOT NULL)), false) AS fills, \
               coalesce(bool_or(d.attgenerated <> ''), false) AS generated \
             FROM down JOIN pg_attribute AS d ON d.attrelid = down.oid AND d.attname = a.attname \
               AND NOT d.attisdropped \
             LEFT JOIN pg_type AS t ON t.oid = d.atttypid) AS f \
           WHERE a.attrelid = c.oid AND a.attnum <> 0 AND NOT a.attisdropped) AS cols ON true \
     LEFT JOIN LATERAL (WITH RECURSIVE \
           down(oid) AS (SELECT c.oid UNION SELECT i.inhrelid FROM pg_inherits AS i JOIN down ON i.inhparent = down.oid), \
           up(oid) AS (SELECT down.oid FROM down UNION SELECT i.inhparent FROM pg_inherits AS i JOIN up ON i.inhrelid = up.oid) \
           SELECT nodes.oids, nodes.schemas, nodes.names, nodes.relkinds, edges.children, edges.parents \
           FROM (SELECT array_agg(r.oid::int8 ORDER BY r.oid) AS oids, \
                   array_agg(rs.nspname::text ORDER BY r.oid) AS schemas, \
                   array_agg(r.relname::text ORDER BY r.oid) AS names, \
                   array_agg(r.relkind::text ORDER BY r.oid) AS relkinds \
                 FROM up JOIN pg_class AS r ON r.oid = up.oid \
                 JOIN pg_namespace AS rs ON rs.oid = r.relnamespace) AS nodes, \
                (SELECT array_agg(i.inhrelid::int8 ORDER BY i.inhrelid, i.inhparent) AS children, \
                   array_agg(i.inhparent::int8 ORDER BY i.inhrelid, i.inhparent) AS parents \
                 FROM pg_inherits AS i WHERE i.inhrelid IN (SELECT up.oid FROM up)) AS edges) AS tree ON true \
     LEFT JOIN LATERAL (WITH RECURSIVE \
           down(oid) AS (SELECT c.oid UNION SELECT i.inhrelid FROM pg_inherits AS i JOIN down ON i.inhparent = down.oid), \
           edge(parent, pcol, child, ccol, action, deletes) AS (SELECT k.confrelid, k.confkey[p.i], k.conrelid, \
                 k.conkey[p.i], CASE WHEN d.deletes THEN k.confdeltype ELSE k.confupdtype END, d.deletes \
               FROM pg_constraint AS k \
               CROSS JOIN LATERAL generate_subscripts(k.conkey, 1) AS p(i) \
               CROSS JOIN (VALUES (false), (true)) AS d(deletes) \
               WHERE k.contype = 'f' AND CASE WHEN d.deletes THEN k.confdeltype ELSE k.confupdtype END IN ('c', 'n', 'd')), \
           walk AS (SELECT e.* FROM edge AS e WHERE e.parent IN (SELECT down.oid FROM down) \
             UNION SELECT e.* FROM walk AS w JOIN edge AS e ON e.parent = w.child) \
           SELECT EXISTS (SELECT FROM pg_trigger AS g WHERE g.tgrelid IN (SELECT down.oid FROM down) AND NOT g.tgisinternal) \
               OR EXISTS (SELECT FROM pg_rewrite AS r WHERE r.ev_class IN (SELECT down.oid FROM down) AND r.ev_type <> '1') \
               AS triggers, \
             array_agg(ps.nspname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS parent_schemas, \
             array_agg(pc.relname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS parent_names, \
             array_agg(pa.attname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS parent_columns, \
             array_agg(cs.nspname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS child_schemas, \
             array_agg(cc.relname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS child_names, \
             array_agg(ca.attname::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS child_columns, \
             array_agg(w.action::text ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS actions, \
             array_agg(w.deletes ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS deletes, \
             array_agg(EXISTS (SELECT FROM pg_trigger AS g WHERE g.tgrelid = w.child AND NOT g.tgisinternal) \
                 OR EXISTS (SELECT FROM pg_rewrite AS r WHERE r.ev_class = w.child AND r.ev_type <> '1') \
               ORDER BY w.parent, w.pcol, w.child, w.ccol, w.deletes) AS child_triggers \
           FROM walk AS w \
           JOIN pg_class AS pc ON pc.oid = w.parent JOIN pg_namespace AS ps ON ps.oid = pc.relnamespace \
           JOIN pg_attribute AS pa ON pa.attrelid = w.parent AND pa.attnum = w.pcol \
           JOIN pg_class AS cc ON cc.oid = w.child JOIN pg_namespace AS cs ON cs.oid = cc.relnamespace \
           JOIN pg_attribute AS ca ON ca.attrelid = w.child AND ca.attnum = w.ccol) AS fk ON true \
     ORDER BY n.ord";

/// `SQLSTATE` classes whose messages name schema objects or states, never values.
///
/// Syntax and access (`42`), transaction state (`25`), rollbacks (`40`),
/// resources (`53`), limits (`54`), operator intervention (`57`), and
/// connections (`08`).
///
/// Not unsupported features (`0A`): a `reg*` cast of a three-part value
/// quotes it as a cross-database reference.
const VALUE_FREE_CLASSES: &[&str] = &["42", "25", "40", "53", "54", "57", "08"];

/// Class `42` codes withheld anyway.
///
/// `reg*` input casts (`ssn::regclass`) quote the cast text in syntax,
/// undefined-relation, -object, -function, and ambiguous-function errors;
/// `invalid_name` quotes configuration parameter and replication slot names
/// taken from values.
const VALUE_ECHOING_CODES: &[&str] = &["42601", "42602", "42P01", "42704", "42725", "42883"];

/// Re-reads the inheritance closure of each relation OID in `$1`, in order.
///
/// The same walk as [`RESOLVE_SQL`]: per OID, the schema and name of every
/// descendant and every ancestor of the relation or a descendant, the
/// relation itself included. A relation gone since is an empty closure.
const FAMILY_SQL: &str = "SELECT coalesce(tree.schemas, '{}'), coalesce(tree.names, '{}') \
     FROM unnest($1::int8[]) WITH ORDINALITY AS o(oid, ord) \
     LEFT JOIN LATERAL (WITH RECURSIVE \
           down(oid) AS (SELECT o.oid::oid UNION SELECT i.inhrelid FROM pg_inherits AS i JOIN down ON i.inhparent = down.oid), \
           up(oid) AS (SELECT down.oid FROM down UNION SELECT i.inhparent FROM pg_inherits AS i JOIN up ON i.inhrelid = up.oid) \
           SELECT array_agg(rs.nspname::text ORDER BY r.oid) AS schemas, \
             array_agg(r.relname::text ORDER BY r.oid) AS names \
           FROM up JOIN pg_class AS r ON r.oid = up.oid \
           JOIN pg_namespace AS rs ON rs.oid = r.relnamespace) AS tree ON true \
     ORDER BY o.ord";

/// Reads the current body of each view OID in `$1`, in order.
const VIEWS_SQL: &str = "SELECT pg_get_viewdef(v.oid::oid) \
     FROM unnest($1::int8[]) WITH ORDINALITY AS v(oid, ord) ORDER BY v.ord";

/// Schemas whose relations are system catalogs, never expanded as views.
///
/// Not `pg_toast`: TOAST relations hold the raw out-of-line values of
/// their tables, ruled columns included, so they stay absent.
const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema"];

/// Whether the statement may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// Runs in a read-only transaction and rolls back.
    Read,
    /// Commits; refused in read-only mode.
    Write,
}

/// Redacted rows and write counts of a statement run under the column rules.
#[derive(Debug)]
pub(crate) struct Enforced {
    pub(crate) rows: Vec<Value>,
    pub(crate) rows_affected: u64,
    pub(crate) last_insert_id: Option<u64>,
}

/// Why a statement returned no rows under the column rules.
#[derive(Debug, thiserror::Error)]
enum Stop {
    /// The planner refused the statement; nothing ran.
    #[error(transparent)]
    Rejected(#[from] Rejection),
    /// The result does not fit the plan; nothing was committed.
    #[error(transparent)]
    Unfit(#[from] Unfit),
    /// A result column came from a relation the catalog reads did not describe.
    #[error("column policy: result column {0} comes from a relation the policy did not resolve")]
    Unresolved(usize),
    /// A view body changed between planning and execution; nothing was committed.
    #[error("column policy: a view changed while the statement ran")]
    ViewChanged,
    /// A ruled relation joined an inheritance tree the statement read; nothing was committed.
    #[error("column policy: an inheritance tree changed while the statement ran")]
    FamilyChanged,
    /// The statement failed with an error that may echo a ruled value.
    #[error("query failed (SQLSTATE {0}): error detail withheld under column policy")]
    Withheld(String),
    /// Any other database error, reported as without column rules.
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
}

struct Run<'c> {
    planned: Planned<'c>,
    names: Vec<String>,
    rows_affected: u64,
    last_insert_id: Option<u64>,
}

struct Executed {
    names: Vec<String>,
    results: Vec<PgQueryResult>,
}

/// Session state that decides how names resolve.
struct Probe {
    /// The connected database: the catalog segment of rule keys.
    database: String,
    /// The schema an unqualified `CREATE` lands in; `None` when unknown.
    creation: Option<String>,
}

/// A relation the catalog reads described, keyed by OID in [`Owners`].
struct Owner {
    id: RelationId,
    /// Column names by attribute number.
    columns: HashMap<i16, String>,
    /// The `pg_get_viewdef` body planned from, for a view or materialized view.
    view: Option<String>,
    /// Whether its inheritance tree is re-read: tables and foreign tables.
    tree: bool,
}

/// The relations a statement's result columns may come from, by OID.
type Owners = HashMap<u32, Owner>;

/// Returns the quoted, qualified name `to_regclass` reads for `reference`.
fn regclass(reference: &RelationRef) -> String {
    match &reference.schema {
        Some(schema) => format!("{}.{}", quote_ident(schema), quote_ident(&reference.name)),
        None => quote_ident(&reference.name),
    }
}

/// The write facts of one [`RESOLVE_SQL`] row, after its first 16 columns.
///
/// Triggers, then per cascade edge: parent schema, table, and column, child
/// schema, table, and column, action, `DELETE`-fired, and child triggers.
type Writes = (
    bool,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<bool>,
    Vec<bool>,
);

/// The first 16 columns of one [`RESOLVE_SQL`] row; [`Writes`] holds the rest.
type Resolved = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Vec<String>,
    Vec<i16>,
    bool,
    Vec<i64>,
    Vec<String>,
    Vec<String>,
    Vec<String>,
    Vec<i64>,
    Vec<i64>,
    Vec<bool>,
    Vec<bool>,
);

impl PostgresHandler {
    /// Plans `sql` under `policy`'s rules, runs it into `window`, and redacts the kept rows.
    ///
    /// `sql` is the text that runs, on one connection to `database`. The
    /// window holds each row as a JSON array of its values; the kept rows
    /// come back redacted, as objects.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] for a write in read-only
    /// mode, a refusal when the handler has no redactor, the reason the
    /// column rules stopped the statement, or the [`SqlError`] of anything
    /// else that failed.
    pub(crate) async fn enforce(
        &self,
        policy: &PiiConfig,
        sql: &str,
        access: Access,
        timeout_secs: u64,
        database: Option<&str>,
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        let Some(redactor) = &self.redactor else {
            return Err(ErrorData::internal_error(
                "column policy: column rules need PII redaction enabled",
                None,
            ));
        };
        if access == Access::Write && self.connection.read_only() {
            return Err(SqlError::ReadOnlyViolation.into());
        }
        let run = self
            .connection
            .with_connection(sql, database, timeout_secs, async |conn| {
                match run(conn, policy, sql, access, &mut |row| window.push(row)).await {
                    Err(Stop::Sql(error)) => Err(error),
                    other => Ok(other),
                }
            })
            .await?
            .map_err(|stop| ErrorData::internal_error(stop.to_string(), None))?;
        let rows = mem::take(&mut window.items).into_iter().map(values).collect();
        let rows = run.planned.redact(redactor, run.names, rows).await?;
        Ok(Enforced {
            rows,
            rows_affected: run.rows_affected,
            last_insert_id: run.last_insert_id,
        })
    }
}

/// Unwraps a window row, which `execute` always pushes as an array.
fn values(row: Value) -> Vec<Value> {
    match row {
        Value::Array(values) => values,
        other => vec![other],
    }
}

/// Plans and executes one statement inside a transaction on `conn`.
async fn run<'c>(
    conn: &mut sqlx::PgConnection,
    config: &'c PiiConfig,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let begin = match access {
        Access::Read => "START TRANSACTION READ ONLY",
        Access::Write => "BEGIN",
    };
    let mut tx = sqlx::Connection::begin_with(&mut *conn, SqlStr::from_static(begin)).await?;
    let (analyzed, owners) = resolve(&mut tx, sql).await?;
    let mut planned = analyzed.plan(config)?;
    if *analyzed.statement() == StatementKind::CreateIndex {
        tx.rollback().await?;
        return autocommit(conn, sql, planned, &owners, sink).await;
    }
    let executed = match execute(&mut tx, sql, &mut planned, &owners, sink).await {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(failed(&planned, error)),
        Err(stop) => {
            tx.rollback().await?;
            return Err(stop);
        }
    };
    if !views_unchanged(&mut tx, &owners, analyzed.target()).await? {
        tx.rollback().await?;
        return Err(Stop::ViewChanged);
    }
    if !families_unchanged(&mut tx, &owners, &planned).await? {
        tx.rollback().await?;
        return Err(Stop::FamilyChanged);
    }
    // Deferred constraints fail at `COMMIT`, with messages as revealing.
    let finished = match access {
        Access::Read => tx.rollback().await,
        Access::Write => tx.commit().await,
    };
    finished.map_err(|error| failed(&planned, error))?;
    let (rows_affected, last_insert_id) = PostgresConnection::counts(sql, &executed.results);
    Ok(Run {
        planned,
        names: executed.names,
        rows_affected,
        last_insert_id,
    })
}

/// Runs a planned `CREATE INDEX` outside a transaction block; it returns no rows.
async fn autocommit<'c>(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    mut planned: Planned<'c>,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let executed = match execute(conn, sql, &mut planned, owners, sink).await {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(failed(&planned, error)),
        Err(stop) => return Err(stop),
    };
    let (rows_affected, last_insert_id) = PostgresConnection::counts(sql, &executed.results);
    Ok(Run {
        planned,
        names: executed.names,
        rows_affected,
        last_insert_id,
    })
}

/// Reports whether every view body planned from is still the current one.
///
/// Executing a view holds a lock that blocks its replacement until the
/// transaction ends, so the body read now is the one that ran. The
/// statement's own `target`, which it may drop or replace, is skipped.
async fn views_unchanged(
    conn: &mut sqlx::PgConnection,
    owners: &Owners,
    target: Option<&RelationId>,
) -> Result<bool, sqlx::Error> {
    let (oids, bodies): (Vec<i64>, Vec<&str>) = owners
        .iter()
        .filter(|(_, owner)| target != Some(&owner.id))
        .filter_map(|(oid, owner)| Some((i64::from(*oid), owner.view.as_deref()?)))
        .unzip();
    if oids.is_empty() {
        return Ok(true);
    }
    let current: Vec<Option<String>> = sqlx::query_scalar(VIEWS_SQL).bind(oids).fetch_all(conn).await?;
    Ok(current.iter().map(Option::as_deref).eq(bodies.into_iter().map(Some)))
}

/// Reports whether no ruled relation joined a planned table's inheritance tree.
///
/// Every table and foreign table is re-read, childless ones included, since
/// `ATTACH PARTITION` and `INHERIT` take no lock the statement's scan
/// blocks. A relation that left a tree since planning is ignored.
async fn families_unchanged(
    conn: &mut sqlx::PgConnection,
    owners: &Owners,
    planned: &Planned<'_>,
) -> Result<bool, sqlx::Error> {
    let (oids, trees): (Vec<i64>, Vec<&RelationId>) = owners
        .iter()
        .filter(|(_, owner)| owner.tree)
        .map(|(oid, owner)| (i64::from(*oid), &owner.id))
        .unzip();
    if oids.is_empty() {
        return Ok(true);
    }
    let current: Vec<(Vec<String>, Vec<String>)> = sqlx::query_as(FAMILY_SQL).bind(oids).fetch_all(conn).await?;
    Ok(trees.into_iter().zip(current).all(|(owner, (schemas, names))| {
        schemas
            .iter()
            .zip(&names)
            .all(|(schema, name)| !planned.joined_ruled(owner, &RelationId::new(&owner.catalog, schema, name)))
    }))
}

/// Runs `sql`, feeding each row's values to `sink`.
///
/// The first row is checked against `planned` before any row reaches `sink`.
async fn execute(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    planned: &mut Planned<'_>,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, Stop> {
    let first = |row: &PgRow, names: &[String]| {
        planned.check_first(names, origins(row.columns(), owners)?)?;
        Ok::<_, Stop>(())
    };
    let (names, results) = PostgresConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed { names, results })
}

/// Translates each result column's OID and attribute number into its origin.
///
/// A view column reports the view; a whole-row reference has no column.
///
/// # Errors
///
/// Returns [`Stop::Unresolved`] for an origin the catalog reads did not describe.
fn origins(columns: &[PgColumn], owners: &Owners) -> Result<Vec<(usize, RelationId, Option<String>)>, Stop> {
    let mut origins = Vec::new();
    for column in columns {
        let position = column.ordinal();
        let Some(oid) = column.relation_id() else {
            continue;
        };
        let Some(owner) = owners.get(&oid.0) else {
            return Err(Stop::Unresolved(position));
        };
        let name = match column.relation_attribute_no() {
            Some(number) => match owner.columns.get(&number) {
                Some(name) => Some(name.clone()),
                None => return Err(Stop::Unresolved(position)),
            },
            None => None,
        };
        origins.push((position, owner.id.clone(), name));
    }
    Ok(origins)
}

/// Analyzes `sql`, fetching missing relations for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds.
///
/// Also returns the relations it described, by OID.
async fn resolve(conn: &mut sqlx::PgConnection, sql: &str) -> Result<(Analyzed, Owners), Stop> {
    let (database, conforming, first, search_path, user): (String, String, Option<String>, String, String) =
        sqlx::query_as(PROBE_SQL).fetch_one(&mut *conn).await?;
    let probe = Probe {
        database,
        creation: first.filter(|first| !temp_first(&search_path, first, &user)),
    };
    let mut analysis = Analysis::new(sql, DatabaseBackend::Postgres);
    analysis.connected_to(Some(probe.database.clone()));
    analysis.creates_in(probe.creation.clone());
    if conforming != "on" {
        analysis.mismatch("standard_conforming_strings is off");
    }
    let mut owners = Owners::new();
    let mut entries = Vec::new();
    loop {
        match analysis.step(entries)? {
            Step::Missing(names) => entries = fetch(conn, &probe, names, &mut owners).await?,
            Step::Done(analyzed) => return Ok((analyzed, owners)),
        }
    }
}

/// Reports whether `search_path` lists a temporary schema before `first`.
///
/// `first` is the first existing schema; `$user` names `user`'s schema. The
/// server creates bare names in `pg_temp` when it comes first, creating that
/// schema on first use, so `first` would name the wrong creation target. A
/// path that never reaches `first` is treated as temporary.
fn temp_first(search_path: &str, first: &str, user: &str) -> bool {
    for element in search_path.split(',') {
        let element = element.trim();
        let name = element
            .strip_prefix('"')
            .and_then(|quoted| quoted.strip_suffix('"'))
            .map_or_else(|| element.to_ascii_lowercase(), str::to_owned);
        if name.starts_with("pg_temp") {
            return true;
        }
        if name == first || (name == "$user" && user == first) {
            return false;
        }
    }
    true
}

/// Builds a catalog entry for each of `names`, read in one batched query.
async fn fetch(
    conn: &mut sqlx::PgConnection,
    probe: &Probe,
    names: Vec<(WrittenName, RelationRef)>,
    owners: &mut Owners,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    let regclasses: Vec<String> = names.iter().map(|(_, reference)| regclass(reference)).collect();
    let schemas: Vec<Option<String>> = names
        .iter()
        .map(|(_, reference)| reference.schema.as_deref().map(quote_ident))
        .collect();
    let rows: Vec<PgRow> = sqlx::query(RESOLVE_SQL)
        .bind(regclasses)
        .bind(schemas)
        .fetch_all(&mut *conn)
        .await?;
    let mut entries = Vec::new();
    for ((name, reference), row) in names.into_iter().zip(rows) {
        let writes = writes(&row)?;
        let row = Resolved::from_row(&row)?;
        let entry = entry(probe, &name, &reference, &row, writes);
        let (_, _, _, oid, body, names, numbers, ..) = row;
        if let (Entry::Resolved(relation), Some(oid)) = (&entry, oid.and_then(|oid| u32::try_from(oid).ok())) {
            owners.entry(oid).or_insert_with(|| Owner {
                id: relation.id.clone(),
                columns: numbers.into_iter().zip(names).collect(),
                view: body,
                tree: matches!(relation.kind, RelationKind::Table | RelationKind::Foreign),
            });
        }
        entries.push((name, entry));
    }
    Ok(entries)
}

/// Reads the [`Writes`] columns of one [`RESOLVE_SQL`] row.
fn writes(row: &PgRow) -> Result<Writes, sqlx::Error> {
    Ok((
        row.try_get(16)?,
        row.try_get(17)?,
        row.try_get(18)?,
        row.try_get(19)?,
        row.try_get(20)?,
        row.try_get(21)?,
        row.try_get(22)?,
        row.try_get(23)?,
        row.try_get(24)?,
        row.try_get(25)?,
    ))
}

/// Reads whether a relation has triggers, and its cascades, from `writes`.
///
/// An action the walk does not name sets the child's default (fail closed).
fn triggers_and_cascades(probe: &Probe, writes: Writes) -> (bool, Vec<Cascade>) {
    let (
        triggers,
        parent_schemas,
        parent_names,
        parent_columns,
        child_schemas,
        child_names,
        child_columns,
        actions,
        deletes,
        child_triggers,
    ) = writes;
    let column =
        |schema: &str, name: &str, column: &str| SourceRef::new(RelationId::new(&probe.database, schema, name), column);
    let mut cascades = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        let (Some(ps), Some(pn), Some(pc), Some(cs), Some(cn), Some(cc), Some(&on_delete), Some(&child)) = (
            parent_schemas.get(index),
            parent_names.get(index),
            parent_columns.get(index),
            child_schemas.get(index),
            child_names.get(index),
            child_columns.get(index),
            deletes.get(index),
            child_triggers.get(index),
        ) else {
            // Arrays of unequal length: the relation counts as triggered.
            return (true, cascades);
        };
        cascades.push(Cascade {
            parent: column(ps, pn, pc),
            child: column(cs, cn, cc),
            action: match action.as_str() {
                "c" if on_delete => CascadeAction::Delete,
                "c" => CascadeAction::Copy,
                "n" => CascadeAction::Null,
                _ => CascadeAction::Default,
            },
            on_delete,
            triggers: child,
        });
    }
    (triggers, cascades)
}

/// Builds the catalog entry of one resolved row.
fn entry(probe: &Probe, name: &WrittenName, reference: &RelationRef, row: &Resolved, writes: Writes) -> Entry {
    let (schema, relname, relkind, _, body, columns, numbers, schema_exists, .., defaults, generated) = row;
    let (Some(schema), Some(relname), Some(relkind)) = (schema, relname, relkind) else {
        return vacant(probe, name, reference, *schema_exists);
    };
    let system = SYSTEM_SCHEMAS.contains(&schema.as_str());
    let kind = match relkind.as_str() {
        _ if system => RelationKind::System,
        "r" | "p" | "S" => RelationKind::Table,
        "f" => RelationKind::Foreign,
        "v" => RelationKind::View,
        "m" => RelationKind::MatView,
        _ => return Entry::Absent,
    };
    let columns = columns
        .iter()
        .zip(numbers)
        .enumerate()
        .map(|(index, (column, &number))| {
            // A fact the row lacks is `true` (fail closed).
            let fact = |facts: &[bool]| facts.get(index).copied().unwrap_or(true);
            CatalogColumn {
                name: column.clone(),
                star_visible: number > 0,
                default: fact(defaults),
                generated: fact(generated),
                // `PostgreSQL` has no on-update columns.
                on_update: false,
            }
        })
        .collect();
    let view_sql = match body.as_deref().map(view_query) {
        _ if !matches!(kind, RelationKind::View | RelationKind::MatView) => None,
        Some(query) if !query.is_empty() => Some(query),
        _ => return Entry::Absent,
    };
    let (triggers, cascades) = triggers_and_cascades(probe, writes);
    Entry::Resolved(Relation {
        id: RelationId::new(&probe.database, schema, relname),
        kind,
        columns,
        view_sql,
        family: family(probe, row),
        triggers,
        cascades,
    })
}

/// Builds the inheritance tree of one resolved row from its closure nodes and edges.
fn family(probe: &Probe, row: &Resolved) -> Family {
    let (_, _, relkind, oid, .., oids, schemas, names, relkinds, children, parents, _, _) = row;
    let Some(owner) = *oid else {
        return Family::default();
    };
    let nodes: HashMap<i64, (RelationId, bool)> = oids
        .iter()
        .zip(schemas.iter().zip(names).zip(relkinds))
        .map(|(&oid, ((schema, name), relkind))| {
            (oid, (RelationId::new(&probe.database, schema, name), relkind == "p"))
        })
        .collect();
    let mut up: HashMap<i64, Vec<i64>> = HashMap::new();
    let mut down: HashMap<i64, Vec<i64>> = HashMap::new();
    for (&child, &parent) in children.iter().zip(parents) {
        up.entry(child).or_default().push(parent);
        down.entry(parent).or_default().push(child);
    }
    let ids = |oids: BTreeSet<i64>| -> BTreeSet<RelationId> {
        oids.iter().filter_map(|oid| Some(nodes.get(oid)?.0.clone())).collect()
    };
    let mut descendants: Vec<Member> = closure(owner, &down)
        .into_iter()
        .filter_map(|oid| {
            let (id, partitioned) = nodes.get(&oid)?;
            Some(Member {
                id: id.clone(),
                partitioned: *partitioned,
                ancestors: ids(closure(oid, &up)),
            })
        })
        .collect();
    descendants.sort_by(|a, b| a.id.cmp(&b.id));
    Family {
        partitioned: relkind.as_deref() == Some("p"),
        ancestors: ids(closure(owner, &up)),
        descendants,
    }
}

/// Returns every node reachable from `start` along `edges`, `start` itself only on a loop.
fn closure(start: i64, edges: &HashMap<i64, Vec<i64>>) -> BTreeSet<i64> {
    let mut reached = BTreeSet::new();
    let mut pending = vec![start];
    while let Some(node) = pending.pop() {
        for &next in edges.get(&node).into_iter().flatten() {
            if reached.insert(next) {
                pending.push(next);
            }
        }
    }
    reached
}

/// Returns the entry for a name nothing resolves to.
///
/// A relation created under a bare name lands in the session's creation
/// schema; under a qualified name, in that schema when it exists. Temporary
/// schemas, names in view bodies, and names without a schema are absent.
fn vacant(probe: &Probe, name: &WrittenName, reference: &RelationRef, schema_exists: bool) -> Entry {
    if name.context != Origin::Query {
        return Entry::Absent;
    }
    let schema = match &reference.schema {
        None => probe.creation.as_deref(),
        Some(schema) => schema_exists.then_some(schema.as_str()),
    };
    schema
        .filter(|schema| !schema.starts_with("pg_temp") && !schema.starts_with("pg_toast_temp"))
        .map_or(Entry::Absent, |schema| {
            Entry::Vacant(RelationId::new(&probe.database, schema, &reference.name))
        })
}

/// Strips the whitespace and trailing semicolon `pg_get_viewdef` wraps a query in.
fn view_query(body: &str) -> String {
    let body = body.trim();
    body.strip_suffix(';').unwrap_or(body).trim().to_owned()
}

/// Withholds `error` when it may echo a value and the plan withholds.
fn failed(planned: &Planned<'_>, error: sqlx::Error) -> Stop {
    planned
        .withheld(echoing(&error))
        .map_or(Stop::Sql(error), Stop::Withheld)
}

/// Returns the `SQLSTATE` of a database error that may echo a value.
///
/// Timeouts and non-database errors return `None`.
fn echoing(error: &sqlx::Error) -> Option<String> {
    if PostgresConnection::is_timeout(error) {
        return None;
    }
    let code = error.as_database_error()?.code()?;
    let value_free =
        VALUE_FREE_CLASSES.iter().any(|class| code.starts_with(class)) && !VALUE_ECHOING_CODES.contains(&code.as_ref());
    (!value_free).then(|| code.into_owned())
}

#[cfg(test)]
mod tests {
    use dbmcp_policy::NamePart;

    use super::*;

    const DB: &str = "app";

    /// A reference to `name`, in `schema` when given.
    fn reference(schema: Option<&str>, name: &str) -> RelationRef {
        RelationRef {
            schema: schema.map(Into::into),
            name: name.into(),
        }
    }

    #[test]
    fn regclass_quotes_each_part() {
        assert_eq!(regclass(&reference(Some("public"), "Users")), "\"public\".\"Users\"");
        assert_eq!(regclass(&reference(None, "users")), "\"users\"");
        assert_eq!(regclass(&reference(None, "a\"b")), "\"a\"\"b\"");
    }

    #[test]
    fn temporary_schemas_before_the_first_existing_one_are_detected() {
        for path in [
            "pg_temp, public",
            "\"$user\", pg_temp, public",
            "PG_TEMP, public",
            "other",
        ] {
            assert!(temp_first(path, "public", "app"), "{path}");
        }
        for path in [
            "\"$user\", public",
            "public, pg_temp",
            "public, pg_temp_3",
            "\"PG_TEMP\", public",
            "temp, public",
        ] {
            assert!(!temp_first(path, "public", "app"), "{path}");
        }
        assert!(!temp_first("\"$user\", pg_temp, public", "app", "app"));
    }

    fn probe(creation: Option<&str>) -> Probe {
        Probe {
            database: DB.into(),
            creation: creation.map(Into::into),
        }
    }

    fn vacant_id(probe: &Probe, name: &WrittenName, reference: &RelationRef, exists: bool) -> Option<String> {
        match vacant(probe, name, reference, exists) {
            Entry::Vacant(id) => Some(format!("{}.{}.{}", id.catalog, id.schema, id.name)),
            _ => None,
        }
    }

    #[test]
    fn bare_creates_land_in_the_creation_schema() {
        let name = WrittenName::query(vec![NamePart::bare("New")]);
        let new = reference(None, "new");
        assert_eq!(
            vacant_id(&probe(Some("public")), &name, &new, false).as_deref(),
            Some("app.public.new")
        );
        assert_eq!(vacant_id(&probe(None), &name, &new, false), None);
        assert_eq!(vacant_id(&probe(Some("pg_temp_3")), &name, &new, false), None);
    }

    #[test]
    fn qualified_creates_need_an_existing_non_temporary_schema() {
        let name = WrittenName::query(vec![NamePart::bare("s"), NamePart::bare("n")]);
        let id = |schema, exists| vacant_id(&probe(Some("public")), &name, &reference(Some(schema), "n"), exists);
        assert_eq!(id("shadow", true).as_deref(), Some("app.shadow.n"));
        assert_eq!(id("nope", false), None);
        assert_eq!(id("pg_temp", true), None);
    }

    #[test]
    fn names_in_view_bodies_are_never_vacant() {
        let name = WrittenName {
            parts: vec![NamePart::bare("n")],
            context: Origin::View(RelationId::new(DB, "public", "v")),
        };
        assert_eq!(
            vacant_id(&probe(Some("public")), &name, &reference(None, "n"), false),
            None
        );
    }

    fn row(relkind: &str, schema: &str, body: Option<&str>) -> Resolved {
        (
            Some(schema.into()),
            Some("r".into()),
            Some(relkind.into()),
            Some(16_384),
            body.map(Into::into),
            vec!["id".into(), "ctid".into()],
            vec![1, -1],
            true,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![true, false],
            vec![false, false],
        )
    }

    /// A row for `public.<name>` (OID 1) whose closure holds `nodes` and `edges`.
    ///
    /// Each node is `(oid, name, relkind)` in `public`; each edge `(child, parent)`.
    fn tree_row(name: &str, relkind: &str, nodes: &[(i64, &str, &str)], edges: &[(i64, i64)]) -> Resolved {
        let mut row = row(relkind, "public", None);
        row.1 = Some(name.into());
        row.3 = Some(1);
        row.8 = nodes.iter().map(|node| node.0).collect();
        row.9 = nodes.iter().map(|_| "public".to_owned()).collect();
        row.10 = nodes.iter().map(|node| node.1.to_owned()).collect();
        row.11 = nodes.iter().map(|node| node.2.to_owned()).collect();
        row.12 = edges.iter().map(|edge| edge.0).collect();
        row.13 = edges.iter().map(|edge| edge.1).collect();
        row
    }

    /// `app.public.<name>`.
    fn id(name: &str) -> RelationId {
        RelationId::new(DB, "public", name)
    }

    /// The identities of `names`.
    fn ids(names: &[&str]) -> BTreeSet<RelationId> {
        names.iter().map(|name| id(name)).collect()
    }

    /// `app.public.<name>` as a [`Member`] below `ancestors`.
    fn member(name: &str, partitioned: bool, ancestors: &[&str]) -> Member {
        Member {
            id: id(name),
            partitioned,
            ancestors: ids(ancestors),
        }
    }

    #[test]
    fn partitioned_tables_carry_their_partitions() {
        let nodes = [
            (1, "ptab", "p"),
            (2, "ptab_1", "r"),
            (3, "ptab_2", "p"),
            (4, "ptab_2a", "r"),
        ];
        let edges = [(2, 1), (3, 1), (4, 3)];
        let relation = resolved(&tree_row("ptab", "p", &nodes, &edges)).expect("table");
        assert_eq!(
            relation.family,
            Family {
                partitioned: true,
                ancestors: BTreeSet::new(),
                descendants: vec![
                    member("ptab_1", false, &["ptab"]),
                    member("ptab_2", true, &["ptab"]),
                    member("ptab_2a", false, &["ptab", "ptab_2"]),
                ],
            }
        );
    }

    #[test]
    fn descendants_carry_every_parent_they_inherit() {
        let nodes = [
            (1, "inh_child", "r"),
            (2, "inh_parent", "r"),
            (3, "inh_other", "r"),
            (4, "inh_grandchild", "r"),
            (5, "inh_root", "r"),
        ];
        let edges = [(1, 2), (1, 3), (4, 1), (2, 5)];
        let relation = resolved(&tree_row("inh_child", "r", &nodes, &edges)).expect("table");
        assert_eq!(
            relation.family,
            Family {
                partitioned: false,
                ancestors: ids(&["inh_parent", "inh_other", "inh_root"]),
                descendants: vec![member(
                    "inh_grandchild",
                    false,
                    &["inh_child", "inh_parent", "inh_other", "inh_root"]
                )],
            }
        );
    }

    #[test]
    fn relations_outside_a_tree_have_an_empty_family() {
        let relation = resolved(&tree_row("solo", "r", &[(1, "solo", "r")], &[])).expect("table");
        assert_eq!(relation.family, Family::default());
    }

    fn resolved(row: &Resolved) -> Option<Relation> {
        let name = WrittenName::query(vec![NamePart::bare("r")]);
        match entry(
            &probe(Some("public")),
            &name,
            &reference(None, "r"),
            row,
            Writes::default(),
        ) {
            Entry::Resolved(relation) => Some(relation),
            _ => None,
        }
    }

    #[test]
    fn relkinds_map_to_relation_kinds() {
        for (relkind, kind) in [
            ("r", RelationKind::Table),
            ("p", RelationKind::Table),
            ("S", RelationKind::Table),
            ("f", RelationKind::Foreign),
        ] {
            let relation = resolved(&row(relkind, "public", None)).expect(relkind);
            assert_eq!(relation.kind, kind, "{relkind}");
            assert_eq!(relation.id, RelationId::new(DB, "public", "r"));
        }
        for relkind in ["i", "c", "t"] {
            assert_eq!(resolved(&row(relkind, "public", None)), None, "{relkind}");
        }
        assert_eq!(resolved(&row("t", "pg_toast", None)), None, "TOAST");
        let system = resolved(&row("v", "pg_catalog", None)).expect("system view");
        assert_eq!(system.kind, RelationKind::System);
        assert_eq!(system.view_sql, None);
    }

    #[test]
    fn system_columns_are_hidden_from_star() {
        let relation = resolved(&row("r", "public", None)).expect("table");
        assert_eq!(
            relation.columns,
            vec![
                CatalogColumn {
                    name: "id".into(),
                    star_visible: true,
                    default: true,
                    generated: false,
                    on_update: false,
                },
                CatalogColumn {
                    name: "ctid".into(),
                    star_visible: false,
                    default: false,
                    generated: false,
                    on_update: false,
                }
            ]
        );
    }

    #[test]
    fn columns_missing_from_the_fact_arrays_have_defaults_and_generate() {
        let mut row = row("r", "public", None);
        row.14 = Vec::new();
        row.15 = Vec::new();
        let relation = resolved(&row).expect("table");
        assert!(
            relation
                .columns
                .iter()
                .all(|column| column.default && column.generated && !column.on_update)
        );
    }

    #[test]
    fn views_carry_their_query_or_are_absent() {
        let view = resolved(&row("v", "public", Some(" SELECT users.id\n   FROM users;"))).expect("view");
        assert_eq!(view.kind, RelationKind::View);
        assert_eq!(view.view_sql.as_deref(), Some("SELECT users.id\n   FROM users"));
        let matview = resolved(&row("m", "public", Some(" SELECT 1;"))).expect("matview");
        assert_eq!(matview.kind, RelationKind::MatView);
        assert_eq!(resolved(&row("v", "public", None)), None);
    }

    #[test]
    fn unresolved_rows_fall_back_to_vacant() {
        let name = WrittenName::query(vec![NamePart::bare("n")]);

        let row: Resolved = (
            None,
            None,
            None,
            None,
            None,
            Vec::new(),
            Vec::new(),
            false,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        assert_eq!(
            entry(
                &probe(Some("public")),
                &name,
                &reference(None, "n"),
                &row,
                Writes::default()
            ),
            Entry::Vacant(RelationId::new(DB, "public", "n"))
        );
    }

    /// Connects to the seeded `app` database that `./tests/run.sh` starts.
    async fn seeded() -> sqlx::PgConnection {
        let host = std::env::var("DBMCP_DB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("DBMCP_DB_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(5432);
        let options = sqlx::postgres::PgConnectOptions::new()
            .host(&host)
            .port(port)
            .username("postgres")
            .database("app");
        <sqlx::PgConnection as sqlx::Connection>::connect_with(&options)
            .await
            .expect("connect to the seeded database")
    }

    /// Fetches the family of `public.<name>` from the seeded database.
    async fn seeded_family(conn: &mut sqlx::PgConnection, name: &str) -> Family {
        let probe = probe(Some("public"));
        let names = vec![(WrittenName::query(vec![NamePart::bare(name)]), reference(None, name))];
        let entries = fetch(conn, &probe, names, &mut Owners::new()).await.expect("fetch");
        match entries.into_iter().next() {
            Some((_, Entry::Resolved(relation))) => relation.family,
            other => panic!("{name}: {other:?}"),
        }
    }

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn resolve_reads_inheritance_and_partition_trees() {
        let mut conn = seeded().await;
        assert_eq!(
            seeded_family(&mut conn, "ptab").await,
            Family {
                partitioned: true,
                ancestors: BTreeSet::new(),
                descendants: vec![member("ptab_1", false, &["ptab"]), member("ptab_2", false, &["ptab"])],
            }
        );
        assert_eq!(
            seeded_family(&mut conn, "ptab_2").await,
            Family {
                partitioned: false,
                ancestors: ids(&["ptab"]),
                descendants: Vec::new(),
            }
        );
        let grandchild = member("inh_grandchild", false, &["inh_child", "inh_parent", "inh_other"]);
        assert_eq!(
            seeded_family(&mut conn, "inh_parent").await,
            Family {
                partitioned: false,
                ancestors: BTreeSet::new(),
                descendants: vec![
                    member("inh_child", false, &["inh_parent", "inh_other"]),
                    grandchild.clone()
                ],
            }
        );
        assert_eq!(
            seeded_family(&mut conn, "inh_child").await,
            Family {
                partitioned: false,
                ancestors: ids(&["inh_parent", "inh_other"]),
                descendants: vec![grandchild],
            }
        );
        assert_eq!(seeded_family(&mut conn, "users").await, Family::default());
    }

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn resolve_reads_column_facts_over_every_landing_member() {
        let mut conn = seeded().await;
        let mut tx = sqlx::Connection::begin(&mut conn).await.expect("begin");
        sqlx::raw_sql(SqlStr::from_static(
            "CREATE DOMAIN facts_email AS text DEFAULT 'd@example.com'; \
             CREATE TABLE facts_plain (id int GENERATED ALWAYS AS IDENTITY, a text, \
               b text DEFAULT 'x', c facts_email, d text GENERATED ALWAYS AS (a) STORED); \
             CREATE TABLE facts_p (id int, email text) PARTITION BY LIST (id); \
             CREATE TABLE facts_p1 PARTITION OF facts_p (email DEFAULT 'p@example.com') FOR VALUES IN (1)",
        ))
        .execute(&mut *tx)
        .await
        .expect("setup");
        let probe = probe(Some("public"));
        let mut facts = Vec::new();
        for name in ["facts_plain", "facts_p"] {
            let names = vec![(WrittenName::query(vec![NamePart::bare(name)]), reference(None, name))];
            let entries = fetch(&mut tx, &probe, names, &mut Owners::new()).await.expect("fetch");
            let Some((_, Entry::Resolved(relation))) = entries.into_iter().next() else {
                panic!("{name} resolves");
            };
            facts.extend(
                relation
                    .columns
                    .into_iter()
                    .filter(|column| column.star_visible)
                    .map(|column| (column.name, column.default, column.generated, column.on_update)),
            );
        }
        tx.rollback().await.expect("rollback");
        let fact = |name: &str, default, generated| (name.to_owned(), default, generated, false);
        assert_eq!(
            facts,
            [
                fact("id", true, false),
                fact("a", false, false),
                fact("b", true, false),
                fact("c", true, false),
                fact("d", true, true),
                fact("id", false, false),
                fact("email", true, false),
            ]
        );
    }

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn resolve_reads_triggers_rules_and_the_cascades_below_every_landing_member() {
        let mut conn = seeded().await;
        let mut tx = sqlx::Connection::begin(&mut conn).await.expect("begin");
        sqlx::raw_sql(SqlStr::from_static(
            "CREATE FUNCTION fk_noop() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RETURN NEW; END $$; \
             CREATE TABLE fk_p (a int, b int, UNIQUE (a, b)) PARTITION BY LIST (a); \
             CREATE TABLE fk_p1 PARTITION OF fk_p FOR VALUES IN (1); \
             CREATE TABLE fk_c (id int UNIQUE, x int, y int, \
               FOREIGN KEY (y, x) REFERENCES fk_p (a, b) ON UPDATE CASCADE ON DELETE SET NULL); \
             CREATE TABLE fk_g (z int REFERENCES fk_c (id) ON DELETE CASCADE, \
               w int REFERENCES fk_c (id) ON UPDATE SET DEFAULT ON DELETE RESTRICT); \
             CREATE TRIGGER fk_g_noop BEFORE INSERT ON fk_g FOR EACH ROW EXECUTE FUNCTION fk_noop(); \
             ALTER TABLE fk_g DISABLE TRIGGER fk_g_noop; \
             CREATE TABLE fk_r (id int); \
             CREATE RULE fk_r_none AS ON DELETE TO fk_r DO INSTEAD NOTHING; \
             CREATE TABLE fk_t (id int); \
             CREATE TRIGGER fk_t_noop BEFORE INSERT ON fk_p1 FOR EACH ROW EXECUTE FUNCTION fk_noop()",
        ))
        .execute(&mut *tx)
        .await
        .expect("setup");
        let probe = probe(Some("public"));
        let mut relations = Vec::new();
        for name in ["fk_p", "fk_c", "fk_r", "fk_t", "user_emails"] {
            let names = vec![(WrittenName::query(vec![NamePart::bare(name)]), reference(None, name))];
            let entries = fetch(&mut tx, &probe, names, &mut Owners::new()).await.expect("fetch");
            let Some((_, Entry::Resolved(relation))) = entries.into_iter().next() else {
                panic!("{name} resolves");
            };
            relations.push(relation);
        }
        tx.rollback().await.expect("rollback");
        let triggers: Vec<_> = relations.iter().map(|relation| relation.triggers).collect();
        // A partition's trigger, a rule, none, and a view's `SELECT` rule only.
        assert_eq!(triggers, [true, false, true, false, false]);
        let edges = |relation: &Relation| {
            let mut edges: Vec<_> = relation
                .cascades
                .iter()
                .map(|edge| {
                    format!(
                        "{}.{} -> {}.{} {:?} delete={} triggers={}",
                        edge.parent.relation.name,
                        edge.parent.column,
                        edge.child.relation.name,
                        edge.child.column,
                        edge.action,
                        edge.on_delete,
                        edge.triggers
                    )
                })
                .collect();
            edges.sort();
            edges
        };
        // `fk_p1` carries a clone of the constraint, so its edges come too.
        // The walk takes every edge of the reached `fk_c`, though no write
        // through `fk_p` fires them (`fk_c.id` never changes, no `fk_c` row
        // is removed).
        assert_eq!(
            edges(&relations[0]),
            [
                "fk_c.id -> fk_g.w Default delete=false triggers=true",
                "fk_c.id -> fk_g.z Delete delete=true triggers=true",
                "fk_p.a -> fk_c.y Copy delete=false triggers=false",
                "fk_p.a -> fk_c.y Null delete=true triggers=false",
                "fk_p.b -> fk_c.x Copy delete=false triggers=false",
                "fk_p.b -> fk_c.x Null delete=true triggers=false",
                "fk_p1.a -> fk_c.y Copy delete=false triggers=false",
                "fk_p1.a -> fk_c.y Null delete=true triggers=false",
                "fk_p1.b -> fk_c.x Copy delete=false triggers=false",
                "fk_p1.b -> fk_c.x Null delete=true triggers=false",
            ]
        );
        assert_eq!(
            edges(&relations[1]),
            [
                "fk_c.id -> fk_g.w Default delete=false triggers=true",
                "fk_c.id -> fk_g.z Delete delete=true triggers=true",
            ]
        );
    }

    /// Plans `sql` on `conn`, runs `change` on `other`, then executes and re-reads the trees.
    ///
    /// Returns whether every tree the plan read is unchanged as far as rules go.
    async fn unchanged_across(
        conn: &mut sqlx::PgConnection,
        other: &mut sqlx::PgConnection,
        config: &PiiConfig,
        sql: &str,
        change: &'static str,
    ) -> bool {
        let mut tx = sqlx::Connection::begin_with(&mut *conn, SqlStr::from_static("START TRANSACTION READ ONLY"))
            .await
            .expect("begin");
        let (analyzed, owners) = resolve(&mut tx, sql).await.expect("resolve");
        let mut planned = analyzed.plan(config).expect("plan");
        sqlx::raw_sql(SqlStr::from_static(change))
            .execute(&mut *other)
            .await
            .expect("change");
        execute(&mut tx, sql, &mut planned, &owners, &mut |_| {})
            .await
            .expect("execute");
        let unchanged = families_unchanged(&mut tx, &owners, &planned).await.expect("re-read");
        tx.rollback().await.expect("rollback");
        unchanged
    }

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn ruled_relations_joining_a_tree_after_planning_are_caught() {
        let mut conn = seeded().await;
        let mut other = seeded().await;
        let reset = "DROP TABLE IF EXISTS drift_p, drift_p1, drift_p2, drift_p3, drift_solo, drift_kid CASCADE";
        sqlx::raw_sql(SqlStr::from_static(reset))
            .execute(&mut other)
            .await
            .expect("reset");
        sqlx::raw_sql(SqlStr::from_static(
            "CREATE TABLE drift_p (id int, email text) PARTITION BY RANGE (id); \
             CREATE TABLE drift_p1 PARTITION OF drift_p FOR VALUES FROM (0) TO (10); \
             CREATE TABLE drift_p2 (id int, email text); \
             CREATE TABLE drift_p3 (id int, email text); \
             CREATE TABLE drift_solo (id int, email text); \
             CREATE TABLE drift_kid (id int, email text); \
             INSERT INTO drift_p VALUES (1, 'one@example.com'); \
             INSERT INTO drift_p2 VALUES (11, 'two@example.com'); \
             INSERT INTO drift_p3 VALUES (21, 'three@example.com'); \
             INSERT INTO drift_kid VALUES (31, 'kid@example.com')",
        ))
        .execute(&mut other)
        .await
        .expect("setup");
        let rule = |table: &str| dbmcp_config::ColumnRule {
            name: table.into(),
            item: 0,
            key: ["app", "public", table, "email"].map(String::from),
            entity: "EMAIL_ADDRESS",
            operator: dbmcp_config::PiiOperator::Hash,
        };
        let config = PiiConfig {
            rules: vec![rule("drift_p2"), rule("drift_kid")],
            ..PiiConfig::default()
        };
        let read = "SELECT id, email FROM drift_p";

        let unruled = "ALTER TABLE drift_p ATTACH PARTITION drift_p3 FOR VALUES FROM (20) TO (30)";
        assert!(
            unchanged_across(&mut conn, &mut other, &config, read, unruled).await,
            "unruled attach"
        );
        let ruled = "ALTER TABLE drift_p ATTACH PARTITION drift_p2 FOR VALUES FROM (10) TO (20)";
        assert!(
            !unchanged_across(&mut conn, &mut other, &config, read, ruled).await,
            "ruled attach"
        );
        let detach = "ALTER TABLE drift_p DETACH PARTITION drift_p2";
        assert!(
            unchanged_across(&mut conn, &mut other, &config, read, detach).await,
            "detach"
        );
        let inherit = "ALTER TABLE drift_kid INHERIT drift_solo";
        let solo = "SELECT id, email FROM drift_solo";
        assert!(
            !unchanged_across(&mut conn, &mut other, &config, solo, inherit).await,
            "ruled inherit"
        );

        sqlx::raw_sql(SqlStr::from_static(reset))
            .execute(&mut other)
            .await
            .expect("cleanup");
    }

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn origins_translate_oids_and_attribute_numbers() {
        let mut conn = seeded().await;
        let sql = "SELECT name, email, 1 FROM users";
        let (_, owners) = resolve(&mut conn, sql).await.expect("resolve");
        let row = sqlx::query(sql).fetch_one(&mut conn).await.expect("row");
        let users = RelationId::new("app", "public", "users");
        assert_eq!(
            origins(row.columns(), &owners).expect("described"),
            [
                (0, users.clone(), Some("name".to_owned())),
                (1, users, Some("email".to_owned())),
            ]
        );
        assert!(matches!(
            origins(row.columns(), &Owners::new()),
            Err(Stop::Unresolved(0))
        ));
    }
}
