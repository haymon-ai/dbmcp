//! Column rule (`privacy.rules`) enforcement on `SQLite`.
//!
//! With column rules, `read`, `write`, and `explain` run each
//! statement through [`SqliteHandler::enforce`]. On one connection, inside
//! one transaction (`BEGIN IMMEDIATE` for writes, so the write lock is held
//! before anything runs; reads under `PRAGMA query_only`) and under one
//! deadline, it:
//!
//! 1. analyzes the statement's lineage, fetching the relations it reports
//!    missing (`pragma_table_list`, `pragma_table_xinfo`, view bodies and
//!    triggers from `sqlite_schema`, referential actions from
//!    `pragma_foreign_key_list`) for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds,
//! 2. plans it with [`Analyzed::plan`](dbmcp_policy::Analyzed::plan); a rejection runs nothing,
//! 3. executes it, refusing a result that does not fit the plan before any
//!    row is kept or a write commits,
//! 4. commits a write or rolls a read back.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors whose extended code may echo a value are
//! replaced by a generic message.

use std::collections::{BTreeMap, BTreeSet};
use std::convert::Infallible;

use dbmcp_policy::{
    Access, Analysis, Analyzed, Cascade, CascadeAction, CatalogColumn, Enforced, Entry, Executed, Family, Origin,
    Planned, Relation, RelationId, RelationKind, RelationRef, Run, SourceRef, Step, WrittenName, admit, settle,
};
use dbmcp_sql::Connection as _;
use dbmcp_truncate::Window;
use dbmcp_types::{database, privacy};
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use sqlx::sqlite::{SqliteColumn, SqliteRow};
use sqlx::{Column as _, ColumnOrigin, Row as _};

use crate::SqliteHandler;
use crate::connection::SqliteConnection;
use crate::queries;

/// Catalog segment of every `SQLite` rule key.
const CATALOG: &str = "main";

/// Extended result codes whose messages name schema objects, never values.
///
/// `CHECK`, `FOREIGNKEY`, `NOTNULL`, `PRIMARYKEY`, `UNIQUE`, `ROWID`, and
/// `DATATYPE` constraint failures. `TRIGGER` is left out: its message is
/// whatever the trigger raised.
const VALUE_FREE_CODES: &[i32] = &[275, 787, 1299, 1555, 2067, 2579, 3091];

/// `SQLITE_ERROR` messages of the JSON functions that carry no value.
const VALUE_FREE_JSON: &[&str] = &["malformed JSON", "JSON cannot hold BLOB values"];

type Stop = dbmcp_policy::Stop<Infallible>;

/// A table or view the catalog lists.
struct Listed {
    schema: String,
    name: String,
    /// `None` for a type this module does not know: it exists, but cannot be described.
    kind: Option<RelationKind>,
}

impl SqliteHandler {
    /// Plans `sql` under `policy`'s rules, runs it into `window`, and redacts the kept rows.
    ///
    /// `sql` is the text that runs. The window holds each row as a JSON
    /// array of its values; the kept rows come back redacted, as objects.
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
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        let read_only = self.connection.read_only();
        let admitted = admit(self.redactor.as_ref(), access, read_only)?;
        let ran = self
            .connection
            .with_connection(sql, None, timeout_secs, async |conn| {
                Stop::split(run(conn, read_only, policy, sql, access, &mut |row| window.push(row)).await)
            })
            .await;
        Ok(admitted.finish(ran, window).await?)
    }
}

/// Plans and executes one statement inside a transaction on `conn`.
async fn run<'c>(
    conn: &mut sqlx::SqliteConnection,
    read_only: bool,
    config: &'c privacy::Config,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Run<'c>, Stop> {
    // A file opened read-only needs no pragma; `before_acquire` switches it off again.
    if access == Access::Read && !read_only {
        queries::session::query_only(&mut *conn, true).await?;
    }
    let mut tx = queries::policy::begin(conn, access).await?;
    let analyzed = resolve(&mut tx, sql).await?;
    let mut planned = analyzed.plan(config)?;
    let executed = execute(&mut tx, sql, &mut planned, sink).await;
    settle(Some(tx), access, planned, echoing, executed).await
}

/// Runs `sql`, feeding each row's values to `sink`.
///
/// The first row is checked against `planned` before any row reaches `sink`.
async fn execute(
    conn: &mut sqlx::SqliteConnection,
    sql: &str,
    planned: &mut Planned<'_>,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, Stop> {
    let first =
        |row: &SqliteRow, names: &[String]| planned.check_first(names, origins(row.columns())).map_err(Stop::from);
    let (names, rows_affected, last_insert_id) = SqliteConnection::execute_checked(conn, sql, first, sink).await?;
    Ok(Executed {
        names,
        rows_affected,
        last_insert_id,
    })
}

/// Translates each result column's base-table origin, one entry per reading.
///
/// `SQLite` reports `table` in `main` and `schema.table` elsewhere; a table
/// name holding a `.` reads both ways.
fn origins(columns: &[SqliteColumn]) -> Vec<(usize, RelationId, Option<String>)> {
    let mut origins = Vec::new();
    for column in columns {
        let ColumnOrigin::Table(origin) = column.origin() else {
            continue;
        };
        for (schema, table) in readings(&origin.table) {
            let relation = RelationId::new(CATALOG, schema, table);
            origins.push((column.ordinal(), relation, Some(origin.name.to_string())));
        }
    }
    origins
}

/// Splits an origin table into each `(schema, table)` reading it allows.
fn readings(table: &str) -> impl Iterator<Item = (&str, &str)> {
    std::iter::once(("main", table)).chain(
        table
            .match_indices('.')
            .map(move |(dot, _)| (&table[..dot], &table[dot + 1..])),
    )
}

/// Analyzes `sql`, fetching missing relations for at most [`CATALOG_ROUNDS`](dbmcp_policy::CATALOG_ROUNDS) rounds.
async fn resolve(conn: &mut sqlx::SqliteConnection, sql: &str) -> Result<Analyzed, Stop> {
    let schemas = queries::policy::schemas(&mut *conn).await?;
    let mut analysis = Analysis::new(sql, database::Backend::Sqlite);
    analysis.creates_in(Some("main".into()));
    let mut entries = Vec::new();
    loop {
        match analysis.step(entries)? {
            Step::Missing(names) => entries = fetch(conn, &schemas, names).await?,
            Step::Done(analyzed) => return Ok(analyzed),
        }
    }
}

/// Lists the relations named `tables` in any schema, in name-resolution order.
async fn lookup(conn: &mut sqlx::SqliteConnection, tables: &[&str]) -> Result<Vec<Listed>, sqlx::Error> {
    Ok(queries::policy::lookup(conn, tables)
        .await?
        .into_iter()
        .map(|(schema, name, kind)| Listed {
            schema,
            name,
            kind: relation_kind(&kind),
        })
        .collect())
}

/// Shadow tables hold raw copies of virtual-table data, so they count as
/// virtual. Any other type is unknown: it lists, but cannot be described.
fn relation_kind(kind: &str) -> Option<RelationKind> {
    match kind {
        "table" => Some(RelationKind::Table),
        "view" => Some(RelationKind::View),
        "virtual" | "shadow" => Some(RelationKind::Virtual),
        _ => None,
    }
}

/// Finds the relation `reference` resolves to, as `SQLite` would.
///
/// A reference without a schema searches `temp`, `main`, then attached
/// schemas, the order `listed` comes in.
fn locate<'a>(listed: &'a [Listed], reference: &RelationRef) -> Option<&'a Listed> {
    listed.iter().find(|relation| {
        relation.name.eq_ignore_ascii_case(&reference.name)
            && reference
                .schema
                .as_deref()
                .is_none_or(|schema| relation.schema.eq_ignore_ascii_case(schema))
    })
}

/// Returns the entry for a name nothing is listed under.
///
/// A relation created under a bare name lands in `main`; under a qualified
/// name, in that schema when it exists. Names in view bodies are absent.
fn vacant(schemas: &[String], context: &Origin, reference: &RelationRef) -> Entry {
    if *context != Origin::Query {
        return Entry::Absent;
    }
    let schema = match &reference.schema {
        None => Some("main"),
        Some(schema) => schemas
            .iter()
            .find(|known| known.eq_ignore_ascii_case(schema))
            .map(String::as_str),
    };
    schema.map_or(Entry::Absent, |schema| {
        Entry::Vacant(RelationId::new(CATALOG, schema, &reference.name))
    })
}

/// Builds a catalog entry for each of `names`, read in batched queries.
async fn fetch(
    conn: &mut sqlx::SqliteConnection,
    schemas: &[String],
    names: Vec<(WrittenName, RelationRef)>,
) -> Result<Vec<(WrittenName, Entry)>, sqlx::Error> {
    let tables: Vec<&str> = names.iter().map(|(_, reference)| reference.name.as_str()).collect();
    let listed = lookup(conn, &tables).await?;
    let mut entries = Vec::new();
    let mut found: Vec<(WrittenName, &Listed, RelationKind)> = Vec::new();
    for (name, reference) in names {
        let Some(relation) = locate(&listed, &reference) else {
            let entry = vacant(schemas, &name.context, &reference);
            entries.push((name, entry));
            continue;
        };
        match relation.kind {
            Some(kind) => found.push((name, relation, kind)),
            // Exists, but cannot be described: never vacant, so `IF EXISTS` cannot skip it.
            None => entries.push((name, Entry::Absent)),
        }
    }
    if found.is_empty() {
        return Ok(entries);
    }
    let pairs: Vec<(&str, &str)> = found
        .iter()
        .map(|(_, relation, _)| (relation.schema.as_str(), relation.name.as_str()))
        .collect();
    let rows = queries::policy::columns(&mut *conn, &pairs).await?;
    let mut triggers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (index, (name, listed, kind)) in found.into_iter().enumerate() {
        let columns = rows
            .iter()
            .filter(|(key, ..)| usize::try_from(*key).is_ok_and(|key| key == index))
            .map(|(_, column, hidden, default)| {
                // `hidden` = 1 marks virtual-table hidden columns; 2 and 3 are generated columns.
                CatalogColumn {
                    name: column.clone(),
                    star_visible: *hidden != 1,
                    default: *default,
                    generated: matches!(hidden, 2 | 3),
                    // `SQLite` has no on-update columns.
                    on_update: false,
                }
            })
            .collect();
        if !triggers.contains_key(&listed.schema) {
            let names = triggered(conn, &listed.schema).await?;
            triggers.insert(listed.schema.clone(), names);
        }
        let triggered = &triggers[&listed.schema];
        let cascades = if kind == RelationKind::Table {
            cascades(conn, listed, triggered).await?
        } else {
            Vec::new()
        };
        let view_sql = if kind == RelationKind::View {
            let Some(body) = view_body(conn, listed).await? else {
                entries.push((name, Entry::Absent));
                continue;
            };
            Some(body)
        } else {
            None
        };
        let relation = Relation {
            id: RelationId::new(CATALOG, &listed.schema, &listed.name),
            kind,
            columns,
            view_sql,
            // `SQLite` has no inheritance.
            family: Family::default(),
            triggers: triggered.contains(&listed.name.to_lowercase()),
            cascades,
        };
        entries.push((name, Entry::Resolved(relation)));
    }
    Ok(entries)
}

/// Lists lowercased names of tables in `schema` a trigger may fire on.
async fn triggered(conn: &mut sqlx::SqliteConnection, schema: &str) -> Result<BTreeSet<String>, sqlx::Error> {
    let names = queries::policy::triggered(conn, schema).await?;
    Ok(names.into_iter().map(|name| name.to_lowercase()).collect())
}

/// Reads the referential actions below `table`, each child's triggers from `triggered`.
async fn cascades(
    conn: &mut sqlx::SqliteConnection,
    table: &Listed,
    triggered: &BTreeSet<String>,
) -> Result<Vec<Cascade>, sqlx::Error> {
    let rows = queries::policy::cascades(conn, &table.schema, &table.name).await?;
    let column =
        |relation: &str, column: String| SourceRef::new(RelationId::new(CATALOG, &table.schema, relation), column);
    Ok(rows
        .into_iter()
        .map(|(parent, pcol, child, ccol, rule, on_delete)| Cascade {
            action: match rule.as_str() {
                "CASCADE" if on_delete => CascadeAction::Delete,
                "CASCADE" => CascadeAction::Copy,
                "SET NULL" => CascadeAction::Null,
                _ => CascadeAction::Default,
            },
            on_delete,
            triggers: triggered.contains(&child.to_lowercase()),
            parent: column(&parent, pcol),
            child: column(&child, ccol),
        })
        .collect())
}

/// Reads the query of a view's `CREATE VIEW` statement; `None` when unreadable.
async fn view_body(conn: &mut sqlx::SqliteConnection, view: &Listed) -> Result<Option<String>, sqlx::Error> {
    let create = queries::policy::view_body(conn, &view.schema, &view.name).await?;
    Ok(create.as_deref().and_then(view_query))
}

fn view_query(create: &str) -> Option<String> {
    match Parser::parse_sql(&SQLiteDialect {}, create).ok()?.as_slice() {
        [Statement::CreateView(view)] => Some(view.query.to_string()),
        _ => None,
    }
}

/// Labels the extended code of a database error that may echo a value.
///
/// Timeouts and non-database errors return `None`.
fn echoing(error: &sqlx::Error) -> Option<String> {
    if SqliteConnection::is_timeout(error) {
        return None;
    }
    let error = error.as_database_error()?;
    let code = error.code()?;
    let value_free = code.parse::<i32>().is_ok_and(|code| {
        VALUE_FREE_CODES.contains(&code) || (code == 1 && VALUE_FREE_JSON.contains(&error.message()))
    });
    (!value_free).then(|| format!("SQLite code {code}"))
}

#[cfg(test)]
mod tests {
    use dbmcp_policy::{NamePart, Unfit};
    use dbmcp_privacy::cells::ShapeError;
    use dbmcp_sql::SqlError;
    use dbmcp_types::{Config, Transport};
    use sqlx::{AssertSqlSafe, Connection as _};

    use super::*;
    use crate::types::{ReadRequest, WriteRequest};

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// `users.email` and `users.ssn` are ruled by [`rules_config`].
    const SETUP: &str = "CREATE TABLE users (id INTEGER PRIMARY KEY, email TEXT, name TEXT, ssn TEXT);
         INSERT INTO users (email, name, ssn) VALUES
             ('alice@example.com', 'Alice', '123-45-6789'),
             ('bob@example.com', 'Bob', NULL),
             ('carol@example.com', 'Carol', NULL);
         CREATE TABLE audit (id INTEGER PRIMARY KEY, note TEXT);
         INSERT INTO audit (note) VALUES ('one'), ('two');";

    fn listed() -> Vec<Listed> {
        [
            ("temp", "t", RelationKind::Table),
            ("main", "users", RelationKind::Table),
            ("main", "t", RelationKind::Table),
            ("aux", "users", RelationKind::Table),
            ("aux", "only_aux", RelationKind::Table),
        ]
        .into_iter()
        .map(|(schema, name, kind)| Listed {
            schema: schema.into(),
            name: name.into(),
            kind: Some(kind),
        })
        .collect()
    }

    /// A reference to `name`, in `schema` when given.
    fn reference(schema: Option<&str>, name: &str) -> RelationRef {
        RelationRef {
            schema: schema.map(Into::into),
            name: name.into(),
        }
    }

    fn located(schema: Option<&str>, name: &str) -> Option<String> {
        locate(&listed(), &reference(schema, name)).map(|relation| format!("{}.{}", relation.schema, relation.name))
    }

    fn column_rule(key: [&str; 4], entity: &'static str) -> privacy::ColumnRule {
        privacy::ColumnRule {
            name: "rule".into(),
            item: 0,
            key: key.map(String::from),
            entity: entity.parse().expect("known entity"),
            operator: privacy::Operator::Hash,
        }
    }

    fn rules_config() -> privacy::Config {
        privacy::Config {
            key: Some(KEY.into()),
            rules: vec![
                column_rule(["main", "main", "users", "email"], "EMAIL_ADDRESS"),
                column_rule(["main", "main", "users", "ssn"], "US_SSN"),
            ],
            ..privacy::Config::default()
        }
    }

    /// Builds a handler on a fresh in-memory database holding [`SETUP`] when writable.
    async fn handler(read_only: bool, privacy: privacy::Config) -> SqliteHandler {
        let handler = SqliteHandler::new(&Config {
            transport: Transport::Stdio,
            database: database::Config {
                backend: database::Backend::Sqlite,
                name: Some(":memory:".into()),
                read_only,
                ..database::Config::default()
            },
            http: None,
            privacy: Some(privacy),
            ssh: None,
        })
        .expect("handler builds");
        if !read_only {
            sqlx::raw_sql(SETUP)
                .execute(&handler.connection.pool(None).await.expect("pool"))
                .await
                .expect("setup");
        }
        handler
    }

    async fn read(handler: &SqliteHandler, query: &str) -> Result<Vec<Value>, ErrorData> {
        handler
            .read(ReadRequest {
                query: query.into(),
                cursor: None,
                timeout: None,
            })
            .await
            .map(|response| response.rows)
    }

    async fn write(handler: &SqliteHandler, query: &str) -> Result<Vec<Value>, ErrorData> {
        handler
            .write(WriteRequest { query: query.into() })
            .await
            .map(|response| response.rows)
    }

    async fn memory(setup: &str) -> sqlx::SqliteConnection {
        let mut conn = sqlx::SqliteConnection::connect(":memory:").await.expect("connect");
        sqlx::raw_sql(AssertSqlSafe(setup.to_owned()))
            .execute(&mut conn)
            .await
            .expect("setup");
        conn
    }

    #[test]
    fn bare_names_search_temp_main_then_attached() {
        assert_eq!(located(None, "T"), Some("temp.t".into()));
        assert_eq!(located(None, "users"), Some("main.users".into()));
        assert_eq!(located(None, "only_aux"), Some("aux.only_aux".into()));
        assert_eq!(located(None, "missing"), None);
    }

    #[test]
    fn qualified_names_pick_their_schema() {
        assert_eq!(located(Some("AUX"), "users"), Some("aux.users".into()));
        assert_eq!(located(Some("main"), "t"), Some("main.t".into()));
        assert_eq!(located(Some("other"), "t"), None);
    }

    #[test]
    fn unlisted_names_are_vacant_where_sqlite_would_create_them() {
        let schemas: Vec<String> = ["main", "temp", "aux"].map(String::from).to_vec();
        let query = Origin::Query;
        let id = |context: &Origin, schema, table| match vacant(&schemas, context, &reference(schema, table)) {
            Entry::Vacant(id) => Some(format!("{}.{}.{}", id.catalog, id.schema, id.name)),
            _ => None,
        };
        assert_eq!(id(&query, None, "New").as_deref(), Some("main.main.New"));
        assert_eq!(id(&query, Some("AUX"), "n").as_deref(), Some("main.aux.n"));
        assert_eq!(id(&query, Some("nope"), "n"), None);
        let in_view = Origin::View(RelationId::new(CATALOG, "main", "v"));
        assert_eq!(id(&in_view, Some("main"), "n"), None);
    }

    #[test]
    fn unknown_relation_types_list_without_a_kind() {
        assert_eq!(relation_kind("table"), Some(RelationKind::Table));
        assert_eq!(relation_kind("view"), Some(RelationKind::View));
        assert_eq!(relation_kind("shadow"), Some(RelationKind::Virtual));
        assert_eq!(relation_kind("index"), None);
    }

    #[test]
    fn view_query_strips_the_create_statement() {
        assert_eq!(
            view_query("CREATE VIEW v(a) AS SELECT id FROM users WHERE ssn IS NOT NULL").as_deref(),
            Some("SELECT id FROM users WHERE ssn IS NOT NULL")
        );
        assert_eq!(view_query("CREATE TABLE t (a)"), None);
        assert_eq!(view_query("not sql"), None);
    }

    #[test]
    fn origin_tables_read_every_schema_split() {
        assert_eq!(readings("users").collect::<Vec<_>>(), [("main", "users")]);
        assert_eq!(
            readings("a.b.c").collect::<Vec<_>>(),
            [("main", "a.b.c"), ("a", "b.c"), ("a.b", "c")]
        );
    }

    async fn error_of(setup: &str, sql: &str) -> sqlx::Error {
        let mut conn = memory(setup).await;
        sqlx::raw_sql(AssertSqlSafe(sql.to_owned()))
            .execute(&mut conn)
            .await
            .expect_err("statement fails")
    }

    #[tokio::test]
    async fn value_echoing_errors_are_withheld() {
        let error = error_of("SELECT 1", "SELECT json_extract('{}', 'secret')").await;
        assert!(error.to_string().contains("secret"), "{error}");
        assert_eq!(echoing(&error).as_deref(), Some("SQLite code 1"));
    }

    #[tokio::test]
    async fn constraint_and_json_errors_pass() {
        let unique = "CREATE TABLE t (a UNIQUE, b NOT NULL); INSERT INTO t VALUES (1, 1)";
        for sql in [
            "INSERT INTO t VALUES (1, 2)",
            "INSERT INTO t VALUES (2, NULL)",
            "SELECT json('not json')",
        ] {
            let error = error_of(unique, sql).await;
            assert_eq!(echoing(&error), None, "{sql}: {error}");
        }
    }

    #[tokio::test]
    async fn trigger_raised_errors_are_withheld() {
        let setup = "CREATE TABLE t (a); \
             CREATE TRIGGER g BEFORE INSERT ON t BEGIN SELECT RAISE(ABORT, 'no'); END";
        let error = error_of(setup, "INSERT INTO t VALUES (1)").await;
        assert_eq!(echoing(&error).as_deref(), Some("SQLite code 1811"));
    }

    #[tokio::test]
    async fn fetch_describes_hidden_generated_and_view_columns() {
        let mut conn = memory(
            "CREATE TABLE g (a INTEGER, b INTEGER GENERATED ALWAYS AS (a + 1), id INTEGER PRIMARY KEY, c DEFAULT 'x');
             CREATE VIRTUAL TABLE f USING fts5(body);
             CREATE VIEW v(x) AS SELECT a FROM g",
        )
        .await;
        let schemas = vec!["main".to_owned(), "temp".to_owned()];
        let names = ["g", "f", "v"].map(|table| WrittenName::query(vec![NamePart::bare(table)]));
        let missing = names
            .iter()
            .map(|name| (name.clone(), reference(None, &name.parts[0].value)));
        let entries = fetch(&mut conn, &schemas, missing.collect()).await.expect("fetch");
        let resolved = |name: &WrittenName| match entries.iter().find(|(written, _)| written == name) {
            Some((_, Entry::Resolved(relation))) => relation.clone(),
            other => panic!("{name:?}: {other:?}"),
        };
        let generated = resolved(&names[0]);
        assert!(
            generated.columns.iter().all(|column| column.star_visible),
            "{generated:?}"
        );
        let facts: Vec<_> = generated
            .columns
            .iter()
            .map(|column| (column.name.as_str(), column.default, column.generated, column.on_update))
            .collect();
        assert_eq!(
            facts,
            [
                ("a", false, false, false),
                ("b", false, true, false),
                ("id", true, false, false),
                ("c", true, false, false)
            ]
        );
        let fts = resolved(&names[1]);
        assert_eq!(fts.kind, RelationKind::Virtual);
        assert!(
            fts.columns
                .iter()
                .any(|column| !column.star_visible && column.name == "rank"),
            "{fts:?}"
        );
        let view = resolved(&names[2]);
        assert_eq!(view.view_sql.as_deref(), Some("SELECT a FROM g"));
    }

    #[tokio::test]
    async fn fetch_reads_triggers_and_the_cascades_below_a_table() {
        let mut conn = memory(
            "CREATE TABLE p (id INTEGER PRIMARY KEY, k TEXT UNIQUE);
             CREATE TABLE c (id INTEGER PRIMARY KEY, k TEXT REFERENCES P (K) ON UPDATE CASCADE ON DELETE SET NULL,
               pid INTEGER REFERENCES p ON DELETE CASCADE);
             CREATE TABLE g (id INTEGER PRIMARY KEY, ck TEXT REFERENCES c (k) ON UPDATE SET DEFAULT ON DELETE RESTRICT,
               cid INTEGER REFERENCES c (id) ON UPDATE CASCADE ON DELETE SET DEFAULT);
             CREATE TABLE gg (id INTEGER, k TEXT REFERENCES g (ck) ON UPDATE CASCADE,
               gid INTEGER REFERENCES g ON DELETE CASCADE);
             CREATE TRIGGER g_insert AFTER INSERT ON G BEGIN SELECT 1; END;
             CREATE TEMP TRIGGER p_delete AFTER DELETE ON main.p BEGIN SELECT 1; END",
        )
        .await;
        let schemas = vec!["main".to_owned(), "temp".to_owned()];
        let names = ["p", "c"].map(|table| WrittenName::query(vec![NamePart::bare(table)]));
        let missing = names
            .iter()
            .map(|name| (name.clone(), reference(None, &name.parts[0].value)));
        let entries = fetch(&mut conn, &schemas, missing.collect()).await.expect("fetch");
        let resolved = |index: usize| match &entries[index] {
            (_, Entry::Resolved(relation)) => relation.clone(),
            other => panic!("{other:?}"),
        };
        let (parent, child) = (resolved(0), resolved(1));
        assert!(parent.triggers, "temporary trigger");
        assert!(!child.triggers);
        let mut edges: Vec<_> = parent
            .cascades
            .iter()
            .map(|edge| {
                (
                    format!("{}.{}", edge.parent.relation.name, edge.parent.column),
                    format!("{}.{}", edge.child.relation.name, edge.child.column),
                    edge.action,
                    edge.on_delete,
                    edge.triggers,
                )
            })
            .collect();
        edges.sort_by(|a, b| format!("{a:?}").cmp(&format!("{b:?}")));
        let edge = |parent: &str, child: &str, action, on_delete, triggers| {
            (parent.to_owned(), child.to_owned(), action, on_delete, triggers)
        };
        assert_eq!(
            edges,
            [
                // The walk takes every edge of each reached table; no write
                // through `p` fires this one (`c.id` never changes).
                edge("c.id", "g.cid", CascadeAction::Copy, false, true),
                edge("c.id", "g.cid", CascadeAction::Default, true, true),
                edge("c.k", "g.ck", CascadeAction::Default, false, true),
                edge("g.ck", "gg.k", CascadeAction::Copy, false, false),
                // Nor this one: no `g` row is ever removed.
                edge("g.id", "gg.gid", CascadeAction::Delete, true, false),
                edge("p.id", "c.pid", CascadeAction::Delete, true, false),
                edge("p.k", "c.k", CascadeAction::Copy, false, false),
                edge("p.k", "c.k", CascadeAction::Null, true, false),
            ]
        );
    }

    #[tokio::test]
    async fn temp_tables_shadow_main_for_bare_names() {
        let mut conn = memory("CREATE TABLE t (a); CREATE TEMP TABLE t (b)").await;
        let listed = lookup(&mut conn, &["T"]).await.expect("lookup");
        let found = locate(&listed, &reference(None, "t")).expect("listed");
        assert_eq!((found.schema.as_str(), found.name.as_str()), ("temp", "t"));
    }

    #[tokio::test]
    async fn origins_name_each_result_columns_base_table() {
        let mut conn = memory(SETUP).await;
        let row = sqlx::query("SELECT name, email, 1 FROM users")
            .fetch_one(&mut conn)
            .await
            .expect("row");
        let users = RelationId::new(CATALOG, "main", "users");
        assert_eq!(
            origins(row.columns()),
            [
                (0, users.clone(), Some("name".to_owned())),
                (1, users, Some("email".to_owned())),
            ]
        );
    }

    #[tokio::test]
    async fn results_that_do_not_fit_the_plan_stop_before_any_row() {
        let mut conn = memory(SETUP).await;
        let mut tx = sqlx::Connection::begin(&mut conn).await.expect("begin");
        let rules = rules_config();
        // A one-column plan from another statement.
        let analyzed = resolve(&mut tx, "SELECT id FROM audit").await.expect("resolve");
        let mut one = analyzed.plan(&rules).expect("plan");
        let mut sunk = 0;
        let stop = execute(
            &mut tx,
            "UPDATE audit SET note = 'x' WHERE id = 1 RETURNING id, note",
            &mut one,
            &mut |_| sunk += 1,
        )
        .await
        .expect_err("shape refused");
        assert!(
            matches!(
                stop,
                Stop::Unfit(Unfit::Shape(ShapeError::ColumnCount { expected: 1, actual: 2 }))
            ),
            "{stop}"
        );
        assert_eq!(sunk, 0);
    }

    #[tokio::test]
    async fn rules_without_a_redactor_refuse_every_statement() {
        let mut handler = handler(false, rules_config()).await;
        handler.redactor = None;
        let error = read(&handler, "SELECT note FROM audit").await.expect_err("fail closed");
        assert!(error.message.contains("column policy"), "{}", error.message);
    }

    #[tokio::test]
    async fn reads_run_under_query_only() {
        let mut conn = memory(SETUP).await;
        let error = run(
            &mut conn,
            false,
            &rules_config(),
            "INSERT INTO audit (note) VALUES ('x')",
            Access::Read,
            &mut |_| {},
        )
        .await
        .expect_err("query_only rejects the write");
        assert!(error.to_string().contains("readonly"), "{error}");
    }

    #[tokio::test]
    async fn ctas_into_a_temp_shadowed_name_is_refused() {
        let handler = handler(false, rules_config()).await;
        let pool = handler.connection.pool(None).await.expect("pool");
        sqlx::raw_sql("CREATE TEMP TABLE copy (id INTEGER)")
            .execute(&pool)
            .await
            .expect("temp table");
        write(&handler, "CREATE TABLE fresh AS SELECT id FROM users")
            .await
            .expect("unshadowed CTAS plans");
        write(&handler, "CREATE TABLE temp.scratch AS SELECT id FROM users")
            .await
            .expect("qualified CTAS plans outside main");
        for sql in [
            "CREATE TABLE copy AS SELECT id FROM users",
            "CREATE VIEW copy AS SELECT id FROM users",
        ] {
            let error = write(&handler, sql).await.expect_err("guard");
            assert!(
                error
                    .message
                    .contains("create target \"temp.copy\" resolves to another relation"),
                "{sql}: {}",
                error.message
            );
        }
    }

    #[tokio::test]
    async fn dropping_a_missing_table_if_exists_plans() {
        let handler = handler(false, rules_config()).await;
        write(&handler, "DROP TABLE IF EXISTS nope").await.expect("no target");
    }

    #[tokio::test]
    async fn timeouts_under_rules_still_report_query_timeout() {
        let handler = handler(false, rules_config()).await;
        let pool = handler.connection.pool(None).await.expect("pool");
        sqlx::raw_sql(
            "CREATE TABLE n (x INTEGER);
             WITH RECURSIVE c(x) AS (SELECT 1 UNION ALL SELECT x + 1 FROM c WHERE x < 1000)
             INSERT INTO n SELECT x FROM c",
        )
        .execute(&pool)
        .await
        .expect("setup");
        let error = handler
            .read(ReadRequest {
                query: "SELECT count(*) AS c FROM n a, n b, n c".into(),
                cursor: None,
                timeout: Some(1),
            })
            .await
            .expect_err("times out");
        assert!(error.message.contains("timed out"), "{}", error.message);
    }

    #[tokio::test]
    async fn writes_are_refused_in_read_only_mode() {
        let handler = handler(true, rules_config()).await;
        let error = write(&handler, "CREATE TABLE t (a)").await.expect_err("read-only");
        assert_eq!(error.message, SqlError::ReadOnlyViolation.to_string());
    }

    #[tokio::test]
    async fn policy_exists_only_with_rules() {
        assert!(handler(false, rules_config()).await.policy.is_some());
        assert!(handler(false, privacy::Config::default()).await.policy.is_none());
    }
}
