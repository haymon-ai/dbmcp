//! Column rule (`pii.rules`) enforcement on `SQLite`.
//!
//! With column rules, `readQuery`, `writeQuery`, and `explainQuery` run each
//! statement through [`SqliteHandler::enforce`]. On one connection, inside
//! one transaction (`BEGIN IMMEDIATE` for writes, so the write lock is held
//! before anything runs; reads under `PRAGMA query_only`) and under one
//! deadline, it:
//!
//! 1. lists the attached schemas (`pragma_database_list`),
//! 2. analyzes the statement's lineage, fetching the relations it reports
//!    missing (`pragma_table_list(name)`, `pragma_table_xinfo`, view bodies
//!    from `sqlite_schema`) for at most [`CATALOG_ROUNDS`] rounds,
//! 3. plans it with [`dbmcp_pii::policy::plan`]; a rejection runs nothing,
//! 4. executes the statement, refusing a result whose server-reported column
//!    origin is a ruled column the plan does not rewrite, and commits a write
//!    or rolls a read back.
//!
//! The rows are then redacted by position. When the statement touched a
//! ruled relation, database errors whose extended code may echo a value are
//! replaced by a generic message.

use std::mem;

use dbmcp_config::{ColumnRule, PiiConfig};
use dbmcp_pii::cells::{Cells, ShapeError, check_columns};
use dbmcp_pii::policy::{CATALOG_ROUNDS, LineageClass, Plan, Position, Rejection, reject_lineage, touches_ruled};
use dbmcp_server::truncate::Window;
use dbmcp_sql::{Connection as _, SqlError};
use dbmcp_sql_lineage::{
    Catalog, CatalogColumn, Dialect, Entry, Lineage, LineageError, Origin, Relation, RelationId, RelationKind,
    StatementKind, WrittenName, analyze,
};
use futures_util::TryStreamExt as _;
use rmcp::model::ErrorData;
use serde_json::Value;
use sqlparser::ast::Statement;
use sqlparser::dialect::SQLiteDialect;
use sqlparser::parser::Parser;
use sqlx::sqlite::{SqliteColumn, SqliteQueryResult};
use sqlx::{AssertSqlSafe, Column as _, ColumnOrigin, Either, Executor as _, Row as _, SqlSafeStr as _, SqlStr};
use sqlx_json::RowExt as _;

use crate::SqliteHandler;
use crate::connection::{SqliteConnection, quote_ident};

/// Catalog name of every `SQLite` relation in rule keys.
const CATALOG: &str = "main";

/// Lists the relations named by the JSON array of names in `?1`, `temp` first.
///
/// `pragma_table_list(name)` searches every schema and matches names
/// case-insensitively, as `SQLite` resolves them. Listing every relation
/// instead costs about 13 ms per call at 2,000 tables; this lookup stays
/// near 0.3 ms whatever the schema size.
const LOOKUP_SQL: &str = "SELECT t.schema, t.name, t.type \
     FROM json_each(?1) AS j, pragma_table_list(j.value) AS t \
     JOIN pragma_database_list AS d ON d.name = t.schema \
     ORDER BY t.schema <> 'temp', d.seq";

/// Lists the attached schemas, in `pragma_database_list` order.
const SCHEMAS_SQL: &str = "SELECT name FROM pragma_database_list ORDER BY seq";

/// Lists the columns of each `[schema, name]` pair of the JSON array in `?1`.
const COLUMNS_SQL: &str = "SELECT j.key, x.name, x.hidden \
     FROM json_each(?1) AS j, \
     pragma_table_xinfo(json_extract(j.value, '$[1]'), json_extract(j.value, '$[0]')) AS x \
     ORDER BY j.key, x.cid";

/// Extended result codes whose messages name schema objects, never values.
///
/// `CHECK`, `FOREIGNKEY`, `NOTNULL`, `PRIMARYKEY`, `UNIQUE`, `ROWID`, and
/// `DATATYPE` constraint failures. `TRIGGER` is left out: its message is
/// whatever the trigger raised.
const VALUE_FREE_CODES: &[i32] = &[275, 787, 1299, 1555, 2067, 2579, 3091];

/// `SQLITE_ERROR` messages of the JSON functions that carry no value.
const VALUE_FREE_JSON: &[&str] = &["malformed JSON", "JSON cannot hold BLOB values"];

/// Whether the statement may write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// A read under `query_only`, rolled back afterwards.
    Read,
    /// A write, committed afterwards; refused in read-only mode.
    Write,
}

/// Rows and counts of a statement run under the column rules.
#[derive(Debug)]
pub(crate) struct Enforced {
    /// The window's rows, redacted, as JSON objects.
    pub(crate) rows: Vec<Value>,
    /// Rows changed by a write.
    pub(crate) rows_affected: u64,
    /// Rowid of the last inserted row.
    pub(crate) last_insert_id: Option<u64>,
}

/// Why a statement returned no rows under the column rules.
#[derive(Debug, thiserror::Error)]
enum Stop {
    /// The planner refused the statement; nothing ran.
    #[error(transparent)]
    Rejected(#[from] Rejection),
    /// The result does not fit the plan, or puts a ruled column where the
    /// plan does not rewrite it; nothing was committed.
    #[error(transparent)]
    Shape(#[from] ShapeError),
    /// The statement failed with an error that may echo a ruled value.
    #[error("query failed (SQLite code {0}): error detail withheld under column policy")]
    Withheld(String),
}

/// What one run produced besides the rows it fed to the window.
struct Run {
    /// The statement kind, deciding positional or global redaction.
    statement: StatementKind,
    /// The plan of the statement's output columns.
    plan: Plan,
    /// The result's column names, from its first row.
    names: Vec<String>,
    /// Rows changed by a write.
    rows_affected: u64,
    /// Rowid of the last inserted row.
    last_insert_id: Option<u64>,
}

/// What executing the statement produced besides its rows.
struct Executed {
    /// The result's column names, from its first row.
    names: Vec<String>,
    /// Per-statement results, for the counts.
    results: Vec<SqliteQueryResult>,
    /// Why the first row does not fit the plan; the stream stopped there.
    shape: Option<ShapeError>,
}

/// A table or view the catalog lists.
struct Listed {
    /// The schema: `main`, `temp`, or an attached alias.
    schema: String,
    /// The relation's actual name.
    name: String,
    /// What kind of relation it is; `None` for a type this module does not know.
    kind: Option<RelationKind>,
}

impl SqliteHandler {
    /// Plans `sql` under `policy`'s rules, runs it into `window`, and redacts the kept rows.
    ///
    /// `policy` holds at least one column rule; the handler's redactor
    /// applies the plan, and its absence refuses the statement before it
    /// runs. `sql` is the text that runs: a paged query keeps the output positions
    /// of the query it pages. The whole exchange runs on one connection
    /// within `timeout_secs`. The window holds each row as a JSON array of
    /// its values; the returned rows are the window's, redacted, as objects,
    /// and `window.rows` is left empty.
    ///
    /// # Errors
    ///
    /// Returns [`SqlError::ReadOnlyViolation`] for a write in read-only
    /// mode, the planner's rejection, a refusal when a result column comes
    /// from a ruled column the plan does not rewrite, a generic message for
    /// a withheld database error, the [`SqlError`] of anything else that
    /// failed, or the redaction error when the rows do not fit the plan.
    pub(crate) async fn enforce(
        &self,
        policy: &PiiConfig,
        sql: &str,
        access: Access,
        timeout_secs: u64,
        window: &mut Window,
    ) -> Result<Enforced, ErrorData> {
        let Some(redactor) = &self.redactor else {
            return Err(ErrorData::internal_error(
                "column policy: column rules need PII redaction enabled",
                None,
            ));
        };
        let read_only = self.connection.read_only();
        if access == Access::Write && read_only {
            return Err(SqlError::ReadOnlyViolation.into());
        }
        let run = self
            .connection
            .with_connection(sql, None, timeout_secs, async |conn| {
                run(conn, read_only, policy, sql, access, &mut |row| {
                    window.push(row);
                })
                .await
            })
            .await?
            .map_err(|stop| ErrorData::internal_error(stop.to_string(), None))?;
        let rows = mem::take(&mut window.rows);
        let rows = if positional(&run.statement) {
            let mut cells = Cells::default();
            for row in rows {
                cells.push(entries(&run.names, row));
            }
            redactor.redact_cells(cells, &run.plan).await?
        } else {
            // Plan text and metadata are not the planned columns: detect globally.
            let rows = rows
                .into_iter()
                .map(|row| Value::Object(entries(&run.names, row).into_iter().collect()))
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

/// Reports whether the result columns are the planned output columns.
fn positional(statement: &StatementKind) -> bool {
    !matches!(
        statement,
        StatementKind::Explain(_) | StatementKind::Show(_) | StatementKind::Describe
    )
}

/// Pairs a window row's values with the result's column names.
fn entries(names: &[String], row: Value) -> Vec<(String, Value)> {
    let values = match row {
        Value::Array(values) => values,
        other => vec![other],
    };
    names.iter().cloned().zip(values).collect()
}

/// Plans and executes one statement inside a transaction on `conn`.
///
/// Rows reach `sink` as JSON arrays of their values, in result order.
async fn run(
    conn: &mut sqlx::SqliteConnection,
    read_only: bool,
    config: &PiiConfig,
    sql: &str,
    access: Access,
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Result<Run, Stop>, sqlx::Error> {
    let begin = match access {
        Access::Read => {
            // A file opened read-only needs no pragma; `before_acquire` switches it off again.
            if !read_only {
                sqlx::query("PRAGMA query_only = ON").execute(&mut *conn).await?;
            }
            "BEGIN"
        }
        Access::Write => "BEGIN IMMEDIATE",
    };
    let mut tx = sqlx::Connection::begin_with(conn, SqlStr::from_static(begin)).await?;
    let lineage = match resolve(&mut tx, sql).await? {
        Ok(lineage) => lineage,
        Err(rejection) => return Ok(Err(rejection.into())),
    };
    // `CREATE TABLE x AS` creates `main.x` even when a bare `x` resolved to a
    // `temp` or attached table, so its rules would check the wrong target.
    if lineage.statement == StatementKind::CreateTableAs
        && lineage.target.as_ref().is_some_and(|target| target.schema != "main")
    {
        return Ok(Err(Rejection::Lineage(LineageClass::Unsupported).logged().into()));
    }
    let plan = match dbmcp_pii::policy::plan(&lineage, Dialect::Sqlite, config) {
        Ok(plan) => plan,
        Err(rejection) => return Ok(Err(rejection.into())),
    };
    let expected = positional(&lineage.statement).then_some(&plan);
    let executed = match execute(&mut tx, sql, expected, &config.rules, sink).await {
        Ok(executed) => executed,
        Err(error) => {
            return match withheld(&error) {
                Some(code) if touches_ruled(&lineage, config) => Ok(Err(Stop::Withheld(code))),
                _ => Err(error),
            };
        }
    };
    if let Some(shape) = executed.shape {
        tx.rollback().await?;
        return Ok(Err(shape.into()));
    }
    match access {
        Access::Read => tx.rollback().await?,
        Access::Write => tx.commit().await?,
    }
    let (rows_affected, last_insert_id) = SqliteConnection::counts(sql, &executed.results);
    Ok(Ok(Run {
        statement: lineage.statement,
        plan,
        names: executed.names,
        rows_affected,
        last_insert_id,
    }))
}

/// Runs `sql`, feeding each row's values to `sink` and collecting its results.
///
/// With `expected`, the first row's column origins, count, and names are
/// checked against it first, so a write that does not fit is never
/// committed; on a mismatch nothing reaches `sink` and the stream stops.
async fn execute(
    conn: &mut sqlx::SqliteConnection,
    sql: &str,
    expected: Option<&Plan>,
    rules: &[ColumnRule],
    sink: &mut (impl FnMut(Value) + Send),
) -> Result<Executed, sqlx::Error> {
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
                    && let Some(plan) = expected
                {
                    let fits = match unplanned_ruled(row.columns(), plan, rules) {
                        Some(position) => Err(ShapeError::NameMismatch { position }),
                        None => check_columns(plan, &row_names),
                    };
                    if let Err(shape) = fits {
                        return Ok(Executed {
                            names: Vec::new(),
                            results,
                            shape: Some(shape),
                        });
                    }
                }
                names.get_or_insert(row_names);
                sink(Value::Array(values));
            }
        }
    }
    Ok(Executed {
        names: names.unwrap_or_default(),
        results,
        shape: None,
    })
}

/// Returns the first result position whose origin is ruled but not planned as ruled.
///
/// `SQLite` reports a column's origin through views and subqueries down to
/// its base table, as `table` in `main` and `schema.table` elsewhere. A
/// table name holding a `.` reads both ways; either reading counts.
fn unplanned_ruled(columns: &[SqliteColumn], plan: &Plan, rules: &[ColumnRule]) -> Option<usize> {
    columns.iter().position(|column| {
        let ColumnOrigin::Table(origin) = column.origin() else {
            return false;
        };
        let name = ColumnRule::fold(&origin.name);
        let is_ruled = readings(&origin.table).any(|(schema, table)| {
            let key = [
                CATALOG.to_owned(),
                ColumnRule::fold(schema),
                ColumnRule::fold(table),
                name.clone(),
            ];
            rules.iter().any(|rule| rule.key == key)
        });
        is_ruled
            && !matches!(
                plan.columns.get(column.ordinal()).map(|planned| planned.position),
                Some(Position::Action(..))
            )
    })
}

/// Splits an origin table into each `(schema, table)` reading it allows.
fn readings(table: &str) -> impl Iterator<Item = (&str, &str)> {
    std::iter::once(("main", table)).chain(
        table
            .match_indices('.')
            .map(move |(dot, _)| (&table[..dot], &table[dot + 1..])),
    )
}

/// Analyzes `sql`, fetching missing relations for at most [`CATALOG_ROUNDS`] rounds.
async fn resolve(conn: &mut sqlx::SqliteConnection, sql: &str) -> Result<Result<Lineage, Rejection>, sqlx::Error> {
    let schemas: Vec<String> = sqlx::query_scalar(SCHEMAS_SQL).fetch_all(&mut *conn).await?;
    let mut catalog = Catalog::new();
    let mut rounds = 0;
    loop {
        match analyze(sql, Dialect::Sqlite, &catalog) {
            Ok(lineage) => return Ok(Ok(lineage)),
            Err(LineageError::Missing(_)) if rounds == CATALOG_ROUNDS => {
                return Ok(Err(Rejection::CatalogRounds.logged()));
            }
            Err(LineageError::Missing(names)) => fetch(conn, &schemas, names, &mut catalog).await?,
            Err(error) => return Ok(Err(reject_lineage(&error))),
        }
        rounds += 1;
    }
}

/// Lists the relations named `tables` in any schema, in name-resolution order.
///
/// A relation of a type this module does not know lists with no kind.
async fn lookup(conn: &mut sqlx::SqliteConnection, tables: &[&str]) -> Result<Vec<Listed>, sqlx::Error> {
    let tables = Value::from(tables.to_vec()).to_string();
    let rows: Vec<(String, String, String)> = sqlx::query_as(LOOKUP_SQL).bind(tables).fetch_all(conn).await?;
    Ok(rows
        .into_iter()
        .map(|(schema, name, kind)| Listed {
            schema,
            name,
            kind: relation_kind(&kind),
        })
        .collect())
}

/// Maps a `pragma_table_list` type to a relation kind.
///
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

/// Finds the relation a written name resolves to, as `SQLite` would.
///
/// A bare name searches `temp`, `main`, then attached schemas; inside a
/// view stored in a schema other than `temp`, only that schema.
fn locate<'a>(listed: &'a [Listed], name: &WrittenName) -> Option<&'a Listed> {
    let (schema, table) = match name.parts.as_slice() {
        [table] => match &name.context {
            Origin::View(view) if !view.schema.eq_ignore_ascii_case("temp") => (Some(view.schema.as_str()), table),
            _ => (None, table),
        },
        [schema, table] => (Some(schema.value.as_str()), table),
        _ => return None,
    };
    listed.iter().find(|relation| {
        relation.name.eq_ignore_ascii_case(&table.value)
            && schema.is_none_or(|schema| relation.schema.eq_ignore_ascii_case(schema))
    })
}

/// Returns the entry for a name nothing is listed under.
///
/// A relation created under a bare name lands in `main`; under a qualified
/// name, in that schema when it exists. Names in view bodies are absent.
fn vacant(schemas: &[String], name: &WrittenName) -> Entry {
    let schema = match (name.parts.as_slice(), &name.context) {
        ([table], Origin::Query) => Some(("main", table)),
        ([schema, table], Origin::Query) => schemas
            .iter()
            .find(|known| known.eq_ignore_ascii_case(&schema.value))
            .map(|known| (known.as_str(), table)),
        _ => None,
    };
    schema.map_or(Entry::Absent, |(schema, table)| {
        Entry::Vacant(RelationId::new(CATALOG, schema, &table.value))
    })
}

/// Inserts a catalog entry for each of `names`, read in batched queries.
async fn fetch(
    conn: &mut sqlx::SqliteConnection,
    schemas: &[String],
    names: Vec<WrittenName>,
    catalog: &mut Catalog,
) -> Result<(), sqlx::Error> {
    let tables: Vec<&str> = names
        .iter()
        .filter_map(|name| name.parts.last().map(|part| part.value.as_str()))
        .collect();
    let listed = lookup(conn, &tables).await?;
    let mut found: Vec<(WrittenName, &Listed, RelationKind)> = Vec::new();
    for name in names {
        let Some(relation) = locate(&listed, &name) else {
            let entry = vacant(schemas, &name);
            catalog.insert(name, entry);
            continue;
        };
        match relation.kind {
            Some(kind) => found.push((name, relation, kind)),
            // Exists, but cannot be described: never vacant, so `IF EXISTS` cannot skip it.
            None => catalog.insert(name, Entry::Absent),
        }
    }
    if found.is_empty() {
        return Ok(());
    }
    let pairs: Vec<Value> = found
        .iter()
        .map(|(_, relation, _)| Value::from(vec![relation.schema.as_str(), relation.name.as_str()]))
        .collect();
    let pairs = Value::from(pairs).to_string();
    let rows: Vec<(i64, String, i64)> = sqlx::query_as(COLUMNS_SQL).bind(pairs).fetch_all(&mut *conn).await?;
    for (index, (name, listed, kind)) in found.into_iter().enumerate() {
        let columns = rows
            .iter()
            .filter(|(key, _, _)| usize::try_from(*key).is_ok_and(|key| key == index))
            .map(|(_, column, hidden)| {
                // `hidden` = 1 marks virtual-table hidden columns; 2 and 3 are generated columns.
                if *hidden == 1 {
                    CatalogColumn::hidden(column)
                } else {
                    CatalogColumn::new(column)
                }
            })
            .collect();
        let id = RelationId::new(CATALOG, &listed.schema, &listed.name);
        let mut relation = Relation::new(id, kind, columns);
        if kind == RelationKind::View {
            let Some(body) = view_body(conn, listed).await? else {
                catalog.insert(name, Entry::Absent);
                continue;
            };
            relation = relation.with_view_sql(body);
        }
        catalog.insert(name, Entry::Resolved(relation));
    }
    Ok(())
}

/// Reads the query of a view's `CREATE VIEW` statement; `None` when unreadable.
async fn view_body(conn: &mut sqlx::SqliteConnection, view: &Listed) -> Result<Option<String>, sqlx::Error> {
    let sql = format!(
        "SELECT sql FROM {}.sqlite_schema WHERE type = 'view' AND name = ?1",
        quote_ident(&view.schema)
    );
    let create: Option<Option<String>> = sqlx::query_scalar(AssertSqlSafe(sql))
        .bind(&view.name)
        .fetch_optional(conn)
        .await?;
    Ok(create.flatten().as_deref().and_then(view_query))
}

/// Extracts the query from a `CREATE VIEW` statement.
fn view_query(create: &str) -> Option<String> {
    match Parser::parse_sql(&SQLiteDialect {}, create).ok()?.as_slice() {
        [Statement::CreateView(view)] => Some(view.query.to_string()),
        _ => None,
    }
}

/// Returns the extended code of a database error that may echo a value.
///
/// Timeouts and non-database errors are never withheld.
fn withheld(error: &sqlx::Error) -> Option<String> {
    if <SqliteConnection as dbmcp_sql::Connection>::is_timeout(error) {
        return None;
    }
    let error = error.as_database_error()?;
    let code = error.code()?;
    let value_free = code.parse::<i32>().is_ok_and(|code| {
        VALUE_FREE_CODES.contains(&code) || (code == 1 && VALUE_FREE_JSON.contains(&error.message()))
    });
    (!value_free).then(|| code.into_owned())
}

#[cfg(test)]
mod tests {
    use dbmcp_config::{Config, DatabaseBackend, DatabaseConfig, PiiOperator, Transport};
    use dbmcp_pii::Entity;
    use dbmcp_pii::policy::PlannedColumn;
    use dbmcp_sql_lineage::NamePart;
    use sqlx::Connection as _;

    use super::*;
    use crate::types::{ExplainQueryRequest, QueryRequest, ReadQueryRequest};

    /// HMAC key for the `hash` operator, 32 bytes.
    const KEY: &str = "0123456789abcdef0123456789abcdef";

    /// Tables the handler tests read: `users.email` and `users.ssn` are ruled.
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

    fn located(name: &WrittenName) -> Option<String> {
        locate(&listed(), name).map(|relation| format!("{}.{}", relation.schema, relation.name))
    }

    fn rule(key: [&str; 4], entity: &'static str) -> ColumnRule {
        ColumnRule {
            name: "rule".into(),
            item: 0,
            key: key.map(String::from),
            entity,
            operator: PiiOperator::Hash,
        }
    }

    fn rules_config() -> PiiConfig {
        PiiConfig {
            key: Some(KEY.into()),
            rules: vec![
                rule(["main", "main", "users", "email"], "EMAIL_ADDRESS"),
                rule(["main", "main", "users", "ssn"], "US_SSN"),
            ],
            ..PiiConfig::default()
        }
    }

    /// Builds a handler with column rules on a fresh in-memory database holding [`SETUP`].
    async fn handler(read_only: bool) -> SqliteHandler {
        let handler = SqliteHandler::new(&Config {
            transport: Transport::Stdio,
            database: DatabaseConfig {
                backend: DatabaseBackend::Sqlite,
                name: Some(":memory:".into()),
                read_only,
                ..DatabaseConfig::default()
            },
            http: None,
            pii: Some(rules_config()),
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
            .read_query(ReadQueryRequest {
                query: query.into(),
                cursor: None,
                timeout: None,
            })
            .await
            .map(|response| response.rows)
    }

    async fn write(handler: &SqliteHandler, query: &str) -> Result<Vec<Value>, ErrorData> {
        handler
            .write_query(QueryRequest { query: query.into() })
            .await
            .map(|response| response.rows)
    }

    async fn count(handler: &SqliteHandler, table: &str) -> i64 {
        let pool = handler.connection.pool(None).await.expect("pool");
        sqlx::query_scalar(AssertSqlSafe(format!("SELECT COUNT(*) FROM {table}")))
            .fetch_one(&pool)
            .await
            .expect("count")
    }

    /// Opens a raw in-memory connection holding `setup`.
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
        let name = |table: &str| WrittenName::query(vec![NamePart::bare(table)]);
        assert_eq!(located(&name("T")), Some("temp.t".into()));
        assert_eq!(located(&name("users")), Some("main.users".into()));
        assert_eq!(located(&name("only_aux")), Some("aux.only_aux".into()));
        assert_eq!(located(&name("missing")), None);
    }

    #[test]
    fn qualified_names_pick_their_schema() {
        let name =
            |schema: &str, table: &str| WrittenName::query(vec![NamePart::bare(schema), NamePart::quoted(table)]);
        assert_eq!(located(&name("AUX", "users")), Some("aux.users".into()));
        assert_eq!(located(&name("main", "t")), Some("main.t".into()));
        assert_eq!(located(&name("other", "t")), None);
        let three = WrittenName::query(vec![NamePart::bare("a"), NamePart::bare("b"), NamePart::bare("c")]);
        assert_eq!(located(&three), None);
    }

    #[test]
    fn view_body_names_resolve_in_the_view_schema() {
        let in_view = |schema: &str| WrittenName {
            parts: vec![NamePart::bare("t")],
            context: Origin::View(RelationId::new(CATALOG, schema, "v")),
        };
        assert_eq!(located(&in_view("main")), Some("main.t".into()));
        assert_eq!(located(&in_view("temp")), Some("temp.t".into()));
    }

    #[test]
    fn unlisted_names_are_vacant_where_sqlite_would_create_them() {
        let schemas: Vec<String> = ["main", "temp", "aux"].map(String::from).to_vec();
        let id = |name: &WrittenName| match vacant(&schemas, name) {
            Entry::Vacant(id) => Some(format!("{}.{}.{}", id.catalog, id.schema, id.name)),
            _ => None,
        };
        assert_eq!(
            id(&WrittenName::query(vec![NamePart::bare("New")])).as_deref(),
            Some("main.main.New")
        );
        assert_eq!(
            id(&WrittenName::query(vec![NamePart::bare("AUX"), NamePart::bare("n")])).as_deref(),
            Some("main.aux.n")
        );
        assert_eq!(
            id(&WrittenName::query(vec![NamePart::bare("nope"), NamePart::bare("n")])),
            None
        );
        let in_view = WrittenName {
            parts: vec![NamePart::bare("n")],
            context: Origin::View(RelationId::new(CATALOG, "main", "v")),
        };
        assert_eq!(id(&in_view), None);
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

    /// Runs `sql` on a fresh in-memory database and returns its error.
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
        assert_eq!(withheld(&error).as_deref(), Some("1"));
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
            assert_eq!(withheld(&error), None, "{sql}: {error}");
        }
    }

    #[tokio::test]
    async fn trigger_raised_errors_are_withheld() {
        let setup = "CREATE TABLE t (a); \
             CREATE TRIGGER g BEFORE INSERT ON t BEGIN SELECT RAISE(ABORT, 'no'); END";
        let error = error_of(setup, "INSERT INTO t VALUES (1)").await;
        assert_eq!(withheld(&error).as_deref(), Some("1811"));
    }

    #[tokio::test]
    async fn fetch_describes_hidden_generated_and_view_columns() {
        let mut conn = memory(
            "CREATE TABLE g (a INTEGER, b INTEGER GENERATED ALWAYS AS (a + 1));
             CREATE VIRTUAL TABLE f USING fts5(body);
             CREATE VIEW v(x) AS SELECT a FROM g",
        )
        .await;
        let schemas = vec!["main".to_owned(), "temp".to_owned()];
        let names = ["g", "f", "v"].map(|table| WrittenName::query(vec![NamePart::bare(table)]));
        let mut catalog = Catalog::new();
        fetch(&mut conn, &schemas, names.to_vec(), &mut catalog)
            .await
            .expect("fetch");
        let resolved = |name: &WrittenName| match catalog.get(name) {
            Some(Entry::Resolved(relation)) => relation.clone(),
            other => panic!("{name:?}: {other:?}"),
        };
        let generated = resolved(&names[0]);
        assert!(
            generated.columns.iter().all(|column| column.star_visible),
            "{generated:?}"
        );
        assert_eq!(generated.columns.len(), 2);
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
    async fn temp_tables_shadow_main_for_bare_names() {
        let mut conn = memory("CREATE TABLE t (a); CREATE TEMP TABLE t (b)").await;
        let listed = lookup(&mut conn, &["T"]).await.expect("lookup");
        let name = WrittenName::query(vec![NamePart::bare("t")]);
        let found = locate(&listed, &name).expect("listed");
        assert_eq!((found.schema.as_str(), found.name.as_str()), ("temp", "t"));
    }

    #[tokio::test]
    async fn origin_check_refuses_ruled_columns_at_global_positions() {
        let mut conn = memory(SETUP).await;
        let row = sqlx::query("SELECT name, email FROM users")
            .fetch_one(&mut conn)
            .await
            .expect("row");
        let planned = |position| PlannedColumn { position, name: None };
        let action = Position::Action(PiiOperator::Hash, Entity::EmailAddress);
        let rules = rules_config().rules;
        let global = Plan {
            columns: vec![planned(Position::Global), planned(Position::Global)],
        };
        assert_eq!(unplanned_ruled(row.columns(), &global, &rules), Some(1));
        let rewritten = Plan {
            columns: vec![planned(Position::Global), planned(action)],
        };
        assert_eq!(unplanned_ruled(row.columns(), &rewritten, &rules), None);
        let short = Plan {
            columns: vec![planned(Position::Global)],
        };
        assert_eq!(unplanned_ruled(row.columns(), &short, &rules), Some(1));
    }

    #[tokio::test]
    async fn results_that_do_not_fit_the_plan_stop_before_any_row() {
        let mut conn = memory(SETUP).await;
        let mut tx = sqlx::Connection::begin(&mut conn).await.expect("begin");
        let one = Plan {
            columns: vec![PlannedColumn {
                position: Position::Global,
                name: None,
            }],
        };
        let mut sunk = 0;
        let executed = execute(
            &mut tx,
            "UPDATE audit SET note = 'x' WHERE id = 1 RETURNING id, note",
            Some(&one),
            &[],
            &mut |_| sunk += 1,
        )
        .await
        .expect("execute");
        assert_eq!(executed.shape, Some(ShapeError::ColumnCount { expected: 1, actual: 2 }));
        assert_eq!(sunk, 0);
    }

    #[tokio::test]
    async fn rules_without_a_redactor_refuse_every_statement() {
        let mut handler = handler(false).await;
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
        .err()
        .expect("query_only rejects the write");
        assert!(error.to_string().contains("readonly"), "{error}");
    }

    #[tokio::test]
    async fn ruled_columns_are_hashed_by_position() {
        let handler = handler(false).await;
        let rows = read(&handler, "SELECT id, email AS email, name FROM users ORDER BY id")
            .await
            .expect("read");
        assert_eq!(rows.len(), 3);
        let email = rows[0]["email"].as_str().expect("email string");
        assert!(email.starts_with("<EMAIL_ADDRESS_"), "{email}");
        assert_eq!(rows[0]["name"], "Alice");
        let star = read(&handler, "SELECT * FROM users WHERE id = 2").await.expect("star");
        assert!(
            star[0]["email"]
                .as_str()
                .is_some_and(|v| v.starts_with("<EMAIL_ADDRESS_"))
        );
        assert_eq!(star[0]["ssn"], Value::Null);
    }

    #[tokio::test]
    async fn ruled_columns_outside_the_output_are_refused() {
        let handler = handler(false).await;
        let error = read(&handler, "SELECT id FROM users WHERE email LIKE 'a%'")
            .await
            .expect_err("filter on a ruled column");
        assert!(error.message.contains("column policy"), "{}", error.message);
    }

    #[tokio::test]
    async fn rejected_writes_change_nothing() {
        let handler = handler(false).await;
        let error = write(&handler, "INSERT INTO audit (note) SELECT email FROM users")
            .await
            .expect_err("laundering");
        assert!(error.message.contains("column policy"), "{}", error.message);
        assert_eq!(count(&handler, "audit").await, 2);
    }

    #[tokio::test]
    async fn returning_rows_are_redacted_and_the_write_commits() {
        let handler = handler(false).await;
        let rows = write(
            &handler,
            "UPDATE users SET name = 'Al' WHERE id = 1 RETURNING id, email",
        )
        .await
        .expect("update");
        assert!(
            rows[0]["email"]
                .as_str()
                .is_some_and(|v| v.starts_with("<EMAIL_ADDRESS_"))
        );
        let names = read(&handler, "SELECT name FROM users WHERE id = 1")
            .await
            .expect("read");
        assert_eq!(names[0]["name"], "Al");
    }

    #[tokio::test]
    async fn ctas_into_a_temp_shadowed_name_is_unsupported() {
        let handler = handler(false).await;
        let pool = handler.connection.pool(None).await.expect("pool");
        sqlx::raw_sql("CREATE TEMP TABLE copy (id INTEGER)")
            .execute(&pool)
            .await
            .expect("temp table");
        write(&handler, "CREATE TABLE fresh AS SELECT id FROM users")
            .await
            .expect("unshadowed CTAS plans");
        let error = write(&handler, "CREATE TABLE copy AS SELECT id FROM users")
            .await
            .expect_err("guard");
        assert!(error.message.contains("unsupported"), "{}", error.message);
    }

    #[tokio::test]
    async fn dropping_a_missing_table_if_exists_plans() {
        let handler = handler(false).await;
        write(&handler, "DROP TABLE IF EXISTS nope").await.expect("no target");
    }

    #[tokio::test]
    async fn explain_reads_and_refuses_ruled_relations() {
        let handler = handler(false).await;
        let explain = |query: &str| handler.explain_query(ExplainQueryRequest { query: query.into() });
        explain("SELECT note FROM audit").await.expect("unruled explain");
        let error = explain("SELECT email FROM users").await.expect_err("ruled explain");
        assert!(error.message.contains("EXPLAIN"), "{}", error.message);
    }

    #[tokio::test]
    async fn timeouts_under_rules_still_report_query_timeout() {
        let handler = handler(false).await;
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
            .read_query(ReadQueryRequest {
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
        let handler = handler(true).await;
        let error = write(&handler, "CREATE TABLE t (a)").await.expect_err("read-only");
        assert_eq!(error.message, SqlError::ReadOnlyViolation.to_string());
    }

    #[tokio::test]
    async fn policy_exists_only_with_rules() {
        assert!(handler(false).await.policy.is_some());
        let plain = SqliteHandler::new(&Config {
            transport: Transport::Stdio,
            database: DatabaseConfig {
                backend: DatabaseBackend::Sqlite,
                name: Some(":memory:".into()),
                ..DatabaseConfig::default()
            },
            http: None,
            pii: Some(PiiConfig::default()),
            ssh: None,
        })
        .expect("handler builds");
        assert!(plain.policy.is_none());
    }
}
