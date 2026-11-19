//! Column rule (`pii.rules`) enforcement on `MySQL` and `MariaDB`.
//!
//! With column rules, `readQuery`, `writeQuery`, and `explainQuery` run each
//! statement through [`MysqlHandler::enforce`]. On one pooled connection of
//! the target database and under one deadline, it:
//!
//! 1. probes the session: the default database, `lower_case_table_names`,
//!    and whether `sql_mode` and the client charset let the parser read
//!    quotes and escapes as the server does,
//! 2. analyzes the statement's lineage, resolving the names it reports
//!    missing in `information_schema` (`TABLES`, `COLUMNS` with `INVISIBLE`
//!    columns hidden from `*`, `VIEWS` bodies) for at most
//!    [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds,
//! 3. plans it with [`Analyzed::plan`]; a rejection runs nothing,
//! 4. refuses a write that reads a view but cannot roll back,
//! 5. opens a transaction (read-only for reads) and executes it, refusing a
//!    result that does not fit the plan before any row is kept,
//! 6. refuses the result when a view body it planned from has changed,
//! 7. commits a write or rolls a read back.
//!
//! Steps 1–4 run before the transaction: `MySQL` serves `information_schema`
//! from the transaction's snapshot, so a body read inside it would never
//! show a later change. The statement takes the snapshot once it holds its
//! views' metadata locks, so the re-read in step 6 sees the body that ran.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors outside the value-free error numbers,
//! `COMMIT` included, are replaced by a generic message.

use std::collections::BTreeMap;
use std::mem;

use dbmcp_config::{DatabaseBackend, PiiConfig};
use dbmcp_policy::{
    Analysis, Analyzed, CatalogColumn, Entry, Origin, Planned, Rejection, Relation, RelationId, RelationKind,
    StatementKind, Step, Unfit, WrittenName,
};
use dbmcp_server::truncate::Window;
use dbmcp_sql::validation::validate_single_statement;
use dbmcp_sql::{Connection as _, SqlError};
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlparser::dialect::MySqlDialect;
use sqlparser::tokenizer::{Token, Tokenizer};
use sqlx::mysql::{MySqlColumn, MySqlDatabaseError, MySqlQueryResult, MySqlRow};
use sqlx::{AssertSqlSafe, Column as _, ColumnOrigin, Row as _, SqlStr};

use crate::MysqlHandler;
use crate::connection::MysqlConnection;

/// Catalog name of every `MySQL` relation in rule keys (`TABLE_CATALOG`).
const CATALOG: &str = "def";

/// Reads the default database, `lower_case_table_names`, `sql_mode`, and the client charset.
const PROBE_SQL: &str = "SELECT CAST(DATABASE() AS CHAR), \
     CAST(@@lower_case_table_names AS SIGNED), CAST(@@sql_mode AS CHAR), \
     CAST(@@character_set_client AS CHAR)";

/// `sql_mode` flags under which the server lexes quotes or escapes unlike the parser.
const LEXICAL_MODES: &[&str] = &["ANSI_QUOTES", "NO_BACKSLASH_ESCAPES"];

/// Client charsets whose multibyte characters may end in a `\` byte, hiding an escape.
const LEXICAL_CHARSETS: &[&str] = &["big5", "cp932", "gb18030", "gbk", "sjis"];

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

/// One relation's columns, bound with its exact schema and name.
const COLUMNS_BRANCH: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(COLUMN_NAME AS CHAR), CAST(EXTRA AS CHAR), CAST(ORDINAL_POSITION AS SIGNED), \
     CAST(NULL AS CHAR) FROM information_schema.COLUMNS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// One view's body, bound with its exact schema and name, shaped like [`COLUMNS_BRANCH`].
const VIEW_BRANCH: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(NULL AS CHAR), CAST(NULL AS CHAR), CAST(0 AS SIGNED), \
     CAST(VIEW_DEFINITION AS CHAR) FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// Orders the detail rows by relation, then column position.
const DETAILS_ORDER: &str = " ORDER BY 1, 2, 5";

/// One view's current body, bound with its exact schema and name.
const VIEW_CURRENT: &str = "(SELECT CAST(TABLE_SCHEMA AS CHAR), CAST(TABLE_NAME AS CHAR), \
     CAST(VIEW_DEFINITION AS CHAR) FROM information_schema.VIEWS WHERE TABLE_SCHEMA = ? AND TABLE_NAME = ?)";

/// Lists the schemas whose lowercased name is in the filter.
const SCHEMAS_SQL: &str = "SELECT CAST(SCHEMA_NAME AS CHAR) FROM information_schema.SCHEMATA WHERE ";

/// One schema filter of [`SCHEMAS_SQL`], bound lowercased.
const SCHEMAS_ONE: &str = "LOWER(CAST(SCHEMA_NAME AS CHAR)) = ?";

/// The only engine whose writes roll back.
const TRANSACTIONAL_ENGINE: &str = "InnoDB";

/// Error numbers whose messages name schema objects or states, never values.
///
/// Unknown column (1054) or table (1146), syntax (1064, which quotes the
/// caller's own SQL), access (1142, 1143), unknown function (1305), foreign
/// keys (1451, 1452), `NULL` (1048), range (1264), length (1406), checks
/// (3819, `MariaDB` 4025), JSON (3140, 3141), and duplicate column names
/// (1060, which name the caller's own output columns).
///
/// Not arithmetic overflow (1690): its expression may print an evaluated argument.
const VALUE_FREE_ERRORS: &[u16] = &[
    1054, 1146, 1064, 1142, 1143, 1305, 1451, 1452, 1048, 1264, 1406, 3819, 4025, 3140, 3141, 1060,
];

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
    /// The result does not fit the plan; the transaction rolls back.
    #[error(transparent)]
    Unfit(#[from] Unfit),
    /// A write reads a view but could not be undone.
    #[error(
        "column policy: a write that reads a view must be able to roll back \
         (not CREATE TABLE … AS SELECT, and only into InnoDB tables)"
    )]
    Unrollable,
    /// A view body changed between planning and execution; the transaction rolls back.
    #[error("column policy: a view changed while the statement ran")]
    ViewChanged,
    /// The statement failed with an error that may echo a ruled value.
    #[error("query failed (error {number}, SQLSTATE {state}): error detail withheld under column policy")]
    Withheld { number: u16, state: String },
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
    results: Vec<MySqlQueryResult>,
}

/// Session state that decides how names resolve.
struct Probe {
    /// The default database, which bare names resolve in.
    database: Option<String>,
    /// `lower_case_table_names`: 0 as written, 1 stored lowercased, 2 compared lowercased.
    lower_case: i64,
}

impl Probe {
    /// Returns whether table and database names compare as written.
    fn case_sensitive(&self) -> bool {
        self.lower_case == 0
    }

    /// Returns whether a written name part names the stored `actual` name.
    ///
    /// `information_schema` names compare case-insensitively whatever the setting.
    fn same(&self, written: &str, actual: &str, schema: &str) -> bool {
        if self.case_sensitive() && !schema.eq_ignore_ascii_case("information_schema") {
            written == actual
        } else {
            written
                .chars()
                .flat_map(char::to_lowercase)
                .eq(actual.chars().flat_map(char::to_lowercase))
        }
    }

    /// Returns the `TABLES` branch for `lookup` and the schema and name to bind.
    ///
    /// [`Probe::same`] filters the rows.
    fn branch(&self, lookup: &Lookup) -> (&'static str, String, String) {
        if self.lower_case == 2 || lookup.schema.eq_ignore_ascii_case("information_schema") {
            (TABLES_FOLDED, lookup.schema.to_lowercase(), lookup.name.to_lowercase())
        } else {
            (TABLES_EXACT, self.created(&lookup.schema), self.created(&lookup.name))
        }
    }

    /// Returns the name a relation created under `written` gets.
    ///
    /// Under 2 the server keeps the case, but names and rule keys compare lowercased anyway.
    fn created(&self, written: &str) -> String {
        if self.case_sensitive() {
            written.to_owned()
        } else {
            written.to_lowercase()
        }
    }
}

/// A written name as `MySQL` reads it.
#[derive(Debug, PartialEq, Eq)]
struct Lookup {
    schema: String,
    name: String,
}

impl Lookup {
    /// Reads `written` like the server; `None` when it can name no relation.
    ///
    /// A bare name resolves in the view's own schema inside a view body, else
    /// in the default database. Three or more parts never resolve.
    fn new(written: &WrittenName, database: Option<&str>) -> Option<Self> {
        match written.parts.as_slice() {
            [name] => {
                let schema = match &written.context {
                    Origin::View(view) => view.schema.clone(),
                    Origin::Query => database?.to_owned(),
                };
                Some(Self {
                    schema,
                    name: name.value.clone(),
                })
            }
            [schema, name] => Some(Self {
                schema: schema.value.clone(),
                name: name.value.clone(),
            }),
            _ => None,
        }
    }
}

/// One row of [`TABLES_EXACT`] / [`TABLES_FOLDED`].
type Table = (String, String, String, Option<String>);

/// One row of the [`COLUMNS_BRANCH`] / [`VIEW_BRANCH`] union.
type Detail = (String, String, Option<String>, Option<String>, i64, Option<String>);

/// A relation found in `information_schema`, before it becomes a catalog entry.
#[derive(Debug, Default)]
struct Found {
    /// `TABLE_TYPE`.
    kind: String,
    /// `ENGINE`; `NULL` for views.
    engine: Option<String>,
    /// `VIEW_DEFINITION`; empty when the user may not read it.
    body: Option<String>,
    /// Column names with their `EXTRA` flags, in ordinal order.
    columns: Vec<(String, String)>,
}

/// A relation the catalog reads described.
struct Owner {
    id: RelationId,
    /// Column names, star-hidden ones included.
    columns: Vec<String>,
    /// The `VIEW_DEFINITION` planned from, for a view.
    view: Option<String>,
    /// `ENGINE`, for a table.
    engine: Option<String>,
}

impl MysqlHandler {
    /// Plans `sql` under `policy`'s rules, runs it into `window`, and redacts the kept rows.
    ///
    /// The window holds raw rows as JSON arrays; the kept rows return redacted, as objects.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::MultiStatement`] or
    /// [`SqlError::ExecutableCommentBlocked`] for SQL the parser would read
    /// unlike the server, [`SqlError::ReadOnlyViolation`] for a write in
    /// read-only mode, a refusal when the handler has no redactor, the
    /// reason the column rules stopped the statement, or the [`SqlError`]
    /// of anything else that failed.
    pub(crate) async fn enforce(
        &self,
        policy: &PiiConfig,
        sql: &str,
        access: Access,
        timeout_secs: u64,
        database: Option<&str>,
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        // sqlx always lets `MySQL` run several statements, and the parser
        // expands executable comments the server may skip.
        validate_single_statement(sql, &MySqlDialect {})?;
        let Some(redactor) = &self.redactor else {
            return Err(ErrorData::internal_error(
                "column policy: column rules need PII redaction enabled",
                None,
            ));
        };
        if access == Access::Write && self.connection.read_only() {
            return Err(SqlError::ReadOnlyViolation.into());
        }
        let backend = self.config.backend;
        let run = self
            .connection
            .with_connection(sql, database, timeout_secs, async |conn| {
                match run(conn, policy, backend, sql, access, &mut |row| window.push(row)).await {
                    Err(Stop::Sql(error)) => Err(error),
                    other => Ok(other),
                }
            })
            .await?
            .map_err(|stop| ErrorData::internal_error(stop.to_string(), None))?;
        let rows = mem::take(&mut window.rows).into_iter().map(values).collect();
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

/// Plans one statement on `conn`, then executes it inside a transaction.
async fn run<'c>(
    conn: &mut sqlx::MySqlConnection,
    config: &'c PiiConfig,
    backend: DatabaseBackend,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let begin = match access {
        Access::Read => "START TRANSACTION READ ONLY",
        Access::Write => "START TRANSACTION",
    };
    let (analyzed, probe, owners) = resolve(&mut *conn, backend, sql).await?;
    let mut planned = analyzed.plan(config)?;
    if !rolls_back(analyzed.statement(), analyzed.target(), &owners) {
        return Err(Stop::Unrollable);
    }
    let failed = |planned: &Planned<'_>, error: sqlx::Error| match planned.withheld(echoing(&error)) {
        Some((number, state)) => Stop::Withheld { number, state },
        None => Stop::Sql(error),
    };
    let mut tx = sqlx::Connection::begin_with(&mut *conn, SqlStr::from_static(begin)).await?;
    let executed = match execute(&mut tx, sql, &mut planned, &probe, &owners, sink).await {
        Ok(executed) => executed,
        Err(Stop::Sql(error)) => return Err(failed(&planned, error)),
        Err(stop) => {
            tx.rollback().await?;
            return Err(stop);
        }
    };
    // A statement that commits implicitly cannot be undone, and moves no rows.
    if !commits_implicitly(analyzed.statement()) && !views_unchanged(&mut tx, &owners, analyzed.target()).await? {
        tx.rollback().await?;
        return Err(Stop::ViewChanged);
    }
    let finished = match access {
        Access::Read => tx.rollback().await,
        Access::Write => tx.commit().await,
    };
    finished.map_err(|error| failed(&planned, error))?;
    let (rows_affected, last_insert_id) = MysqlConnection::counts(sql, &executed.results);
    Ok(Run {
        planned,
        names: executed.names,
        rows_affected,
        last_insert_id,
    })
}

/// Reports whether a write to `target` can roll back should a view it reads change.
///
/// The view check runs after the statement: a write reading a view must
/// still be undoable then. `CREATE TABLE … AS SELECT` commits implicitly,
/// and only `InnoDB` targets roll back DML; an unknown engine cannot.
fn rolls_back(statement: &StatementKind, target: Option<&RelationId>, owners: &[Owner]) -> bool {
    // The views `views_unchanged` checks.
    let reads_view = owners
        .iter()
        .any(|owner| owner.view.is_some() && target != Some(&owner.id));
    if !reads_view {
        return true;
    }
    match statement {
        StatementKind::CreateTableAs => false,
        StatementKind::Insert | StatementKind::Update | StatementKind::Delete => target
            .and_then(|target| owners.iter().find(|owner| owner.id == *target))
            .and_then(|owner| owner.engine.as_deref())
            .is_some_and(|engine| engine.eq_ignore_ascii_case(TRANSACTIONAL_ENGINE)),
        _ => true,
    }
}

/// Reports whether `statement` commits implicitly on `MySQL` and `MariaDB`.
fn commits_implicitly(statement: &StatementKind) -> bool {
    matches!(
        statement,
        StatementKind::CreateTable
            | StatementKind::CreateTableAs
            | StatementKind::CreateView
            | StatementKind::Drop
            | StatementKind::Truncate
            | StatementKind::CreateIndex
    )
}

/// Reports whether every view body planned from is still the current one.
///
/// The statement's own `target`, which it may drop or replace, is skipped;
/// the module docs say why the re-read sees the body that ran.
async fn views_unchanged(
    conn: &mut sqlx::MySqlConnection,
    owners: &[Owner],
    target: Option<&RelationId>,
) -> Result<bool, sqlx::Error> {
    let views: Vec<(&RelationId, &str)> = owners
        .iter()
        .filter(|owner| target != Some(&owner.id))
        .filter_map(|owner| Some((&owner.id, owner.view.as_deref()?)))
        .collect();
    if views.is_empty() {
        return Ok(true);
    }
    let sql = vec![VIEW_CURRENT; views.len()].join(" UNION ALL ");
    let mut query = sqlx::query_as::<_, (String, String, Option<String>)>(AssertSqlSafe(sql));
    for (id, _) in &views {
        query = query.bind(&id.schema).bind(&id.name);
    }
    let current = query.fetch_all(conn).await?;
    Ok(views.iter().all(|(id, body)| {
        current.iter().any(|(schema, name, current)| {
            *schema == id.schema && *name == id.name && current.as_deref().map(str::trim) == Some(*body)
        })
    }))
}

/// Runs `sql`, feeding each row's values to `sink`.
///
/// The first row is checked against `planned` before any row reaches `sink`.
async fn execute(
    conn: &mut sqlx::MySqlConnection,
    sql: &str,
    planned: &mut Planned<'_>,
    probe: &Probe,
    owners: &[Owner],
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, Stop> {
    let first = |row: &MySqlRow, names: &[String]| {
        planned
            .check_first(names, origins(row.columns(), probe, owners))
            .map_err(Stop::from)
    };
    let (names, results) = MysqlConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed { names, results })
}

/// Translates the origin the server reports for each result column.
///
/// Only an origin naming a relation the catalog reads described, and one of
/// its columns, is translated. Below a derived table or CTE neither server
/// reports the physical column: `MySQL` names the base table with the
/// derived column's alias, `MariaDB` the derived table's alias (without a
/// schema once materialized). Those, and expressions, rest on the plan.
fn origins(columns: &[MySqlColumn], probe: &Probe, owners: &[Owner]) -> Vec<(usize, RelationId, Option<String>)> {
    let mut origins = Vec::new();
    for column in columns {
        let position = column.ordinal();
        let ColumnOrigin::Table(origin) = column.origin() else {
            continue;
        };
        // `schema.table`, split at the owner's schema length: either part may hold a `.`.
        let owner = owners.iter().find(|owner| {
            let (schema, name) = (&owner.id.schema, &owner.id.name);
            origin.table.get(schema.len()..=schema.len()) == Some(".")
                && probe.same(&origin.table[..schema.len()], schema, schema)
                && probe.same(&origin.table[schema.len() + 1..], name, schema)
        });
        // Column names compare case-insensitively on every setting.
        let Some((owner, name)) = owner.and_then(|owner| {
            let name = owner
                .columns
                .iter()
                .find(|name| name.eq_ignore_ascii_case(&origin.name))?;
            Some((owner, name))
        }) else {
            continue;
        };
        origins.push((position, owner.id.clone(), Some(name.clone())));
    }
    origins
}

/// Analyzes `sql`, fetching missing relations for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds.
///
/// Also returns the session probe and the relations it described.
async fn resolve(
    conn: &mut sqlx::MySqlConnection,
    backend: DatabaseBackend,
    sql: &str,
) -> Result<(Analyzed, Probe, Vec<Owner>), Stop> {
    let (database, lower_case, sql_mode, charset): (Option<String>, i64, String, String) =
        sqlx::query_as(PROBE_SQL).fetch_one(&mut *conn).await?;
    let probe = Probe { database, lower_case };
    let mut analysis = Analysis::new(sql, backend);
    analysis.creates_in(probe.database.as_deref().map(|database| probe.created(database)));
    if let Some(reason) = lexical_mismatch(&sql_mode, &charset, sql) {
        analysis.mismatch(reason);
    }
    let mut owners = Vec::new();
    let mut entries = Vec::new();
    loop {
        match analysis.step(entries)? {
            Step::Missing(names) => entries = fetch(conn, &probe, names, &mut owners).await?,
            Step::Done(analyzed) => return Ok((analyzed, probe, owners)),
        }
    }
}

/// Returns why the server would lex `sql` unlike the parser, if it would.
///
/// Session settings (`sql_mode` flags, multibyte client charsets) change
/// how quotes and escapes lex; the parser reads user and system variables
/// (`@x`, `@@x`) as column names. Executable comments are refused before
/// analysis, by [`validate_single_statement`].
fn lexical_mismatch(sql_mode: &str, charset: &str, sql: &str) -> Option<String> {
    if let Some(mode) = LEXICAL_MODES
        .iter()
        .find(|flag| sql_mode.split(',').any(|set| set.trim().eq_ignore_ascii_case(flag)))
    {
        return Some(format!("sql_mode {mode} is set"));
    }
    if LEXICAL_CHARSETS
        .iter()
        .any(|unsafe_charset| charset.eq_ignore_ascii_case(unsafe_charset))
    {
        return Some(format!("client charset {charset}"));
    }
    // A tokenizer error fails `analyze` too, which parses with the same dialect.
    let tokens = Tokenizer::new(&MySqlDialect {}, sql).tokenize().ok()?;
    tokens
        .iter()
        .any(|token| match token {
            Token::Word(word) => word.quote_style.is_none() && word.value.starts_with('@'),
            Token::AtSign => true,
            _ => false,
        })
        .then(|| "user or system variable".into())
}

/// Builds a catalog entry for each of `names`, read in batched queries.
async fn fetch(
    conn: &mut sqlx::MySqlConnection,
    probe: &Probe,
    names: Vec<WrittenName>,
    owners: &mut Vec<Owner>,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    let mut entries = Vec::new();
    let lookups: Vec<(WrittenName, Lookup)> = names
        .into_iter()
        .filter_map(|name| {
            if let Some(lookup) = Lookup::new(&name, probe.database.as_deref()) {
                return Some((name, lookup));
            }
            entries.push((name, Entry::Absent));
            None
        })
        .collect();
    if lookups.is_empty() {
        return Ok(entries);
    }
    let mut branches = Vec::new();
    let mut binds = Vec::new();
    for (_, lookup) in &lookups {
        let (branch, schema, name) = probe.branch(lookup);
        branches.push(branch);
        binds.extend([schema, name]);
    }
    let mut query = sqlx::query_as::<_, Table>(AssertSqlSafe(branches.join(" UNION ALL ")));
    for bind in binds {
        query = query.bind(bind);
    }
    let mut found: BTreeMap<(String, String), Found> = BTreeMap::new();
    for (schema, name, kind, engine) in query.fetch_all(&mut *conn).await? {
        found.entry((schema, name)).or_insert_with(|| Found {
            kind,
            engine,
            ..Found::default()
        });
    }
    if !found.is_empty() {
        let mut branches = Vec::new();
        let mut keys = Vec::new();
        for (key, relation) in &found {
            branches.push(COLUMNS_BRANCH);
            keys.push(key);
            if relation.kind == "VIEW" {
                branches.push(VIEW_BRANCH);
                keys.push(key);
            }
        }
        let sql = format!("{}{DETAILS_ORDER}", branches.join(" UNION ALL "));
        let mut details = sqlx::query_as::<_, Detail>(AssertSqlSafe(sql));
        for (schema, name) in keys {
            details = details.bind(schema.as_str()).bind(name.as_str());
        }
        for (schema, name, column, extra, _, body) in details.fetch_all(&mut *conn).await? {
            let Some(relation) = found.get_mut(&(schema, name)) else {
                continue;
            };
            if let Some(column) = column {
                relation.columns.push((column, extra.unwrap_or_default()));
            }
            if body.is_some() {
                relation.body = body;
            }
        }
    }

    let mut unresolved = Vec::new();
    for (name, lookup) in lookups {
        let hit = found.iter().find(|((schema, relation), _)| {
            probe.same(&lookup.schema, schema, schema) && probe.same(&lookup.name, relation, schema)
        });
        let Some(((schema, relation), found)) = hit else {
            unresolved.push((name, lookup));
            continue;
        };
        let entry = entry(schema, relation, found);
        if let Entry::Resolved(resolved) = &entry
            && !owners.iter().any(|owner| owner.id == resolved.id)
        {
            owners.push(Owner {
                id: resolved.id.clone(),
                columns: found.columns.iter().map(|(column, _)| column.clone()).collect(),
                view: resolved.view_sql.clone(),
                engine: found.engine.clone(),
            });
        }
        entries.push((name, entry));
    }
    entries.extend(vacant(conn, probe, unresolved).await?);
    Ok(entries)
}

/// Builds the catalog entry of one relation found in `information_schema`.
fn entry(schema: &str, name: &str, found: &Found) -> Entry {
    let kind = match found.kind.as_str() {
        _ if schema.eq_ignore_ascii_case("information_schema") => RelationKind::System,
        "SYSTEM VIEW" => RelationKind::System,
        "BASE TABLE" | "SYSTEM VERSIONED" | "SEQUENCE" => RelationKind::Table,
        "VIEW" => RelationKind::View,
        _ => return Entry::Absent,
    };
    let columns = found
        .columns
        .iter()
        .map(|(column, extra)| {
            let invisible = extra
                .split_whitespace()
                .any(|flag| flag.eq_ignore_ascii_case("INVISIBLE"));
            if invisible {
                CatalogColumn::hidden(column)
            } else {
                CatalogColumn::new(column)
            }
        })
        .collect();
    let relation = Relation::new(RelationId::new(CATALOG, schema, name), kind, columns);
    if kind != RelationKind::View {
        return Entry::Resolved(relation);
    }
    // An empty body means the user may not read it (another definer, no `SHOW VIEW`).
    match found.body.as_deref().map(str::trim) {
        Some(body) if !body.is_empty() => Entry::Resolved(relation.with_view_sql(body)),
        _ => Entry::Absent,
    }
}

/// Builds the entries of names nothing resolves to.
///
/// A relation created under a name in the query lands in its schema when
/// that schema exists; names in view bodies are absent.
async fn vacant(
    conn: &mut sqlx::MySqlConnection,
    probe: &Probe,
    names: Vec<(WrittenName, Lookup)>,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    let (query, view): (Vec<_>, Vec<_>) = names.into_iter().partition(|(name, _)| name.context == Origin::Query);
    let mut entries: Vec<_> = view.into_iter().map(|(name, _)| (name, Entry::Absent)).collect();
    if query.is_empty() {
        return Ok(entries);
    }
    let filter = vec![SCHEMAS_ONE; query.len()].join(" OR ");
    let mut schemas = sqlx::query_scalar::<_, String>(AssertSqlSafe(format!("{SCHEMAS_SQL}{filter}")));
    for (_, lookup) in &query {
        schemas = schemas.bind(lookup.schema.to_lowercase());
    }
    let schemas = schemas.fetch_all(&mut *conn).await?;
    for (name, lookup) in query {
        let schema = schemas.iter().find(|schema| probe.same(&lookup.schema, schema, schema));
        let entry = schema.map_or(Entry::Absent, |schema| {
            Entry::Vacant(RelationId::new(CATALOG, schema, probe.created(&lookup.name)))
        });
        entries.push((name, entry));
    }
    Ok(entries)
}

/// Returns the error number and `SQLSTATE` of an error that may echo a value.
///
/// Timeouts and non-database errors are never withheld.
fn echoing(error: &sqlx::Error) -> Option<(u16, String)> {
    if MysqlConnection::is_timeout(error) {
        return None;
    }
    let error = error.as_database_error()?.try_downcast_ref::<MySqlDatabaseError>()?;
    let number = error.number();
    (!VALUE_FREE_ERRORS.contains(&number)).then(|| (number, error.code().unwrap_or("HY000").to_owned()))
}

#[cfg(test)]
mod tests {
    use dbmcp_policy::NamePart;

    use super::*;

    fn probe(lower_case: i64) -> Probe {
        Probe {
            database: Some("app".into()),
            lower_case,
        }
    }

    #[test]
    fn bare_names_resolve_in_the_default_database_or_the_view_schema() {
        let bare = WrittenName::query(vec![NamePart::bare("Users")]);
        assert_eq!(
            Lookup::new(&bare, Some("app")),
            Some(Lookup {
                schema: "app".into(),
                name: "Users".into(),
            })
        );
        assert_eq!(Lookup::new(&bare, None), None, "no database selected");
        let in_view = WrittenName {
            parts: vec![NamePart::quoted("users")],
            context: Origin::View(RelationId::new(CATALOG, "other", "v")),
        };
        assert_eq!(
            Lookup::new(&in_view, Some("app")).map(|lookup| lookup.schema),
            Some("other".into())
        );
    }

    #[test]
    fn qualified_names_take_their_schema_and_three_parts_never_resolve() {
        let two = WrittenName::query(vec![NamePart::bare("analytics"), NamePart::bare("events")]);
        assert_eq!(
            Lookup::new(&two, Some("app")),
            Some(Lookup {
                schema: "analytics".into(),
                name: "events".into(),
            })
        );
        let three = WrittenName::query(vec![
            NamePart::bare("def"),
            NamePart::bare("app"),
            NamePart::bare("users"),
        ]);
        assert_eq!(Lookup::new(&three, Some("app")), None);
    }

    #[test]
    fn name_case_follows_lower_case_table_names() {
        assert!(!probe(0).same("Users", "users", "app"));
        assert!(probe(0).same("users", "users", "app"));
        assert!(probe(0).same("TABLES", "tables", "information_schema"));
        assert!(probe(1).same("Users", "users", "app"));
        assert!(probe(2).same("Users", "users", "app"));
        assert_eq!(probe(0).created("New"), "New");
        assert_eq!(probe(1).created("New"), "new");
        assert_eq!(
            probe(2)
                .branch(&Lookup {
                    schema: "App".into(),
                    name: "T".into()
                })
                .0,
            TABLES_FOLDED
        );
    }

    #[test]
    fn ansi_quotes_and_no_backslash_escapes_mismatch_the_parser() {
        let mode = |sql_mode| lexical_mismatch(sql_mode, "utf8mb4", "SELECT 1");
        assert_eq!(mode("STRICT_TRANS_TABLES,NO_ENGINE_SUBSTITUTION"), None);
        assert_eq!(
            mode("STRICT_TRANS_TABLES,ANSI_QUOTES").as_deref(),
            Some("sql_mode ANSI_QUOTES is set")
        );
        // `sql_mode = ANSI` reads back expanded.
        assert!(mode("REAL_AS_FLOAT,PIPES_AS_CONCAT,ANSI_QUOTES,IGNORE_SPACE,ONLY_FULL_GROUP_BY,ANSI").is_some());
        assert!(mode("NO_BACKSLASH_ESCAPES").is_some());
        assert_eq!(mode("ANSI_QUOTESX"), None);
    }

    #[test]
    fn multibyte_client_charsets_mismatch_the_parser() {
        for charset in ["gbk", "SJIS", "big5", "cp932", "gb18030"] {
            assert!(lexical_mismatch("", charset, "SELECT 1").is_some(), "{charset}");
        }
        assert_eq!(lexical_mismatch("", "latin1", "SELECT 1"), None);
    }

    #[test]
    fn variables_mismatch_the_parser() {
        let sql = |sql| lexical_mismatch("", "utf8mb4", sql);
        assert!(sql("SELECT @x := ssn AS a FROM users").is_some());
        assert!(sql("SELECT @@version").is_some());
        assert!(sql("SELECT @'x'").is_some());
        assert_eq!(sql("SELECT `@x`, '@x' /* plain */ FROM t"), None);
    }

    fn found(kind: &str, body: Option<&str>) -> Found {
        Found {
            kind: kind.into(),
            engine: None,
            body: body.map(Into::into),
            columns: vec![
                ("id".into(), "auto_increment".into()),
                ("secret".into(), "INVISIBLE".into()),
            ],
        }
    }

    fn resolved(schema: &str, found: &Found) -> Option<Relation> {
        match entry(schema, "r", found) {
            Entry::Resolved(relation) => Some(relation),
            _ => None,
        }
    }

    #[test]
    fn table_types_map_to_relation_kinds() {
        for (kind, expected) in [
            ("BASE TABLE", RelationKind::Table),
            ("SYSTEM VERSIONED", RelationKind::Table),
            ("SEQUENCE", RelationKind::Table),
            ("SYSTEM VIEW", RelationKind::System),
        ] {
            let relation = resolved("app", &found(kind, None)).expect(kind);
            assert_eq!(relation.kind, expected, "{kind}");
            assert_eq!(relation.id, RelationId::new(CATALOG, "app", "r"));
        }
        assert_eq!(resolved("app", &found("TEMPORARY", None)), None);
        let system = resolved("information_schema", &found("VIEW", Some("select 1"))).expect("system");
        assert_eq!(system.kind, RelationKind::System);
        assert_eq!(system.view_sql, None);
    }

    #[test]
    fn invisible_columns_are_hidden_from_star() {
        let relation = resolved("app", &found("BASE TABLE", None)).expect("table");
        assert_eq!(
            relation.columns,
            vec![CatalogColumn::new("id"), CatalogColumn::hidden("secret")]
        );
    }

    #[test]
    fn views_without_a_readable_body_are_absent() {
        let view = resolved(
            "app",
            &found("VIEW", Some(" select `app`.`users`.`id` AS `id` from `app`.`users` ")),
        )
        .expect("view");
        assert_eq!(view.kind, RelationKind::View);
        assert_eq!(
            view.view_sql.as_deref(),
            Some("select `app`.`users`.`id` AS `id` from `app`.`users`")
        );
        assert_eq!(resolved("app", &found("VIEW", Some(""))), None);
        assert_eq!(resolved("app", &found("VIEW", None)), None);
    }

    fn owner(name: &str, engine: Option<&str>, view: Option<&str>) -> Owner {
        Owner {
            id: RelationId::new(CATALOG, "app", name),
            columns: vec!["id".into()],
            view: view.map(Into::into),
            engine: engine.map(Into::into),
        }
    }

    /// Reports whether `statement` writing `app.target` rolls back.
    fn write_rolls_back(statement: &StatementKind, owners: &[Owner]) -> bool {
        rolls_back(statement, Some(&RelationId::new(CATALOG, "app", "target")), owners)
    }

    #[test]
    fn writes_reading_views_must_roll_back() {
        let view = || owner("source", None, Some("select 1"));
        let table = || owner("source", Some("InnoDB"), None);
        let target = |engine| owner("target", engine, None);
        let innodb = [view(), target(Some("InnoDB"))];
        let myisam = [view(), target(Some("MyISAM"))];
        let unknown = [view(), target(None)];
        let no_view = [table(), target(Some("MyISAM"))];
        assert!(!write_rolls_back(&StatementKind::CreateTableAs, &innodb));
        assert!(write_rolls_back(&StatementKind::CreateTableAs, &no_view));
        assert!(write_rolls_back(&StatementKind::Insert, &innodb));
        assert!(!write_rolls_back(&StatementKind::Insert, &myisam));
        assert!(!write_rolls_back(&StatementKind::Update, &unknown));
        assert!(!write_rolls_back(&StatementKind::Delete, &[view()]));
        assert!(write_rolls_back(&StatementKind::Insert, &no_view));
        assert!(write_rolls_back(&StatementKind::Query, &[view()]));
        assert!(write_rolls_back(&StatementKind::CreateView, &[view()]));
    }

    #[test]
    fn a_replaced_view_is_not_a_view_it_reads() {
        let target = RelationId::new(CATALOG, "app", "source");
        let owners = [owner("source", None, Some("select 1"))];
        assert!(rolls_back(&StatementKind::CreateTableAs, Some(&target), &owners));
    }

    /// Connects to the seeded `app` database that `./tests/run.sh` starts.
    async fn seeded() -> sqlx::MySqlConnection {
        let host = std::env::var("DBMCP_DB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let port = std::env::var("DBMCP_DB_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(3306);
        let options = sqlx::mysql::MySqlConnectOptions::new()
            .host(&host)
            .port(port)
            .username("root")
            .database("app");
        <sqlx::MySqlConnection as sqlx::Connection>::connect_with(&options)
            .await
            .expect("connect to the seeded database")
    }

    #[tokio::test]
    #[ignore = "needs the seeded MySQL or MariaDB container: ./tests/run.sh --filter mysql"]
    async fn origins_translate_described_columns() {
        let backend = match std::env::var("DBMCP_DB_BACKEND").as_deref() {
            Ok("mariadb") => DatabaseBackend::Mariadb,
            _ => DatabaseBackend::Mysql,
        };
        let mut conn = seeded().await;
        let sql = "SELECT name, email, 1 FROM users";
        let (_, probe, owners) = resolve(&mut conn, backend, sql).await.expect("resolve");
        let row = sqlx::query(sql).fetch_one(&mut conn).await.expect("row");
        let users = RelationId::new(CATALOG, "app", "users");
        assert_eq!(
            origins(row.columns(), &probe, &owners),
            [
                (0, users.clone(), Some("name".to_owned())),
                (1, users, Some("email".to_owned())),
            ]
        );
        assert_eq!(origins(row.columns(), &probe, &[]), []);
    }
}
