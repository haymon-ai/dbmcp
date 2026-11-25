//! Column rule (`privacy.rules`) enforcement on `MySQL` and `MariaDB`.
//!
//! With column rules, `read`, `write`, and `explain` run each
//! statement through [`MysqlHandler::enforce`]. On one pooled connection of
//! the target database and under one deadline, it:
//!
//! 1. probes the session: the server version (a [`BackendMismatch`]
//!    refuses), the default database, `lower_case_table_names`, and whether
//!    `sql_mode` and the client charset let the parser read quotes and
//!    escapes as the server does,
//! 2. analyzes the statement's lineage, resolving the names it reports
//!    missing in `information_schema` (`TABLES`, `COLUMNS` with `INVISIBLE`
//!    columns hidden from `*`, `VIEWS` bodies, `TRIGGERS`, and referential
//!    actions from `REFERENTIAL_CONSTRAINTS` and `KEY_COLUMN_USAGE`) for at most
//!    [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds,
//! 3. plans it with [`Analyzed::plan`]; a rejection runs nothing,
//! 4. refuses a write that reads a view but cannot roll back, and on
//!    `MySQL` pins the session's `EXPLAIN` JSON format to version 1 while a
//!    pruned plan runs, restoring the server default afterwards,
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

use dbmcp_policy::{
    Access, Analysis, Analyzed, Cascade, CascadeAction, CatalogColumn, Enforced, Entry, Executed, Family, Origin,
    Planned, Relation, RelationId, RelationKind, RelationRef, Run, SourceRef, StatementKind, Step, WrittenName, admit,
    settle,
};
use dbmcp_sql::Connection as _;
use dbmcp_sql::validation::validate_single_statement;
use dbmcp_truncate::Window;
use dbmcp_types::{database, privacy};
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlparser::dialect::MySqlDialect;
use sqlparser::tokenizer::{Token, Tokenizer};
use sqlx::mysql::{MySqlColumn, MySqlDatabaseError, MySqlRow};
use sqlx::{Column as _, ColumnOrigin, Row as _};
use tracing::warn;

use crate::MysqlHandler;
use crate::connection::MysqlConnection;
use crate::flavour::{self, BackendMismatch};
use crate::queries;
use crate::queries::policy::Match;

/// Catalog name of every `MySQL` relation in rule keys (`TABLE_CATALOG`).
const CATALOG: &str = "def";

/// `sql_mode` flags under which the server lexes quotes or escapes unlike the parser.
const LEXICAL_MODES: &[&str] = &["ANSI_QUOTES", "NO_BACKSLASH_ESCAPES"];

/// Client charsets whose multibyte characters may end in a `\` byte, hiding an escape.
const LEXICAL_CHARSETS: &[&str] = &["big5", "cp932", "gb18030", "gbk", "sjis"];

/// The only engine whose writes roll back.
const TRANSACTIONAL_ENGINE: &str = "InnoDB";

/// Error numbers whose messages name schema objects or states, never values.
///
/// Unknown column (1054) or table (1146), syntax (1064, which quotes the
/// caller's own SQL), access (1142, 1143), unknown function (1305), foreign
/// keys (1451, 1452), `NULL` (1048), checks (3819, `MariaDB` 4025), and
/// duplicate column names (1060, which name the caller's own output columns).
///
/// Not arithmetic overflow (1690): its expression may print an evaluated
/// argument. Not invalid JSON text (3140, 3141): its position can encode a
/// value (`CAST(CONCAT(REPEAT(' ', ASCII(email)), 'x') AS JSON)`). Not out of
/// range (1264) or too long (1406): their row number can encode a value
/// (`UPDATE users SET ssn = IF(ssn LIKE '1%', REPEAT('x', 99), ssn)` fails at
/// the first matching row).
const VALUE_FREE_ERRORS: &[u16] = &[1054, 1146, 1064, 1142, 1143, 1305, 1451, 1452, 1048, 3819, 4025, 1060];

/// Why a statement returned no rows under the column rules.
type Stop = dbmcp_policy::Stop<Refusal>;

/// The refusals only `MySQL` and `MariaDB` enforcement makes.
#[derive(Debug, thiserror::Error)]
enum Refusal {
    /// The server is not the configured backend; nothing ran.
    #[error("column policy: {0}")]
    BackendMismatch(BackendMismatch),
    /// A write reads a view but could not be undone.
    #[error(
        "column policy: a write that reads a view must be able to roll back \
         (not CREATE TABLE … AS SELECT, and only into InnoDB tables)"
    )]
    Unrollable,
    /// A view body changed between planning and execution; the transaction rolls back.
    #[error("column policy: a view changed while the statement ran")]
    ViewChanged,
}

/// Session state that decides how names resolve.
struct Probe {
    /// The default database, which bare names resolve in.
    database: Option<String>,
    /// `lower_case_table_names`: 0 as written, 1 stored lowercased, 2 compared lowercased.
    lower_case: i64,
    /// `sql_mode`, as the server reads it back (comma-separated flags).
    sql_mode: String,
    /// The client charset (`character_set_client`).
    charset: String,
    /// Whether the server is `MariaDB`, read from `VERSION()`.
    mariadb: bool,
}

impl Probe {
    /// Reads the session state of `conn`.
    ///
    /// # Errors
    ///
    /// [`Refusal::BackendMismatch`] when the server is not the configured
    /// `backend`, [`Stop::Sql`] when the probe query fails.
    async fn read(conn: &mut sqlx::MySqlConnection, backend: database::Backend) -> Result<Self, Stop> {
        let (database, lower_case, sql_mode, charset, version) = queries::policy::probe(conn).await?;
        let server = flavour::of(&version);
        if server != backend {
            return Err(Stop::Refused(Refusal::BackendMismatch(BackendMismatch {
                configured: backend,
                server,
            })));
        }
        Ok(Self {
            database,
            lower_case,
            sql_mode,
            charset,
            mariadb: server == database::Backend::Mariadb,
        })
    }

    /// Returns why the server would lex `sql` unlike the parser, if it would.
    ///
    /// Session settings (`sql_mode` flags, multibyte client charsets) change
    /// how quotes and escapes lex; the parser reads user and system variables
    /// (`@x`, `@@x`) as column names. Executable comments and characters the
    /// tokenizer cannot read are refused before analysis, by
    /// [`validate_single_statement`].
    fn mismatch(&self, sql: &str) -> Option<String> {
        if let Some(mode) = LEXICAL_MODES.iter().find(|flag| {
            self.sql_mode
                .split(',')
                .any(|set| set.trim().eq_ignore_ascii_case(flag))
        }) {
            return Some(format!("sql_mode {mode} is set"));
        }
        if LEXICAL_CHARSETS
            .iter()
            .any(|unsafe_charset| self.charset.eq_ignore_ascii_case(unsafe_charset))
        {
            return Some(format!("client charset {}", self.charset));
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
    fn branch(&self, lookup: &Lookup) -> (Match, String, String) {
        if self.lower_case == 2 || lookup.schema.eq_ignore_ascii_case("information_schema") {
            (Match::Folded, lookup.schema.to_lowercase(), lookup.name.to_lowercase())
        } else {
            (Match::Exact, self.created(&lookup.schema), self.created(&lookup.name))
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

/// A [`RelationRef`] with its schema, as every `MySQL` reading has.
#[derive(Debug, PartialEq, Eq)]
struct Lookup {
    schema: String,
    name: String,
}

/// A relation found in `information_schema`, before it becomes a catalog entry.
#[derive(Debug, Default)]
struct Found {
    /// `TABLE_TYPE`.
    kind: String,
    /// `ENGINE`; `NULL` for views.
    engine: Option<String>,
    /// `VIEW_DEFINITION`; empty when the user may not read it.
    body: Option<String>,
    /// Columns in ordinal order.
    columns: Vec<FoundColumn>,
    /// Whether `TRIGGERS` lists a trigger on it.
    triggers: bool,
    /// The referential actions below it, for a table.
    cascades: Vec<Cascade>,
}

/// A column found in `information_schema.COLUMNS`.
#[derive(Debug)]
struct FoundColumn {
    /// `COLUMN_NAME`.
    name: String,
    /// `EXTRA` flags.
    extra: String,
    /// Whether `COLUMN_DEFAULT` holds a default ([`has_default`]).
    default: bool,
    /// Whether `GENERATION_EXPRESSION` is set.
    generated: bool,
}

/// Reports whether a `COLUMN_DEFAULT` text is a default.
///
/// `MySQL` reports `DEFAULT NULL` as SQL `NULL`; `MariaDB` as the unquoted
/// text `NULL`, quoting string defaults, so the text means no default there.
fn has_default(text: Option<&str>, mariadb: bool) -> bool {
    text.is_some_and(|text| !(mariadb && text == "NULL"))
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
    /// Returns [`SqlError::MultiStatement`],
    /// [`SqlError::ExecutableCommentBlocked`], or
    /// [`SqlError::UnreadableCharacter`] for SQL the parser would read
    /// unlike the server, [`SqlError::ReadOnlyViolation`] for a write in
    /// read-only mode, a refusal when the handler has no redactor, the
    /// reason the column rules stopped the statement, or the [`SqlError`]
    /// of anything else that failed.
    pub(crate) async fn enforce(
        &self,
        policy: &privacy::Config,
        sql: &str,
        access: Access,
        timeout_secs: u64,
        database: Option<&str>,
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        // sqlx always lets `MySQL` run several statements, and the parser
        // expands executable comments the server may skip.
        validate_single_statement(sql, &MySqlDialect {})?;
        let admitted = admit(self.redactor.as_ref(), access, self.connection.read_only())?;
        let backend = self.config.backend;
        let ran = self
            .connection
            .with_connection(sql, database, timeout_secs, async |conn| {
                Stop::split(run(conn, policy, backend, sql, access, &mut |row| window.push(row)).await)
            })
            .await;
        Ok(admitted.finish(ran, window).await?)
    }
}

/// Plans one statement on `conn`, then executes it inside a transaction.
async fn run<'c>(
    conn: &mut sqlx::MySqlConnection,
    config: &'c privacy::Config,
    backend: database::Backend,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    let (analyzed, probe, owners) = resolve(&mut *conn, backend, sql).await?;
    let mut planned = analyzed.plan(config)?;
    if !rolls_back(analyzed.statement(), analyzed.target(), &owners) {
        return Err(Stop::Refused(Refusal::Unrollable));
    }
    // The prune reads only format version 1 plans.
    let pins = backend == database::Backend::Mysql && planned.prunes();
    if pins {
        queries::policy::explain_format(&mut *conn, true).await?;
    }
    let ran = async {
        let mut tx = queries::policy::begin(&mut *conn, access).await?;
        let executed = async {
            let executed = execute(&mut tx, sql, &mut planned, &probe, &owners, sink).await?;
            // A statement that commits implicitly cannot be undone, and moves no rows.
            if !commits_implicitly(analyzed.statement())
                && !views_unchanged(&mut tx, &owners, analyzed.target()).await?
            {
                return Err(Stop::Refused(Refusal::ViewChanged));
            }
            Ok(executed)
        }
        .await;
        settle(Some(tx), access, planned, echoing, executed).await
    }
    .await;
    if pins && let Err(error) = queries::policy::explain_format(&mut *conn, false).await {
        // Only later unpruned plans on this connection see version 1; a dead one fails the pool's ping.
        warn!(%error, "EXPLAIN JSON format version not restored");
    }
    ran
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
    let names: Vec<_> = views
        .iter()
        .map(|(id, _)| (id.schema.as_str(), id.name.as_str()))
        .collect();
    let current = queries::policy::view_bodies(conn, &names).await?;
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
    let (names, rows_affected, last_insert_id) = MysqlConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed {
        names,
        rows_affected,
        last_insert_id,
    })
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
/// Refuses before analysis when the server is not the configured `backend`.
async fn resolve(
    conn: &mut sqlx::MySqlConnection,
    backend: database::Backend,
    sql: &str,
) -> Result<(Analyzed, Probe, Vec<Owner>), Stop> {
    let probe = Probe::read(&mut *conn, backend).await?;
    let mut analysis = Analysis::new(sql, backend);
    analysis.connected_to(probe.database.clone());
    analysis.creates_in(probe.database.as_deref().map(|database| probe.created(database)));
    if let Some(reason) = probe.mismatch(sql) {
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

/// Builds a catalog entry for each of `names`, read in batched queries.
async fn fetch(
    conn: &mut sqlx::MySqlConnection,
    probe: &Probe,
    names: Vec<(WrittenName, RelationRef)>,
    owners: &mut Vec<Owner>,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    let mut entries = Vec::new();
    let mut lookups = Vec::new();
    for (name, reference) in names {
        match reference.schema {
            Some(schema) => lookups.push((
                name,
                Lookup {
                    schema,
                    name: reference.name,
                },
            )),
            None => entries.push((name, Entry::Absent)),
        }
    }
    if lookups.is_empty() {
        return Ok(entries);
    }
    let branches: Vec<_> = lookups.iter().map(|(_, lookup)| probe.branch(lookup)).collect();
    let mut found: BTreeMap<(String, String), Found> = BTreeMap::new();
    for (schema, name, kind, engine) in queries::policy::tables(&mut *conn, &branches).await? {
        found.entry((schema, name)).or_insert_with(|| Found {
            kind,
            engine,
            ..Found::default()
        });
    }
    if !found.is_empty() {
        let relations: Vec<_> = found
            .iter()
            .map(|((schema, name), relation)| (schema.as_str(), name.as_str(), relation.kind == "VIEW"))
            .collect();
        for (schema, name, column, extra, _, body, default, generated, trigger) in
            queries::policy::details(&mut *conn, &relations).await?
        {
            let Some(relation) = found.get_mut(&(schema, name)) else {
                continue;
            };
            relation.triggers |= trigger != 0;
            if let Some(column) = column {
                relation.columns.push(FoundColumn {
                    name: column,
                    extra: extra.unwrap_or_default(),
                    default: has_default(default.as_deref(), probe.mariadb),
                    generated: generated != 0,
                });
            }
            if body.is_some() {
                relation.body = body;
            }
        }
        cascades(conn, &mut found).await?;
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
                columns: found.columns.iter().map(|column| column.name.clone()).collect(),
                view: resolved.view_sql.clone(),
                engine: found.engine.clone(),
            });
        }
        entries.push((name, entry));
    }
    entries.extend(vacant(conn, probe, unresolved).await?);
    Ok(entries)
}

/// Reads the referential actions below every table of `found`, in one walk.
///
/// `MySQL` and `MariaDB` fire no triggers on cascaded changes.
async fn cascades(
    conn: &mut sqlx::MySqlConnection,
    found: &mut BTreeMap<(String, String), Found>,
) -> Result<(), sqlx::Error> {
    let roots: Vec<_> = found
        .iter()
        .filter(|(_, relation)| matches!(relation.kind.as_str(), "BASE TABLE" | "SYSTEM VERSIONED"))
        .map(|(key, _)| key.clone())
        .collect();
    if roots.is_empty() {
        return Ok(());
    }
    let column =
        |schema: &str, name: &str, column: String| SourceRef::new(RelationId::new(CATALOG, schema, name), column);
    for (rs, rt, ps, pt, pc, cs, ct, cc, rule, deletes) in queries::policy::cascades(&mut *conn, &roots).await? {
        let Some(relation) = found.get_mut(&(rs, rt)) else {
            continue;
        };
        let on_delete = deletes != 0;
        relation.cascades.push(Cascade {
            parent: column(&ps, &pt, pc),
            child: column(&cs, &ct, cc),
            action: match rule.to_ascii_uppercase().as_str() {
                "CASCADE" if on_delete => CascadeAction::Delete,
                "CASCADE" => CascadeAction::Copy,
                "SET NULL" => CascadeAction::Null,
                _ => CascadeAction::Default,
            },
            on_delete,
            triggers: false,
        });
    }
    Ok(())
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
        .map(|column| {
            let flag = |name: &str| {
                column
                    .extra
                    .split_whitespace()
                    .any(|flag| flag.eq_ignore_ascii_case(name))
            };
            CatalogColumn {
                name: column.name.clone(),
                star_visible: !flag("INVISIBLE"),
                default: column.default || flag("auto_increment") || flag("DEFAULT_GENERATED"),
                generated: column.generated,
                // `MySQL` writes `on update CURRENT_TIMESTAMP`, `MariaDB` `on update current_timestamp()`.
                on_update: column.extra.to_ascii_lowercase().contains("on update"),
            }
        })
        .collect();
    // An empty body means the user may not read it (another definer, no `SHOW VIEW`).
    let view_sql = match found.body.as_deref().map(str::trim) {
        _ if kind != RelationKind::View => None,
        Some(body) if !body.is_empty() => Some(body.to_owned()),
        _ => return Entry::Absent,
    };
    Entry::Resolved(Relation {
        id: RelationId::new(CATALOG, schema, name),
        kind,
        columns,
        view_sql,
        // `MySQL` and `MariaDB` have no inheritance.
        family: Family::default(),
        triggers: found.triggers,
        cascades: found.cascades.clone(),
    })
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
    let lowered: Vec<_> = query.iter().map(|(_, lookup)| lookup.schema.to_lowercase()).collect();
    let schemas = queries::policy::schemas(&mut *conn, &lowered).await?;
    for (name, lookup) in query {
        let schema = schemas.iter().find(|schema| probe.same(&lookup.schema, schema, schema));
        let entry = schema.map_or(Entry::Absent, |schema| {
            Entry::Vacant(RelationId::new(CATALOG, schema, probe.created(&lookup.name)))
        });
        entries.push((name, entry));
    }
    Ok(entries)
}

/// Labels the error number and `SQLSTATE` of an error that may echo a value.
///
/// Timeouts and non-database errors return `None`.
fn echoing(error: &sqlx::Error) -> Option<String> {
    if MysqlConnection::is_timeout(error) {
        return None;
    }
    let error = error.as_database_error()?.try_downcast_ref::<MySqlDatabaseError>()?;
    let number = error.number();
    (!VALUE_FREE_ERRORS.contains(&number))
        .then(|| format!("error {number}, SQLSTATE {}", error.code().unwrap_or("HY000")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe(lower_case: i64) -> Probe {
        Probe {
            database: Some("app".into()),
            lower_case,
            sql_mode: String::new(),
            charset: "utf8mb4".into(),
            mariadb: false,
        }
    }

    fn session(sql_mode: &str, charset: &str) -> Probe {
        Probe {
            sql_mode: sql_mode.into(),
            charset: charset.into(),
            ..probe(0)
        }
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
        assert!(matches!(
            probe(2)
                .branch(&Lookup {
                    schema: "App".into(),
                    name: "T".into()
                })
                .0,
            Match::Folded
        ));
    }

    #[test]
    fn ansi_quotes_and_no_backslash_escapes_mismatch_the_parser() {
        let mode = |sql_mode| session(sql_mode, "utf8mb4").mismatch("SELECT 1");
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
            assert!(session("", charset).mismatch("SELECT 1").is_some(), "{charset}");
        }
        assert_eq!(session("", "latin1").mismatch("SELECT 1"), None);
    }

    #[test]
    fn variables_mismatch_the_parser() {
        let sql = |sql| probe(0).mismatch(sql);
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
                column("id", "auto_increment", false, false),
                column("secret", "INVISIBLE", false, false),
            ],
            ..Found::default()
        }
    }

    #[test]
    fn entries_carry_the_triggers_and_cascades_found() {
        let cascade = Cascade {
            parent: SourceRef::new(RelationId::new(CATALOG, "app", "r"), "id"),
            child: SourceRef::new(RelationId::new(CATALOG, "app", "c"), "rid"),
            action: CascadeAction::Copy,
            on_delete: false,
            triggers: false,
        };
        let plain = resolved("app", &found("BASE TABLE", None)).expect("table");
        assert!(!plain.triggers && plain.cascades.is_empty(), "{plain:?}");
        let mut table = found("BASE TABLE", None);
        table.triggers = true;
        table.cascades = vec![cascade.clone()];
        let relation = resolved("app", &table).expect("table");
        assert!(relation.triggers);
        assert_eq!(relation.cascades, [cascade]);
    }

    fn column(name: &str, extra: &str, default: bool, generated: bool) -> FoundColumn {
        FoundColumn {
            name: name.into(),
            extra: extra.into(),
            default,
            generated,
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
            vec![
                CatalogColumn {
                    name: "id".into(),
                    star_visible: true,
                    default: true,
                    generated: false,
                    on_update: false,
                },
                CatalogColumn {
                    name: "secret".into(),
                    star_visible: false,
                    default: false,
                    generated: false,
                    on_update: false,
                }
            ]
        );
    }

    #[test]
    fn column_facts_follow_defaults_generation_and_extra_flags() {
        let mut table = found("BASE TABLE", None);
        table.columns = vec![
            column("plain", "", false, false),
            column("dflt", "", true, false),
            column("now", "DEFAULT_GENERATED", false, false),
            column("gen", "STORED GENERATED", false, true),
            column("mysql_ts", "DEFAULT_GENERATED on update CURRENT_TIMESTAMP", true, false),
            column("maria_ts", "on update current_timestamp()", true, false),
        ];
        let relation = resolved("app", &table).expect("table");
        let facts: Vec<_> = relation
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.default, column.generated, column.on_update))
            .collect();
        assert_eq!(
            facts,
            [
                ("plain", false, false, false),
                ("dflt", true, false, false),
                ("now", true, false, false),
                ("gen", false, true, false),
                ("mysql_ts", true, false, true),
                ("maria_ts", true, false, true),
            ]
        );
    }

    #[test]
    fn mariadb_spells_default_null_as_unquoted_text() {
        assert!(!has_default(None, false));
        assert!(!has_default(None, true));
        assert!(has_default(Some("NULL"), false));
        assert!(!has_default(Some("NULL"), true));
        assert!(has_default(Some("'NULL'"), true));
        assert!(has_default(Some("0"), true));
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

    #[test]
    fn value_echoing_errors_are_not_value_free() {
        for number in [1062, 1264, 1366, 1406, 1690, 3140, 3141] {
            assert!(!VALUE_FREE_ERRORS.contains(&number), "{number}");
        }
    }

    #[test]
    fn value_free_errors_are_unique() {
        let mut numbers = VALUE_FREE_ERRORS.to_vec();
        numbers.sort_unstable();
        numbers.dedup();
        assert_eq!(numbers.len(), VALUE_FREE_ERRORS.len(), "duplicate error");
    }

    /// Tests against the seeded database; `#[ignore]`d outside `./tests/run.sh` and CI.
    mod db {
        use super::*;

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
                Ok("mariadb") => database::Backend::Mariadb,
                _ => database::Backend::Mysql,
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

        #[tokio::test]
        #[ignore = "needs the seeded MySQL or MariaDB container: ./tests/run.sh --filter mysql"]
        async fn opposite_backend_refuses_the_probe() {
            let (configured, server) = match std::env::var("DBMCP_DB_BACKEND").as_deref() {
                Ok("mariadb") => (database::Backend::Mysql, database::Backend::Mariadb),
                _ => (database::Backend::Mariadb, database::Backend::Mysql),
            };
            let mut conn = seeded().await;
            let stop = Probe::read(&mut conn, configured)
                .await
                .err()
                .expect("mismatch refuses");
            assert!(
                matches!(stop, Stop::Refused(Refusal::BackendMismatch(BackendMismatch { configured: c, server: s })) if c == configured && s == server),
                "{stop:?}"
            );
        }
    }
}
