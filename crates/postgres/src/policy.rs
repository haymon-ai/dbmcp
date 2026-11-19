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
//!    it will when the statement runs), columns from `pg_attribute`, and view
//!    bodies from `pg_get_viewdef`, for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds,
//! 3. plans it with [`Analyzed::plan`]; a rejection runs nothing,
//! 4. executes it, refusing a result that does not fit the plan before any
//!    row is kept or a write commits,
//! 5. refuses the result when a view body it planned from has changed,
//! 6. commits a write or rolls a read back.
//!
//! `CREATE INDEX` runs outside the transaction once planned, since its
//! `CONCURRENTLY` form cannot run inside one.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors outside the value-free `SQLSTATE`
//! classes, `COMMIT` included, are replaced by a generic message.

use std::collections::HashMap;
use std::mem;

use dbmcp_config::{DatabaseBackend, PiiConfig};
use dbmcp_pii::cells::{Cells, ShapeError, check_columns};
use dbmcp_policy::{
    Analysis, Analyzed, CatalogColumn, Entry, NamePart, Origin, Planned, Rejection, Relation, RelationId, RelationKind,
    StatementKind, Step, WrittenName,
};
use dbmcp_server::truncate::Window;
use dbmcp_sql::{Connection as _, SqlError};
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlx::postgres::{PgColumn, PgQueryResult, PgRow};
use sqlx::{Column as _, Row as _, SqlStr};

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
/// and numbers (system columns, numbered below zero, come last), and whether
/// the explicit schema exists.
const RESOLVE_SQL: &str = "SELECT s.nspname::text, c.relname::text, c.relkind::text, c.oid::int8, \
     CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid) END, \
     coalesce(cols.names, '{}'), coalesce(cols.numbers, '{}'), \
     to_regnamespace(n.schema) IS NOT NULL \
     FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS n(name, schema, ord) \
     LEFT JOIN pg_class AS c ON c.oid = to_regclass(n.name) \
     LEFT JOIN pg_namespace AS s ON s.oid = c.relnamespace \
     LEFT JOIN LATERAL (SELECT array_agg(a.attname::text ORDER BY a.attnum < 0, a.attnum) AS names, \
           array_agg(a.attnum ORDER BY a.attnum < 0, a.attnum) AS numbers \
           FROM pg_attribute AS a \
           WHERE a.attrelid = c.oid AND a.attnum <> 0 AND NOT a.attisdropped) AS cols ON true \
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

/// Longest identifier in bytes; `PostgreSQL` truncates longer ones.
const MAX_IDENTIFIER_BYTES: usize = 63;

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
    Shape(#[from] ShapeError),
    /// A ruled column came back where the plan does not rewrite it; nothing was committed.
    #[error("column policy: result column {0} carries a ruled column the plan does not rewrite")]
    Unplanned(usize),
    /// A result column came from a relation the catalog reads did not describe.
    #[error("column policy: result column {0} comes from a relation the policy did not resolve")]
    Unresolved(usize),
    /// A view body changed between planning and execution; nothing was committed.
    #[error("column policy: a view changed while the statement ran")]
    ViewChanged,
    /// The statement failed with an error that may echo a ruled value.
    #[error("query failed (SQLSTATE {0}): error detail withheld under column policy")]
    Withheld(String),
    /// Any other database error, reported as without column rules.
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
}

struct Run<'c> {
    /// The plan when the result columns are the planned ones; `EXPLAIN`,
    /// `SHOW`, and `DESCRIBE` rows are detected globally instead.
    plan: Option<Planned<'c>>,
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
    /// Whether a bare name resolved here, outside the creation schema.
    shadowed: bool,
}

/// The relations a statement's result columns may come from, by OID.
type Owners = HashMap<u32, Owner>;

/// A written name as `PostgreSQL` reads it.
#[derive(Debug, PartialEq, Eq)]
struct Lookup {
    /// The explicit schema, case-folded.
    schema: Option<String>,
    /// The relation name, case-folded.
    name: String,
}

impl Lookup {
    /// Folds `written` like the server; `None` when it can name no relation here.
    ///
    /// Unquoted parts fold to ASCII lowercase. A database qualifier must be
    /// the connected `database`; four or more parts never resolve.
    fn new(written: &WrittenName, database: &str) -> Option<Self> {
        let fold = |part: &NamePart| {
            let folded = if part.quoted {
                part.value.clone()
            } else {
                part.value.to_ascii_lowercase()
            };
            truncate_identifier(folded)
        };
        let mut parts: Vec<String> = written.parts.iter().map(fold).collect();
        let name = parts.pop()?;
        let schema = parts.pop();
        match parts.as_slice() {
            [] => Some(Self { schema, name }),
            [catalog] if catalog == database => Some(Self { schema, name }),
            _ => None,
        }
    }

    /// Returns the quoted, qualified name `to_regclass` reads.
    fn regclass(&self) -> String {
        match &self.schema {
            Some(schema) => format!("{}.{}", quote_ident(schema), quote_ident(&self.name)),
            None => quote_ident(&self.name),
        }
    }
}

/// One row of [`RESOLVE_SQL`].
type Resolved = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Vec<String>,
    Vec<i16>,
    bool,
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
        let rows = mem::take(&mut window.rows).into_iter().map(values).collect();
        let rows = redactor
            .redact_cells(Cells::new(run.names, rows), run.plan.as_ref().map(Planned::plan))
            .await?;
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
    // A bare create target resolves through the whole `search_path`, but the
    // relation is created in the creation schema: a name taken further along
    // would check the rules of the wrong target.
    if let (
        StatementKind::CreateTableAs | StatementKind::CreateMaterializedView | StatementKind::CreateView,
        Some(target),
    ) = (analyzed.statement(), analyzed.target())
        && owners.values().any(|owner| owner.shadowed && owner.id == *target)
    {
        return Err(Rejection::shadowed(target).into());
    }
    let plan = analyzed.plan(config)?;
    if *analyzed.statement() == StatementKind::CreateIndex {
        tx.rollback().await?;
        return autocommit(conn, sql, &owners, sink).await;
    }
    let failed = |error: sqlx::Error| match withheld(&error) {
        Some(code) if analyzed.touches_ruled(config) => Stop::Withheld(code),
        _ => Stop::Sql(error),
    };
    let executed = match execute(&mut tx, sql, plan.as_ref(), &owners, sink).await {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(failed(error)),
        Err(stop) => {
            tx.rollback().await?;
            return Err(stop);
        }
    };
    if !views_unchanged(&mut tx, &owners, analyzed.target()).await? {
        tx.rollback().await?;
        return Err(Stop::ViewChanged);
    }
    // Deferred constraints fail at `COMMIT`, with messages as revealing.
    let finished = match access {
        Access::Read => tx.rollback().await,
        Access::Write => tx.commit().await,
    };
    finished.map_err(failed)?;
    let (rows_affected, last_insert_id) = PostgresConnection::counts(sql, &executed.results);
    Ok(Run {
        plan,
        names: executed.names,
        rows_affected,
        last_insert_id,
    })
}

/// Runs a planned `CREATE INDEX` outside a transaction block.
///
/// It returns no rows. Its names may rebind once the planning transaction
/// ends, so value-echoing errors are withheld whatever it touched.
async fn autocommit(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'static>, Stop> {
    let executed = execute(conn, sql, None, owners, sink)
        .await
        .map_err(|stop| match stop {
            Stop::Sql(error) => withheld(&error).map_or(Stop::Sql(error), Stop::Withheld),
            other => other,
        })?;
    let (rows_affected, last_insert_id) = PostgresConnection::counts(sql, &executed.results);
    Ok(Run {
        plan: None,
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

/// Runs `sql`, feeding each row's values to `sink` and collecting its results.
///
/// With `plan`, the first row's column origins, count, and names are checked
/// before any row reaches `sink`, so a write that does not fit never commits.
async fn execute(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    planned: Option<&Planned<'_>>,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, Stop> {
    let first = |row: &PgRow, names: &[String]| {
        if let Some(planned) = planned {
            check_origins(row.columns(), planned, owners)?;
            check_columns(planned.plan(), names)?;
        }
        Ok::<_, Stop>(())
    };
    let (names, results) = PostgresConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed { names, results })
}

/// Refuses a result column whose origin is ruled but not planned as ruled.
///
/// `PostgreSQL` reports a column's origin as its table's OID and attribute
/// number, through subqueries and CTEs; a view column reports the view. A
/// whole-row reference reports attribute zero and is ruled when any column
/// is. An origin the catalog reads did not describe is refused too.
fn check_origins(columns: &[PgColumn], planned: &Planned<'_>, owners: &Owners) -> Result<(), Stop> {
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
                Some(name) => Some(name.as_str()),
                None => return Err(Stop::Unresolved(position)),
            },
            None => None,
        };
        if !planned.admits(position, &owner.id, name) {
            return Err(Stop::Unplanned(position));
        }
    }
    Ok(())
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
    names: Vec<WrittenName>,
    owners: &mut Owners,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    let mut entries = Vec::new();
    let lookups: Vec<(WrittenName, Lookup)> = names
        .into_iter()
        .filter_map(|name| {
            if let Some(lookup) = Lookup::new(&name, &probe.database) {
                return Some((name, lookup));
            }
            entries.push((name, Entry::Absent));
            None
        })
        .collect();
    if lookups.is_empty() {
        return Ok(entries);
    }
    let regclasses: Vec<String> = lookups.iter().map(|(_, lookup)| lookup.regclass()).collect();
    let schemas: Vec<Option<String>> = lookups
        .iter()
        .map(|(_, lookup)| lookup.schema.as_deref().map(quote_ident))
        .collect();
    let rows: Vec<Resolved> = sqlx::query_as(RESOLVE_SQL)
        .bind(regclasses)
        .bind(schemas)
        .fetch_all(&mut *conn)
        .await?;
    for ((name, lookup), row) in lookups.into_iter().zip(rows) {
        let entry = entry(probe, &name, &lookup, &row);
        let (_, _, _, oid, body, names, numbers, _) = row;
        if let (Entry::Resolved(relation), Some(oid)) = (&entry, oid.and_then(|oid| u32::try_from(oid).ok())) {
            let shadowed = lookup.schema.is_none() && probe.creation.as_ref() != Some(&relation.id.schema);
            owners
                .entry(oid)
                .or_insert_with(|| Owner {
                    id: relation.id.clone(),
                    columns: numbers.into_iter().zip(names).collect(),
                    view: body,
                    shadowed: false,
                })
                .shadowed |= shadowed;
        }
        entries.push((name, entry));
    }
    Ok(entries)
}

/// Builds the catalog entry of one resolved row.
fn entry(probe: &Probe, name: &WrittenName, lookup: &Lookup, row: &Resolved) -> Entry {
    let (schema, relname, relkind, _, body, columns, numbers, schema_exists) = row;
    let (Some(schema), Some(relname), Some(relkind)) = (schema, relname, relkind) else {
        return vacant(probe, name, lookup, *schema_exists);
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
        .map(|(column, &number)| {
            if number > 0 {
                CatalogColumn::new(column.clone())
            } else {
                CatalogColumn::hidden(column.clone())
            }
        })
        .collect();
    let id = RelationId::new(&probe.database, schema, relname);
    let relation = Relation::new(id, kind, columns);
    if !matches!(kind, RelationKind::View | RelationKind::MatView) {
        return Entry::Resolved(relation);
    }
    match body.as_deref().map(view_query) {
        Some(query) if !query.is_empty() => Entry::Resolved(relation.with_view_sql(query)),
        _ => Entry::Absent,
    }
}

/// Returns the entry for a name nothing resolves to.
///
/// A relation created under a bare name lands in the session's creation
/// schema; under a qualified name, in that schema when it exists. Temporary
/// schemas, names in view bodies, and names without a schema are absent.
fn vacant(probe: &Probe, name: &WrittenName, lookup: &Lookup, schema_exists: bool) -> Entry {
    if name.context != Origin::Query {
        return Entry::Absent;
    }
    let schema = match &lookup.schema {
        None => probe.creation.as_deref(),
        Some(schema) => schema_exists.then_some(schema.as_str()),
    };
    schema
        .filter(|schema| !schema.starts_with("pg_temp") && !schema.starts_with("pg_toast_temp"))
        .map_or(Entry::Absent, |schema| {
            Entry::Vacant(RelationId::new(&probe.database, schema, &lookup.name))
        })
}

/// Cuts `identifier` to [`MAX_IDENTIFIER_BYTES`] on a character boundary, as the server does.
fn truncate_identifier(mut identifier: String) -> String {
    identifier.truncate(identifier.floor_char_boundary(MAX_IDENTIFIER_BYTES));
    identifier
}

/// Strips the whitespace and trailing semicolon `pg_get_viewdef` wraps a query in.
fn view_query(body: &str) -> String {
    let body = body.trim();
    body.strip_suffix(';').unwrap_or(body).trim().to_owned()
}

/// Returns the `SQLSTATE` of a database error that may echo a value.
///
/// Timeouts and non-database errors are never withheld.
fn withheld(error: &sqlx::Error) -> Option<String> {
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
    use dbmcp_config::{ColumnRule, PiiOperator};

    use super::*;

    const DB: &str = "app";

    fn lookup(parts: Vec<NamePart>) -> Option<Lookup> {
        Lookup::new(&WrittenName::query(parts), DB)
    }

    #[test]
    fn unquoted_parts_fold_and_quoted_parts_stay() {
        let found = lookup(vec![NamePart::bare("Public"), NamePart::quoted("Users")]).expect("two parts");
        assert_eq!(
            found,
            Lookup {
                schema: Some("public".into()),
                name: "Users".into(),
            }
        );
        assert_eq!(found.regclass(), "\"public\".\"Users\"");
        let bare = lookup(vec![NamePart::bare("USERS")]).expect("one part");
        assert_eq!(bare.regclass(), "\"users\"");
    }

    #[test]
    fn quotes_inside_names_are_escaped() {
        let found = lookup(vec![NamePart::quoted("a\"b")]).expect("one part");
        assert_eq!(found.regclass(), "\"a\"\"b\"");
    }

    #[test]
    fn database_qualifier_must_be_the_connected_database() {
        let three = |catalog: NamePart| lookup(vec![catalog, NamePart::bare("public"), NamePart::bare("users")]);
        assert_eq!(
            three(NamePart::bare("APP")),
            Some(Lookup {
                schema: Some("public".into()),
                name: "users".into(),
            })
        );
        assert_eq!(three(NamePart::quoted("APP")), None);
        assert_eq!(three(NamePart::bare("other")), None);
        let four = vec![
            NamePart::bare("app"),
            NamePart::bare("a"),
            NamePart::bare("b"),
            NamePart::bare("c"),
        ];
        assert_eq!(lookup(four), None);
    }

    #[test]
    fn long_identifiers_truncate_to_63_bytes_on_char_boundaries() {
        let long = lookup(vec![NamePart::bare("a".repeat(70))]).expect("one part");
        assert_eq!(long.name, "a".repeat(63));
        let wide = lookup(vec![NamePart::quoted(format!("{}é", "b".repeat(62)))]).expect("one part");
        assert_eq!(wide.name, "b".repeat(62), "é straddles byte 63");
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

    fn vacant_id(probe: &Probe, name: &WrittenName, exists: bool) -> Option<String> {
        let lookup = Lookup::new(name, DB).expect("lookup");
        match vacant(probe, name, &lookup, exists) {
            Entry::Vacant(id) => Some(format!("{}.{}.{}", id.catalog, id.schema, id.name)),
            _ => None,
        }
    }

    #[test]
    fn bare_creates_land_in_the_creation_schema() {
        let name = WrittenName::query(vec![NamePart::bare("New")]);
        assert_eq!(
            vacant_id(&probe(Some("public")), &name, false).as_deref(),
            Some("app.public.new")
        );
        assert_eq!(vacant_id(&probe(None), &name, false), None);
        assert_eq!(vacant_id(&probe(Some("pg_temp_3")), &name, false), None);
    }

    #[test]
    fn qualified_creates_need_an_existing_non_temporary_schema() {
        let name = |schema: &str| WrittenName::query(vec![NamePart::bare(schema), NamePart::bare("n")]);
        let probe = probe(Some("public"));
        assert_eq!(
            vacant_id(&probe, &name("shadow"), true).as_deref(),
            Some("app.shadow.n")
        );
        assert_eq!(vacant_id(&probe, &name("nope"), false), None);
        assert_eq!(vacant_id(&probe, &name("PG_TEMP"), true), None);
    }

    #[test]
    fn names_in_view_bodies_are_never_vacant() {
        let name = WrittenName {
            parts: vec![NamePart::bare("n")],
            context: Origin::View(RelationId::new(DB, "public", "v")),
        };
        assert_eq!(vacant_id(&probe(Some("public")), &name, false), None);
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
        )
    }

    fn resolved(row: &Resolved) -> Option<Relation> {
        let name = WrittenName::query(vec![NamePart::bare("r")]);
        let lookup = Lookup::new(&name, DB).expect("lookup");
        match entry(&probe(Some("public")), &name, &lookup, row) {
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
            vec![CatalogColumn::new("id"), CatalogColumn::hidden("ctid")]
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
        let lookup = Lookup::new(&name, DB).expect("lookup");
        let row: Resolved = (None, None, None, None, None, Vec::new(), Vec::new(), false);
        assert_eq!(
            entry(&probe(Some("public")), &name, &lookup, &row),
            Entry::Vacant(RelationId::new(DB, "public", "n"))
        );
    }

    /// Rules `app.public.users.email` of the functional seed.
    fn email_ruled() -> PiiConfig {
        PiiConfig {
            rules: vec![ColumnRule {
                name: "email".into(),
                item: 0,
                key: ["app", "public", "users", "email"].map(String::from),
                entity: "EMAIL_ADDRESS",
                operator: PiiOperator::Hash,
            }],
            ..PiiConfig::default()
        }
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

    #[tokio::test]
    #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
    async fn origin_check_refuses_ruled_columns_at_global_positions() {
        let mut conn = seeded().await;
        let rules = email_ruled();
        // Each plan comes from another statement, so it disagrees with the row's origins.
        for (planned_sql, unplanned) in [
            ("SELECT name, name FROM users", Some(1)),
            ("SELECT name, email FROM users", None),
            ("SELECT name FROM users", Some(1)),
        ] {
            let (analyzed, owners) = resolve(&mut conn, planned_sql).await.expect("resolve");
            let planned = analyzed.plan(&rules).expect("plan").expect("positional");
            let mut sunk = 0;
            let executed = execute(
                &mut conn,
                "SELECT name, email FROM users",
                Some(&planned),
                &owners,
                &mut |_| sunk += 1,
            )
            .await;
            match unplanned {
                Some(position) => {
                    assert!(
                        matches!(executed, Err(Stop::Unplanned(at)) if at == position),
                        "{planned_sql}"
                    );
                    assert_eq!(sunk, 0, "{planned_sql}");
                }
                None => {
                    executed.expect("admitted");
                }
            }
        }
    }
}
