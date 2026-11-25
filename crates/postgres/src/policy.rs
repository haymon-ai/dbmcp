//! Column rule (`privacy.rules`) enforcement on `PostgreSQL`.
//!
//! With column rules, `read`, `write`, and `explain` run each
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

use dbmcp_policy::{
    Access, Analysis, Analyzed, Cascade, CascadeAction, CatalogColumn, Enforced, Entry, Executed, Family, Member,
    Origin, Planned, Relation, RelationId, RelationKind, RelationRef, Run, SourceRef, StatementKind, Step, WrittenName,
    admit, settle,
};
use dbmcp_sql::Connection as _;
use dbmcp_truncate::Window;
use dbmcp_types::{database, privacy};
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlx::postgres::{PgColumn, PgRow};
use sqlx::{Column as _, Row as _};

use crate::PostgresHandler;
use crate::connection::PostgresConnection;
use crate::queries;
use crate::queries::policy::{Resolved, Writes};

/// `SQLSTATE` codes whose messages name schema objects, states, or fixed limits, never values.
///
/// Connections (`08`), transaction state (`25`), rollbacks (`40`), syntax and
/// access (`42`), resources (`53`), the statement, column, and argument limits
/// of `54`, and operator intervention (`57`), as listed in `errcodes.txt` of
/// `PostgreSQL` 18. Any other code, a new one included, is withheld.
///
/// Not every code of those classes: `reg*` input casts (`ssn::regclass`)
/// quote the cast text in syntax (`42601`), undefined-relation (`42P01`),
/// -object (`42704`), -function (`42883`), and ambiguous-function (`42725`)
/// errors, and `invalid_name` (`42602`) quotes configuration parameter and
/// replication slot names taken from values. Nor `54000`, whose `tsvector`
/// and `tsquery` input errors quote the whole input or its byte count; nor
/// unsupported features (`0A`), where a `reg*` cast of a three-part value
/// quotes it as a cross-database reference.
const VALUE_FREE_CODES: &[&str] = &[
    "08000", "08001", "08003", "08004", "08006", "08007", "08P01", "25000", "25001", "25002", "25003", "25004",
    "25005", "25006", "25007", "25008", "25P01", "25P02", "25P03", "25P04", "40000", "40001", "40002", "40003",
    "40P01", "42000", "42501", "42611", "42622", "42701", "42702", "42703", "42710", "42712", "42723", "42803",
    "42804", "42809", "42830", "42846", "42939", "428C9", "42P02", "42P03", "42P04", "42P05", "42P06", "42P07",
    "42P08", "42P09", "42P10", "42P11", "42P12", "42P13", "42P14", "42P15", "42P16", "42P17", "42P18", "42P19",
    "42P20", "42P21", "42P22", "53000", "53100", "53200", "53300", "53400", "54001", "54011", "54023", "57000",
    "57014", "57P01", "57P02", "57P03", "57P04", "57P05",
];

/// Schemas whose relations are system catalogs, never expanded as views.
///
/// Not `pg_toast`: TOAST relations hold the raw out-of-line values of
/// their tables, ruled columns included, so they stay absent.
const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema"];

/// Why a statement returned no rows under the column rules.
type Stop = dbmcp_policy::Stop<Refusal>;

/// The refusals only `PostgreSQL` enforcement makes.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    /// A result column came from a relation the catalog reads did not describe.
    #[error("column policy: result column {0} comes from a relation the policy did not resolve")]
    Unresolved(usize),
    /// A view body changed between planning and execution; nothing was committed.
    #[error("column policy: a view changed while the statement ran")]
    ViewChanged,
    /// A ruled relation joined an inheritance tree the statement read; nothing was committed.
    #[error("column policy: an inheritance tree changed while the statement ran")]
    FamilyChanged,
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
        policy: &privacy::Config,
        sql: &str,
        access: Access,
        timeout_secs: u64,
        database: Option<&str>,
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        let admitted = admit(self.redactor.as_ref(), access, self.connection.read_only())?;
        let ran = self
            .connection
            .with_connection(sql, database, timeout_secs, async |conn| {
                Stop::split(run(conn, policy, sql, access, &mut |row| window.push(row)).await)
            })
            .await;
        Ok(admitted.finish(ran, window).await?)
    }
}

/// Plans and executes one statement inside a transaction on `conn`.
async fn run<'c>(
    conn: &mut sqlx::PgConnection,
    config: &'c privacy::Config,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let mut tx = queries::policy::begin(&mut *conn, access).await?;
    let (analyzed, owners) = resolve(&mut tx, sql).await?;
    let mut planned = analyzed.plan(config)?;
    if *analyzed.statement() == StatementKind::CreateIndex {
        tx.rollback().await?;
        return autocommit(conn, sql, planned, &owners, sink).await;
    }
    let executed = async {
        let executed = execute(&mut tx, sql, &mut planned, &owners, sink).await?;
        if !views_unchanged(&mut tx, &owners, analyzed.target()).await? {
            return Err(Stop::Refused(Refusal::ViewChanged));
        }
        if !families_unchanged(&mut tx, &owners, &planned).await? {
            return Err(Stop::Refused(Refusal::FamilyChanged));
        }
        Ok(executed)
    }
    .await;
    settle(Some(tx), access, planned, echoing, executed).await
}

/// Runs a planned `CREATE INDEX` outside a transaction block; it returns no rows.
async fn autocommit<'c>(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    mut planned: Planned<'c>,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let executed = execute(conn, sql, &mut planned, owners, sink).await;
    settle::<sqlx::Postgres, _>(None, Access::Write, planned, echoing, executed).await
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
    let current = queries::policy::views(conn, oids).await?;
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
    let current = queries::policy::family(conn, oids).await?;
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
    let (names, rows_affected, last_insert_id) = PostgresConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed {
        names,
        rows_affected,
        last_insert_id,
    })
}

/// Translates each result column's OID and attribute number into its origin.
///
/// A view column reports the view; a whole-row reference has no column.
///
/// # Errors
///
/// Returns [`Refusal::Unresolved`] for an origin the catalog reads did not describe.
fn origins(columns: &[PgColumn], owners: &Owners) -> Result<Vec<(usize, RelationId, Option<String>)>, Stop> {
    let mut origins = Vec::new();
    for column in columns {
        let position = column.ordinal();
        let Some(oid) = column.relation_id() else {
            continue;
        };
        let Some(owner) = owners.get(&oid.0) else {
            return Err(Stop::Refused(Refusal::Unresolved(position)));
        };
        let name = match column.relation_attribute_no() {
            Some(number) => match owner.columns.get(&number) {
                Some(name) => Some(name.clone()),
                None => return Err(Stop::Refused(Refusal::Unresolved(position))),
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
    let (database, conforming, first, search_path, user) = queries::policy::probe(&mut *conn).await?;
    let probe = Probe {
        database,
        creation: first.filter(|first| !temp_first(&search_path, first, &user)),
    };
    let mut analysis = Analysis::new(sql, database::Backend::Postgres);
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
    let references: Vec<(Option<&str>, &str)> = names
        .iter()
        .map(|(_, reference)| (reference.schema.as_deref(), reference.name.as_str()))
        .collect();
    let rows = queries::policy::resolve(&mut *conn, &references).await?;
    let mut entries = Vec::new();
    for ((name, reference), (row, writes)) in names.into_iter().zip(rows) {
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

/// Labels the `SQLSTATE` of a database error that may echo a value.
///
/// Timeouts and non-database errors return `None`.
fn echoing(error: &sqlx::Error) -> Option<String> {
    if PostgresConnection::is_timeout(error) {
        return None;
    }
    let code = error.as_database_error()?.code()?;
    (!value_free(&code)).then(|| format!("SQLSTATE {code}"))
}

/// Whether messages of the `SQLSTATE` `code` never carry a value.
fn value_free(code: &str) -> bool {
    VALUE_FREE_CODES.contains(&code)
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
    fn value_echoing_codes_are_withheld() {
        for code in [
            "54000", "0A000", "22P02", "42601", "42602", "42P01", "42704", "42725", "42883", "XX000", "42ZZZ",
        ] {
            assert!(!value_free(code), "{code}");
        }
        for code in [
            "42501", "42703", "25006", "40001", "53200", "54001", "54011", "54023", "57014", "08P01",
        ] {
            assert!(value_free(code), "{code}");
        }
    }

    #[test]
    fn value_free_codes_are_unique_and_well_formed() {
        let mut codes = VALUE_FREE_CODES.to_vec();
        for code in &codes {
            assert_eq!(code.len(), 5, "{code}");
            assert!(
                code.bytes().all(|b| b.is_ascii_digit() || b.is_ascii_uppercase()),
                "{code}"
            );
        }
        let total = codes.len();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), total, "duplicate code");
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

    /// Tests against the seeded database; `#[ignore]`d outside `./tests/run.sh` and CI.
    mod db {
        use sqlx::SqlStr;

        use super::*;

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
        async fn seeded_relation(conn: &mut sqlx::PgConnection, name: &str) -> Relation {
            let probe = probe(Some("public"));
            let names = vec![(WrittenName::query(vec![NamePart::bare(name)]), reference(None, name))];
            let entries = fetch(conn, &probe, names, &mut Owners::new()).await.expect("fetch");
            match entries.into_iter().next() {
                Some((_, Entry::Resolved(relation))) => relation,
                other => panic!("{name}: {other:?}"),
            }
        }

        #[tokio::test]
        #[ignore = "needs the seeded PostgreSQL container: ./tests/run.sh --filter postgres"]
        async fn resolve_reads_inheritance_and_partition_trees() {
            let mut conn = seeded().await;
            assert_eq!(
                seeded_relation(&mut conn, "ptab").await.family,
                Family {
                    partitioned: true,
                    ancestors: BTreeSet::new(),
                    descendants: vec![member("ptab_1", false, &["ptab"]), member("ptab_2", false, &["ptab"])],
                }
            );
            assert_eq!(
                seeded_relation(&mut conn, "ptab_2").await.family,
                Family {
                    partitioned: false,
                    ancestors: ids(&["ptab"]),
                    descendants: Vec::new(),
                }
            );
            let grandchild = member("inh_grandchild", false, &["inh_child", "inh_parent", "inh_other"]);
            assert_eq!(
                seeded_relation(&mut conn, "inh_parent").await.family,
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
                seeded_relation(&mut conn, "inh_child").await.family,
                Family {
                    partitioned: false,
                    ancestors: ids(&["inh_parent", "inh_other"]),
                    descendants: vec![grandchild],
                }
            );
            assert_eq!(seeded_relation(&mut conn, "users").await.family, Family::default());
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
            let mut facts = Vec::new();
            for name in ["facts_plain", "facts_p"] {
                facts.extend(
                    seeded_relation(&mut tx, name)
                        .await
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
            let mut relations = Vec::new();
            for name in ["fk_p", "fk_c", "fk_r", "fk_t", "user_emails"] {
                relations.push(seeded_relation(&mut tx, name).await);
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
            config: &privacy::Config,
            sql: &str,
            change: &'static str,
        ) -> bool {
            let mut tx = queries::policy::begin(&mut *conn, Access::Read).await.expect("begin");
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
            let rule = |table: &str| dbmcp_types::privacy::ColumnRule {
                name: table.into(),
                item: 0,
                key: ["app", "public", table, "email"].map(String::from),
                entity: dbmcp_types::privacy::Entity::EmailAddress,
                operator: dbmcp_types::privacy::Operator::Hash,
            };
            let config = privacy::Config {
                rules: vec![rule("drift_p2"), rule("drift_kid")],
                ..privacy::Config::default()
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
                Err(Stop::Refused(Refusal::Unresolved(0)))
            ));
        }
    }
}
