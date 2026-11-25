//! `privacy.rules` catalog reads and transaction start on `PostgreSQL`.
//!
//! Every function takes the connection the planner holds and returns raw
//! rows; folding them into catalog entries stays in `crate::policy`.

use sqlx::postgres::PgRow;
use sqlx::{FromRow as _, Row as _, SqlStr, Transaction};

use super::ident;

/// Starts a read-only transaction.
const BEGIN_READ: &str = "START TRANSACTION READ ONLY";

/// Starts a read-write transaction.
const BEGIN_WRITE: &str = "BEGIN";

/// Starts the transaction a planned statement runs in; `write` allows writes.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed transaction start.
pub(crate) async fn begin(
    conn: &mut sqlx::PgConnection,
    write: bool,
) -> Result<Transaction<'_, sqlx::Postgres>, sqlx::Error> {
    let begin = if write { BEGIN_WRITE } else { BEGIN_READ };
    sqlx::Connection::begin_with(conn, SqlStr::from_static(begin)).await
}

/// Reads the session state that decides how names resolve.
///
/// The database, `standard_conforming_strings`, the first existing schema
/// of the search path, the search path itself, and the current user.
const PROBE: &str = "SELECT current_database()::text, \
     current_setting('standard_conforming_strings'), \
     (current_schemas(false))[1]::text, \
     current_setting('search_path'), current_user::text";

/// One row of [`PROBE`]: database, `standard_conforming_strings`, first schema, search path, user.
pub(crate) type Probe = (String, String, Option<String>, String, String);

/// Reads the session state that decides how names resolve.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn probe(conn: &mut sqlx::PgConnection) -> Result<Probe, sqlx::Error> {
    sqlx::query_as(PROBE).fetch_one(conn).await
}

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
/// `Relation::cascades` asks for. `UNION` ends it on cycles.
const RESOLVE: &str = "SELECT s.nspname::text, c.relname::text, c.relkind::text, c.oid::int8, \
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

/// The first 16 columns of one [`RESOLVE`] row; [`Writes`] holds the rest.
pub(crate) type Resolved = (
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

/// The write facts of one [`RESOLVE`] row, after its first 16 columns.
pub(crate) type Writes = (
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

/// Returns the quoted, qualified name `to_regclass` reads for `name` in `schema`.
fn regclass(schema: Option<&str>, name: &str) -> String {
    match schema {
        Some(schema) => format!("{}.{}", ident::quote(schema), ident::quote(name)),
        None => ident::quote(name),
    }
}

/// Resolves each `(schema, name)` of `names` in one batched query, in order.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query or an undecodable row.
pub(crate) async fn resolve(
    conn: &mut sqlx::PgConnection,
    names: &[(Option<&str>, &str)],
) -> Result<Vec<(Resolved, Writes)>, sqlx::Error> {
    let regclasses: Vec<String> = names.iter().map(|&(schema, name)| regclass(schema, name)).collect();
    let schemas: Vec<Option<String>> = names.iter().map(|&(schema, _)| schema.map(ident::quote)).collect();
    let rows: Vec<PgRow> = sqlx::query(RESOLVE)
        .bind(regclasses)
        .bind(schemas)
        .fetch_all(conn)
        .await?;
    rows.iter()
        .map(|row| {
            let writes = writes(row)?;
            Ok((Resolved::from_row(row)?, writes))
        })
        .collect()
}

/// Reads the [`Writes`] columns of one [`RESOLVE`] row.
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

/// Re-reads the inheritance closure of each relation OID in `$1`, in order.
///
/// The same walk as [`RESOLVE`]: per OID, the schema and name of every
/// descendant and every ancestor of the relation or a descendant, the
/// relation itself included. A relation gone since is an empty closure.
const FAMILY: &str = "SELECT coalesce(tree.schemas, '{}'), coalesce(tree.names, '{}') \
     FROM unnest($1::int8[]) WITH ORDINALITY AS o(oid, ord) \
     LEFT JOIN LATERAL (WITH RECURSIVE \
           down(oid) AS (SELECT o.oid::oid UNION SELECT i.inhrelid FROM pg_inherits AS i JOIN down ON i.inhparent = down.oid), \
           up(oid) AS (SELECT down.oid FROM down UNION SELECT i.inhparent FROM pg_inherits AS i JOIN up ON i.inhrelid = up.oid) \
           SELECT array_agg(rs.nspname::text ORDER BY r.oid) AS schemas, \
             array_agg(r.relname::text ORDER BY r.oid) AS names \
           FROM up JOIN pg_class AS r ON r.oid = up.oid \
           JOIN pg_namespace AS rs ON rs.oid = r.relnamespace) AS tree ON true \
     ORDER BY o.ord";

/// Re-reads the `(schemas, names)` inheritance closure of each OID of `oids`, in order.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn family(
    conn: &mut sqlx::PgConnection,
    oids: Vec<i64>,
) -> Result<Vec<(Vec<String>, Vec<String>)>, sqlx::Error> {
    sqlx::query_as(FAMILY).bind(oids).fetch_all(conn).await
}

/// Reads the current body of each view OID in `$1`, in order.
const VIEWS: &str = "SELECT pg_get_viewdef(v.oid::oid) \
     FROM unnest($1::int8[]) WITH ORDINALITY AS v(oid, ord) ORDER BY v.ord";

/// Reads the current body of each view OID of `oids`, in order.
///
/// # Errors
///
/// Returns the [`sqlx::Error`] of a failed query.
pub(crate) async fn views(conn: &mut sqlx::PgConnection, oids: Vec<i64>) -> Result<Vec<Option<String>>, sqlx::Error> {
    sqlx::query_scalar(VIEWS).bind(oids).fetch_all(conn).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regclass_quotes_each_part() {
        assert_eq!(regclass(Some("public"), "Users"), "\"public\".\"Users\"");
        assert_eq!(regclass(None, "users"), "\"users\"");
        assert_eq!(regclass(None, "a\"b"), "\"a\"\"b\"");
    }
}
