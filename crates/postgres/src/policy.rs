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
//!    bodies from `pg_get_viewdef`, for at most [`CATALOG_ROUNDS`] rounds,
//! 3. plans it with [`dbmcp_pii::policy::plan`]; a rejection runs nothing,
//! 4. executes it, refusing a result that does not fit the plan before any
//!    row is kept or a write commits,
//! 5. commits a write or rolls a read back.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors outside the value-free `SQLSTATE`
//! classes, `COMMIT` included, are replaced by a generic message.

use std::collections::HashMap;
use std::mem;

use dbmcp_config::PiiConfig;
use dbmcp_pii::cells::{Cells, ShapeError, check_columns};
use dbmcp_pii::policy::{CATALOG_ROUNDS, LineageClass, Plan, Position, Rejection, reject_lineage, rule, touches_ruled};
use dbmcp_server::truncate::Window;
use dbmcp_sql::{Connection as _, SqlError};
use dbmcp_sql_lineage::{
    Catalog, CatalogColumn, Dialect, Entry, Lexical, Lineage, LineageError, NamePart, Origin, Relation, RelationId,
    RelationKind, SourceRef, StatementKind, WrittenName, analyze,
};
use futures_util::TryStreamExt as _;
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlx::postgres::{PgColumn, PgQueryResult};
use sqlx::{AssertSqlSafe, Column as _, Either, Executor as _, Row as _, SqlSafeStr as _, SqlStr};
use sqlx_json::RowExt as _;

use crate::PostgresHandler;
use crate::connection::{PostgresConnection, quote_ident};

/// Reads the database, `standard_conforming_strings`, the first existing
/// schema of the search path, and the search path itself.
const PROBE_SQL: &str = "SELECT current_database()::text, \
     current_setting('standard_conforming_strings'), \
     (current_schemas(false))[1]::text, \
     current_setting('search_path')";

/// Resolves each `to_regclass` input of `$1`, with its explicit schema in `$2`.
///
/// Per name, in order: schema, name, `relkind`, OID, view body, column names
/// and numbers (system columns, numbered below zero, come last), and whether
/// the explicit schema exists.
const RESOLVE_SQL: &str = "SELECT n.ord, s.nspname::text, c.relname::text, c.relkind::text, c.oid::int8, \
     CASE WHEN c.relkind IN ('v', 'm') THEN pg_get_viewdef(c.oid) END, \
     ARRAY(SELECT a.attname::text FROM pg_attribute AS a \
           WHERE a.attrelid = c.oid AND a.attnum <> 0 AND NOT a.attisdropped \
           ORDER BY a.attnum < 0, a.attnum), \
     ARRAY(SELECT a.attnum FROM pg_attribute AS a \
           WHERE a.attrelid = c.oid AND a.attnum <> 0 AND NOT a.attisdropped \
           ORDER BY a.attnum < 0, a.attnum), \
     to_regnamespace(n.schema) IS NOT NULL \
     FROM unnest($1::text[], $2::text[]) WITH ORDINALITY AS n(name, schema, ord) \
     LEFT JOIN pg_class AS c ON c.oid = to_regclass(n.name) \
     LEFT JOIN pg_namespace AS s ON s.oid = c.relnamespace \
     ORDER BY n.ord";

/// `SQLSTATE` classes whose messages name schema objects or states, never values.
///
/// Syntax and access (`42`), unsupported features (`0A`), transaction state
/// (`25`), rollbacks (`40`), resources (`53`), limits (`54`), operator
/// intervention (`57`), and connections (`08`).
const VALUE_FREE_CLASSES: &[&str] = &["42", "0A", "25", "40", "53", "54", "57", "08"];

/// Class `42` codes withheld anyway.
///
/// `reg*` input casts (`ssn::regclass`) quote the cast text in syntax,
/// undefined-relation, -object, and -function errors; `invalid_name` quotes
/// configuration parameter and replication slot names taken from values.
const VALUE_ECHOING_CODES: &[&str] = &["42601", "42602", "42P01", "42704", "42883"];

/// Longest identifier in bytes; `PostgreSQL` truncates longer ones.
const MAX_IDENTIFIER_BYTES: usize = 63;

/// Reports whether a relation named `$2` exists in schema `$1`.
const EXISTS_SQL: &str = "SELECT EXISTS (SELECT 1 FROM pg_class AS c \
     JOIN pg_namespace AS s ON s.oid = c.relnamespace WHERE s.nspname = $1 AND c.relname = $2)";

/// Schemas whose relations are system catalogs, never expanded as views.
const SYSTEM_SCHEMAS: &[&str] = &["pg_catalog", "information_schema", "pg_toast"];

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
    /// The statement failed with an error that may echo a ruled value.
    #[error("query failed (SQLSTATE {0}): error detail withheld under column policy")]
    Withheld(String),
    /// Any other database error, reported as without column rules.
    #[error(transparent)]
    Sql(#[from] sqlx::Error),
}

struct Run {
    /// The plan when the result columns are the planned ones; `EXPLAIN`,
    /// `SHOW`, and `DESCRIBE` rows are detected globally instead.
    plan: Option<Plan>,
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
    i64,
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
        let rows: Vec<Vec<Value>> = mem::take(&mut window.rows).into_iter().map(values).collect();
        let rows = if let Some(plan) = run.plan {
            redactor.redact_cells(Cells::new(run.names, rows), &plan).await?
        } else {
            let rows = rows
                .into_iter()
                .map(|values| Value::Object(run.names.iter().cloned().zip(values).collect()))
                .collect();
            redactor.redact_rows(rows).await?
        };
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
async fn run(
    conn: &mut sqlx::PgConnection,
    config: &PiiConfig,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run, Stop> {
    let begin = match access {
        Access::Read => "START TRANSACTION READ ONLY",
        Access::Write => "BEGIN",
    };
    let mut tx = sqlx::Connection::begin_with(conn, SqlStr::from_static(begin)).await?;
    let (lineage, owners) = resolve(&mut tx, sql).await?;
    // A bare `CREATE … AS` target resolves through the whole `search_path`, but
    // the relation is created in the first schema: a name already taken
    // anywhere would check the rules of the wrong target.
    if let (StatementKind::CreateTableAs | StatementKind::CreateMaterializedView, Some(target)) =
        (&lineage.statement, &lineage.target)
    {
        let exists: bool = sqlx::query_scalar(EXISTS_SQL)
            .bind(&target.schema)
            .bind(&target.name)
            .fetch_one(&mut *tx)
            .await?;
        if exists {
            return Err(Rejection::Lineage(LineageClass::Unsupported).logged().into());
        }
    }
    let plan = dbmcp_pii::policy::plan(&lineage, Dialect::Postgres, config)?;
    let plan = (!matches!(
        lineage.statement,
        StatementKind::Explain(_) | StatementKind::Show(_) | StatementKind::Describe
    ))
    .then_some(plan);
    let failed = |error: sqlx::Error| match withheld(&error) {
        Some(code) if touches_ruled(&lineage, config) => Stop::Withheld(code),
        _ => Stop::Sql(error),
    };
    let executed = match execute(&mut tx, sql, plan.as_ref(), config, &owners, sink).await {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(failed(error)),
        Err(stop) => {
            tx.rollback().await?;
            return Err(stop);
        }
    };
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

/// Runs `sql`, feeding each row's values to `sink` and collecting its results.
///
/// With `plan`, the first row's column origins, count, and names are checked
/// before any row reaches `sink`, so a write that does not fit never commits.
async fn execute(
    conn: &mut sqlx::PgConnection,
    sql: &str,
    plan: Option<&Plan>,
    config: &PiiConfig,
    owners: &Owners,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, Stop> {
    let query = (AssertSqlSafe(sql.to_owned()).into_sql_str(), None);
    let mut stream = conn.fetch_many(query);
    let mut names = None;
    let mut results = Vec::new();
    while let Some(item) = stream.try_next().await? {
        match item {
            Either::Left(result) => results.push(result),
            Either::Right(row) => {
                let (row_names, values): (Vec<String>, Vec<Value>) = row.to_entries().into_iter().unzip();
                if names.is_none()
                    && let Some(plan) = plan
                {
                    check_origins(row.columns(), plan, config, owners)?;
                    check_columns(plan, &row_names)?;
                }
                names.get_or_insert(row_names);
                sink(Value::Array(values));
            }
        }
    }
    Ok(Executed {
        names: names.unwrap_or_default(),
        results,
    })
}

/// Refuses a result column whose origin is ruled but not planned as ruled.
///
/// `PostgreSQL` reports a column's origin as its table's OID and attribute
/// number, through subqueries and CTEs; a view column reports the view. An
/// origin the catalog reads did not describe is refused too.
fn check_origins(columns: &[PgColumn], plan: &Plan, config: &PiiConfig, owners: &Owners) -> Result<(), Stop> {
    for column in columns {
        let position = column.ordinal();
        let Some(oid) = column.relation_id() else {
            continue;
        };
        let name = owners.get(&oid.0).and_then(|owner| {
            let name = owner.columns.get(&column.relation_attribute_no()?)?;
            Some((&owner.id, name))
        });
        let Some((id, name)) = name else {
            return Err(Stop::Unresolved(position));
        };
        let ruled = rule(config, &SourceRef::new(id.clone(), name)).is_some();
        let planned = matches!(
            plan.columns.get(position).map(|planned| planned.position),
            Some(Position::Action(..))
        );
        if ruled && !planned {
            return Err(Stop::Unplanned(position));
        }
    }
    Ok(())
}

/// Analyzes `sql`, fetching missing relations for at most [`CATALOG_ROUNDS`] rounds.
///
/// Also returns the relations it described, by OID.
async fn resolve(conn: &mut sqlx::PgConnection, sql: &str) -> Result<(Lineage, Owners), Stop> {
    let (database, conforming, first, search_path): (String, String, Option<String>, String) =
        sqlx::query_as(PROBE_SQL).fetch_one(&mut *conn).await?;
    let probe = Probe {
        database,
        creation: first.filter(|_| !lists_temp(&search_path)),
    };
    let mut catalog = Catalog::new();
    if conforming != "on" {
        catalog.set_lexical(Lexical::Mismatch("standard_conforming_strings is off".into()));
    }
    let mut owners = Owners::new();
    let mut rounds = 0;
    loop {
        match analyze(sql, Dialect::Postgres, &catalog) {
            Ok(lineage) => return Ok((lineage, owners)),
            Err(LineageError::Missing(_)) if rounds == CATALOG_ROUNDS => {
                return Err(Rejection::CatalogRounds.logged().into());
            }
            Err(LineageError::Missing(names)) => fetch(conn, &probe, names, &mut catalog, &mut owners).await?,
            Err(error) => return Err(reject_lineage(&error).into()),
        }
        rounds += 1;
    }
}

/// Reports whether `search_path` lists a temporary schema.
///
/// The server creates bare names in `pg_temp` when it comes before every
/// existing schema, creating that schema on first use, so the first
/// existing schema would name the wrong creation target.
fn lists_temp(search_path: &str) -> bool {
    search_path.split(',').any(|element| {
        let element = element.trim();
        let name = element
            .strip_prefix('"')
            .and_then(|quoted| quoted.strip_suffix('"'))
            .map_or_else(|| element.to_ascii_lowercase(), str::to_owned);
        name.starts_with("pg_temp")
    })
}

/// Inserts a catalog entry for each of `names`, read in one batched query.
async fn fetch(
    conn: &mut sqlx::PgConnection,
    probe: &Probe,
    names: Vec<WrittenName>,
    catalog: &mut Catalog,
    owners: &mut Owners,
) -> Result<(), sqlx::Error> {
    let lookups: Vec<(WrittenName, Lookup)> = names
        .into_iter()
        .filter_map(|name| {
            if let Some(lookup) = Lookup::new(&name, &probe.database) {
                return Some((name, lookup));
            }
            catalog.insert(name, Entry::Absent);
            None
        })
        .collect();
    if lookups.is_empty() {
        return Ok(());
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
        let oid = row.4.and_then(|oid| u32::try_from(oid).ok());
        let columns: HashMap<i16, String> = row.7.iter().copied().zip(row.6.iter().cloned()).collect();
        let entry = entry(probe, &name, &lookup, row);
        if let (Entry::Resolved(relation), Some(oid)) = (&entry, oid) {
            owners.insert(
                oid,
                Owner {
                    id: relation.id.clone(),
                    columns,
                },
            );
        }
        catalog.insert(name, entry);
    }
    Ok(())
}

/// Builds the catalog entry of one resolved row.
fn entry(probe: &Probe, name: &WrittenName, lookup: &Lookup, row: Resolved) -> Entry {
    let (_, schema, relname, relkind, _, body, columns, numbers, schema_exists) = row;
    let (Some(schema), Some(relname), Some(relkind)) = (schema, relname, relkind) else {
        return vacant(probe, name, lookup, schema_exists);
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
        .into_iter()
        .zip(numbers)
        .map(|(column, number)| {
            if number > 0 {
                CatalogColumn::new(column)
            } else {
                CatalogColumn::hidden(column)
            }
        })
        .collect();
    let id = RelationId::new(&probe.database, &schema, &relname);
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
    if identifier.len() > MAX_IDENTIFIER_BYTES {
        let end = (0..=MAX_IDENTIFIER_BYTES)
            .rev()
            .find(|&end| identifier.is_char_boundary(end))
            .unwrap_or(0);
        identifier.truncate(end);
    }
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
    fn search_paths_listing_a_temporary_schema_are_detected() {
        for path in [
            "pg_temp, public",
            "\"$user\", pg_temp, public",
            "PG_TEMP",
            "public, pg_temp_3",
        ] {
            assert!(lists_temp(path), "{path}");
        }
        for path in ["\"$user\", public", "public", "\"PG_TEMP\"", "temp"] {
            assert!(!lists_temp(path), "{path}");
        }
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
            1,
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

    fn resolved(row: Resolved) -> Option<Relation> {
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
            let relation = resolved(row(relkind, "public", None)).expect(relkind);
            assert_eq!(relation.kind, kind, "{relkind}");
            assert_eq!(relation.id, RelationId::new(DB, "public", "r"));
        }
        for relkind in ["i", "c", "t"] {
            assert_eq!(resolved(row(relkind, "public", None)), None, "{relkind}");
        }
        let system = resolved(row("v", "pg_catalog", None)).expect("system view");
        assert_eq!(system.kind, RelationKind::System);
        assert_eq!(system.view_sql, None);
    }

    #[test]
    fn system_columns_are_hidden_from_star() {
        let relation = resolved(row("r", "public", None)).expect("table");
        assert_eq!(
            relation.columns,
            vec![CatalogColumn::new("id"), CatalogColumn::hidden("ctid")]
        );
    }

    #[test]
    fn views_carry_their_query_or_are_absent() {
        let view = resolved(row("v", "public", Some(" SELECT users.id\n   FROM users;"))).expect("view");
        assert_eq!(view.kind, RelationKind::View);
        assert_eq!(view.view_sql.as_deref(), Some("SELECT users.id\n   FROM users"));
        let matview = resolved(row("m", "public", Some(" SELECT 1;"))).expect("matview");
        assert_eq!(matview.kind, RelationKind::MatView);
        assert_eq!(resolved(row("v", "public", None)), None);
    }

    #[test]
    fn unresolved_rows_fall_back_to_vacant() {
        let name = WrittenName::query(vec![NamePart::bare("n")]);
        let lookup = Lookup::new(&name, DB).expect("lookup");
        let row: Resolved = (1, None, None, None, None, None, Vec::new(), Vec::new(), false);
        assert_eq!(
            entry(&probe(Some("public")), &name, &lookup, row),
            Entry::Vacant(RelationId::new(DB, "public", "n"))
        );
    }
}
