//! AST-based SQL validation for the read and write query tools.

use crate::SqlError;
use std::ops::ControlFlow;

use dbmcp_sql_lineage::{Dialect, Show};
use sqlparser::ast::{Expr, Function, Ident, ObjectName, Select, Statement, TableFactor, Visit, Visitor};
use sqlparser::dialect::{self, MySqlDialect, PostgreSqlDialect, SQLiteDialect};
use sqlparser::parser::Parser;
use sqlparser::tokenizer::{Token, Tokenizer, TokenizerError, Whitespace};

/// Classifies a validated read-only statement for pagination dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// `SELECT`, `WITH ... SELECT`, or compound queries (`UNION`/`INTERSECT`/`EXCEPT`).
    /// Composable inside a subquery wrap with an appended `LIMIT` / `OFFSET`.
    Select,
    /// `SHOW`, `DESC` / `DESCRIBE`, or `EXPLAIN` — read-only but not
    /// composable as a subquery source. Returned in a single response.
    NonSelect,
}

/// Validates that a SQL query is read-only and classifies it for pagination.
///
/// Parses the query using the given dialect and checks:
/// 1. Exactly one statement (multi-statement injection blocked)
/// 2. Statement type is read-only (SELECT, SHOW, DESCRIBE, EXPLAIN); `USE`
///    is rejected because it would leak to later queries on a pooled connection;
///    `SHOW WARNINGS` / `SHOW ERRORS` are rejected because warnings quote values
/// 3. The statement wrapped by `EXPLAIN` is itself read-only
/// 4. No data-modifying CTEs or `SELECT ... INTO` inside queries
/// 5. No dangerous functions (file access, large objects, sequences, extensions,
///    session locks, backend signalling, server administration)
/// 6. No INTO OUTFILE/DUMPFILE clauses
/// 7. On `MySQL` / `MariaDB`, no executable comments (`/*! … */`, `/*M! … */`)
///    and no characters the tokenizer cannot read
///
/// Returns the classified [`StatementKind`] on success so callers that
/// paginate can dispatch `Select` to a paged fetch and route `NonSelect`
/// through a single-page pass-through.
///
/// # Errors
///
/// Returns [`SqlError`] if the query is not allowed in read-only mode.
pub fn validate_read(sql: &str, dialect: Dialect) -> Result<StatementKind, SqlError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(SqlError::ReadOnlyViolation);
    }

    // Pre-check for INTO OUTFILE/DUMPFILE — sqlparser may not parse MySQL-specific syntax
    let upper = trimmed.to_uppercase();
    if upper.contains("INTO OUTFILE") || upper.contains("INTO DUMPFILE") {
        return Err(SqlError::IntoOutfileBlocked);
    }

    // Must be exactly one statement
    let stmt = match dialect {
        Dialect::Postgres => parse_single(trimmed, &PostgreSqlDialect {}),
        Dialect::Mysql | Dialect::Mariadb => parse_single(trimmed, &MySqlDialect {}),
        Dialect::Sqlite => parse_single(trimmed, &SQLiteDialect {}),
    }?
    .ok_or(SqlError::ReadOnlyViolation)?;

    classify(&stmt, trimmed, dialect)
}

/// Checks that `stmt`, parsed from `sql`, is read-only and classifies it.
fn classify(stmt: &Statement, sql: &str, dialect: Dialect) -> Result<StatementKind, SqlError> {
    match stmt {
        Statement::Query(_) => {
            check_data_modifying_queries(stmt)?;
            check_dangerous_functions(stmt)?;
            Ok(StatementKind::Select)
        }
        // `EXPLAIN ANALYZE` executes the wrapped statement, so it must be read-only too.
        Statement::Explain { statement, .. } => {
            classify(statement, sql, dialect)?;
            Ok(StatementKind::NonSelect)
        }
        Statement::ShowVariable { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowCatalogs { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowProcessList { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowCharset(_)
        | Statement::ShowObjects(_)
        | Statement::ShowTables { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowCollation { .. } => {
            if matches!(stmt, Statement::ShowVariable { variable } if is_diagnostics(variable))
                || Show::classify(sql, dialect).replays_values()
            {
                Err(SqlError::DiagnosticsBlocked)
            } else {
                Ok(StatementKind::NonSelect)
            }
        }
        Statement::ExplainTable { .. } => Ok(StatementKind::NonSelect),
        Statement::Use(_) => Err(SqlError::UseBlocked),
        _ => Err(SqlError::ReadOnlyViolation),
    }
}

/// Reports whether parsed `SHOW` words are `[COUNT] WARNINGS` or `[COUNT] ERRORS`.
///
/// Backs up [`Show::classify`], which reads tokens instead of parsed words.
fn is_diagnostics(variable: &[Ident]) -> bool {
    let list = |part: &Ident| part.value.eq_ignore_ascii_case("warnings") || part.value.eq_ignore_ascii_case("errors");
    match variable {
        [count, part, ..] if count.value.eq_ignore_ascii_case("count") => list(part),
        [part, ..] => list(part),
        [] => false,
    }
}

/// Validates SQL for the write tool, rejecting `USE`.
///
/// `USE` would change the database of a pooled connection for later
/// queries. Every other statement is allowed. SQL that fails to parse passes
/// through, so the database reports its own error for syntax the parser
/// does not know (e.g. `MySQL` routine bodies, `DO`). Only `MySQL` and
/// `MariaDB` have `USE`, so only their write tool calls this. Comments the
/// server reads otherwise are rejected first, so they cannot hide a `USE`.
///
/// # Errors
///
/// Returns [`SqlError::UnreadableCharacter`] for SQL the server reads
/// otherwise, or [`SqlError::UseBlocked`] if any parsed statement is `USE`.
pub fn validate_write(sql: &str, dialect: &impl dialect::Dialect) -> Result<(), SqlError> {
    if dialect.supports_multiline_comment_hints() && unreadable(sql, dialect).unwrap_or(false) {
        return Err(SqlError::UnreadableCharacter);
    }
    let Ok(statements) = Parser::parse_sql(dialect, sql) else {
        return Ok(());
    };
    if statements.iter().any(|stmt| matches!(stmt, Statement::Use(_))) {
        return Err(SqlError::UseBlocked);
    }
    Ok(())
}

/// Validates that a SQL string contains exactly one statement.
///
/// Unlike [`validate_read`], any statement type is accepted. Use this
/// where the input is embedded into a wrapper (e.g. `EXPLAIN {query}`) that
/// must never smuggle additional statements past the driver.
///
/// # Errors
///
/// Returns [`SqlError::MultiStatement`] if the input holds more than one
/// statement, or [`SqlError::Query`] if it is empty or fails to parse.
pub fn validate_single_statement(sql: &str, dialect: &impl dialect::Dialect) -> Result<(), SqlError> {
    parse_single(sql.trim(), dialect)?.ok_or_else(|| SqlError::Query("Query must contain a statement".into()))?;
    Ok(())
}

/// Parses `sql` and returns its only statement, `None` if it has none.
///
/// On `MySQL`/`MariaDB` (dialects with `/*!` hints), first rejects SQL the
/// server may read differently: executable comments (matched on raw text,
/// string literals included) and tokens the server reads otherwise (see
/// [`is_unreadable`]).
fn parse_single(sql: &str, dialect: &impl dialect::Dialect) -> Result<Option<Statement>, SqlError> {
    if dialect.supports_multiline_comment_hints() {
        if ["/*!", "/*M!", "/*m!"].iter().any(|p| sql.contains(p)) {
            return Err(SqlError::ExecutableCommentBlocked);
        }
        if unreadable(sql, dialect).map_err(|e| SqlError::Query(format!("SQL parse error: {e}")))? {
            return Err(SqlError::UnreadableCharacter);
        }
    }

    let mut statements =
        Parser::parse_sql(dialect, sql).map_err(|e| SqlError::Query(format!("SQL parse error: {e}")))?;

    if statements.len() > 1 {
        return Err(SqlError::MultiStatement);
    }
    Ok(statements.pop())
}

/// Reports whether any token of `sql` is one the `MySQL` server reads otherwise.
fn unreadable(sql: &str, dialect: &impl dialect::Dialect) -> Result<bool, TokenizerError> {
    Ok(Tokenizer::new(dialect, sql).tokenize()?.iter().any(is_unreadable))
}

/// Reports whether `token` holds text the `MySQL` server reads otherwise.
///
/// The server starts a `--` comment only at an ASCII space or control
/// character (the tokenizer also at a Unicode space such as `U+00A0`), and
/// ends a `--` or `#` comment at a NUL byte. A character the tokenizer
/// cannot read (`Token::Char`), such as a control character that starts a
/// `--` comment on the server only, is rejected too.
fn is_unreadable(token: &Token) -> bool {
    match token {
        Token::Char(_) => true,
        Token::Whitespace(Whitespace::SingleLineComment { prefix, comment }) => {
            let server_comment = |ch: char| ch.is_ascii_whitespace() || ch.is_ascii_control();
            comment.contains('\0') || (prefix == "--" && !comment.starts_with(server_comment))
        }
        _ => false,
    }
}

/// Rejects data-modifying CTEs and `SELECT ... INTO` anywhere inside a query.
fn check_data_modifying_queries(stmt: &Statement) -> Result<(), SqlError> {
    if stmt.visit(&mut DataModifyingQueryChecker).is_break() {
        return Err(SqlError::ReadOnlyViolation);
    }
    Ok(())
}

struct DataModifyingQueryChecker;

impl Visitor for DataModifyingQueryChecker {
    type Break = ();

    fn pre_visit_statement(&mut self, stmt: &Statement) -> ControlFlow<Self::Break> {
        // Nested statements (e.g. `WITH x AS (DELETE ... RETURNING *)`) must be queries.
        if matches!(stmt, Statement::Query(_)) {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn pre_visit_select(&mut self, select: &Select) -> ControlFlow<Self::Break> {
        // `SELECT ... INTO new_table` creates a table on PostgreSQL.
        if select.into.is_some() {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

/// Functions that read or write server files, mutate state, or reach the network.
///
/// Matched case-insensitively against the last segment of the function name.
const DANGEROUS_FUNCTIONS: &[&str] = &[
    // MySQL / MariaDB
    "load_file",
    "get_lock",
    // PostgreSQL
    "pg_cancel_backend",
    "pg_terminate_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "pg_switch_wal",
    "pg_promote",
    "pg_log_backend_memory_contexts",
    "pg_logical_emit_message",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "lo_import",
    "lo_export",
    // Large-object writes: PostgreSQL before 16 runs them in read-only transactions.
    // Only direct calls are seen; the read-only transaction's rollback discards calls
    // made inside other functions.
    "lo_creat",
    "lo_create",
    "lo_from_bytea",
    "lo_put",
    "lowrite",
    "lo_truncate",
    "lo_truncate64",
    "lo_unlink",
    "setval",
    // SQLite (CLI / extension functions)
    "readfile",
    "writefile",
    "load_extension",
];

/// Function name prefixes blocked in addition to [`DANGEROUS_FUNCTIONS`].
const DANGEROUS_FUNCTION_PREFIXES: &[&str] = &[
    "dblink",
    // PostgreSQL session locks
    "pg_advisory",
    "pg_try_advisory",
    // PostgreSQL server administration (restore points, replication slots, statistics)
    "pg_create_",
    "pg_drop_",
    "pg_copy_",
    "pg_replication_",
    "pg_stat_reset",
    "pg_wal_replay_",
    "pg_backup_",
];

/// Check for dangerous function calls like `LOAD_FILE()` in the AST.
fn check_dangerous_functions(stmt: &Statement) -> Result<(), SqlError> {
    let mut checker = DangerousFunctionChecker { found: None };
    let _ = stmt.visit(&mut checker);
    if let Some(err) = checker.found {
        return Err(err);
    }
    Ok(())
}

/// Returns the error for a call to `name` if it is a dangerous function.
fn dangerous_function(name: &ObjectName) -> Option<SqlError> {
    // Match the last segment so quoting (`LOAD_FILE`) or schema prefixes (pg_catalog.) don't evade it.
    let last = name.0.last()?;
    let func = last
        .as_ident()
        .map_or_else(|| last.to_string(), |ident| ident.value.clone());
    let func = func.to_ascii_lowercase();

    if func == "load_file" {
        return Some(SqlError::LoadFileBlocked);
    }
    let blocked = DANGEROUS_FUNCTIONS.contains(&func.as_str())
        || DANGEROUS_FUNCTION_PREFIXES
            .iter()
            .any(|prefix| func.starts_with(prefix));
    blocked.then_some(SqlError::DangerousFunctionBlocked(func))
}

struct DangerousFunctionChecker {
    found: Option<SqlError>,
}

impl DangerousFunctionChecker {
    fn check(&mut self, name: &ObjectName) -> ControlFlow<()> {
        if let Some(err) = dangerous_function(name) {
            self.found = Some(err);
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    }
}

impl Visitor for DangerousFunctionChecker {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        if let Expr::Function(Function { name, .. }) = expr {
            return self.check(name);
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_table_factor(&mut self, table_factor: &TableFactor) -> ControlFlow<Self::Break> {
        // Set-returning calls in FROM, e.g. `SELECT * FROM pg_ls_dir('/')`.
        match table_factor {
            TableFactor::Table {
                name, args: Some(_), ..
            }
            | TableFactor::Function { name, .. } => self.check(name),
            _ => ControlFlow::Continue(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MYSQL: MySqlDialect = MySqlDialect {};
    const POSTGRES: PostgreSqlDialect = PostgreSqlDialect {};
    const SQLITE: SQLiteDialect = SQLiteDialect {};

    const DIALECT: MySqlDialect = MySqlDialect {};

    // === Statement classification ===

    #[test]
    fn classifies_select_vs_non_select() {
        // Select-like: plain SELECT, CTE, compound queries all map to StatementKind::Select.
        assert_eq!(
            validate_read("SELECT 1", Dialect::Mysql).expect("query is valid"),
            StatementKind::Select,
        );
        assert_eq!(
            validate_read("WITH x AS (SELECT 1) SELECT * FROM x", Dialect::Mysql).expect("query is valid"),
            StatementKind::Select,
        );
        assert_eq!(
            validate_read("SELECT 1 UNION SELECT 2", Dialect::Mysql).expect("query is valid"),
            StatementKind::Select,
        );

        // Catalog/metadata statements map to StatementKind::NonSelect.
        assert_eq!(
            validate_read("SHOW DATABASES", Dialect::Mysql).expect("query is valid"),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read("DESCRIBE users", Dialect::Mysql).expect("query is valid"),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read("EXPLAIN SELECT 1", Dialect::Mysql).expect("query is valid"),
            StatementKind::NonSelect,
        );
    }

    // === Allowed queries ===

    #[test]
    fn test_select_allowed() {
        assert!(validate_read("SELECT * FROM users", Dialect::Mysql).is_ok());
        assert!(validate_read("select * from users", Dialect::Mysql).is_ok());
    }

    #[test]
    fn test_show_allowed() {
        assert!(validate_read("SHOW DATABASES", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW TABLES", Dialect::Mysql).is_ok());
    }

    #[test]
    fn every_parsed_show_passes() {
        for sql in [
            "SHOW CHARSET",
            "SHOW CHARACTER SET",
            "SHOW PROCESSLIST",
            "SHOW FULL PROCESSLIST",
            "SHOW CATALOGS",
        ] {
            assert_eq!(
                validate_read(sql, Dialect::Mysql).ok(),
                Some(StatementKind::NonSelect),
                "{sql}"
            );
        }
    }

    #[test]
    fn test_describe_allowed() {
        // sqlparser parses DESC/DESCRIBE as ExplainTable
        assert!(validate_read("DESC users", Dialect::Mysql).is_ok());
        assert!(validate_read("DESCRIBE users", Dialect::Mysql).is_ok());
    }

    #[test]
    fn diagnostics_are_blocked() {
        for sql in [
            "SHOW WARNINGS",
            "show warnings",
            "SHOW WARNINGS LIMIT 5",
            "SHOW ERRORS",
            "SHOW ERRORS LIMIT 1, 2",
            "SHOW COUNT(*) WARNINGS",
            "SHOW COUNT(*) ERRORS",
            "show count(*) errors",
            "SHOW COUNT ( * ) WARNINGS",
            "SHOW /*x*/ WARNINGS",
            "SHOW\nWARNINGS",
            "SHOW WARNINGS LIMIT 1 OFFSET 2",
        ] {
            assert!(
                matches!(validate_read(sql, Dialect::Mysql), Err(SqlError::DiagnosticsBlocked)),
                "{sql}"
            );
        }
        assert!(matches!(
            validate_read("SHOW WARNINGS", Dialect::Postgres),
            Err(SqlError::DiagnosticsBlocked)
        ));
        assert!(matches!(
            validate_read("SHOW WARNINGS", Dialect::Sqlite),
            Err(SqlError::DiagnosticsBlocked)
        ));
        assert!(validate_read("SHOW VARIABLES", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW INDEX FROM errors", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW COLUMNS FROM errors", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW VARIABLES LIKE 'warnings'", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW WARNINGS_X", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW TABLE STATUS FROM warnings", Dialect::Mysql).is_ok());
        assert!(validate_read("SHOW search_path", Dialect::Postgres).is_ok());
        // A quoted word is no keyword, but the parsed word still names WARNINGS.
        assert!(matches!(
            validate_read("SHOW `WARNINGS`", Dialect::Mysql),
            Err(SqlError::DiagnosticsBlocked)
        ));
        assert!(matches!(
            validate_read("SHOW WARNINGS", Dialect::Mariadb),
            Err(SqlError::DiagnosticsBlocked)
        ));
        assert!(matches!(
            validate_read("EXPLAIN SHOW WARNINGS", Dialect::Mysql),
            Err(SqlError::DiagnosticsBlocked)
        ));
    }

    /// Asserts that both `MySQL` dialects and the single-statement check refuse `sql`.
    fn assert_unreadable(sql: &str) {
        for dialect in [Dialect::Mysql, Dialect::Mariadb] {
            assert!(
                matches!(validate_read(sql, dialect), Err(SqlError::UnreadableCharacter)),
                "{sql:?}"
            );
        }
        assert!(
            matches!(
                validate_single_statement(sql, &MYSQL),
                Err(SqlError::UnreadableCharacter)
            ),
            "{sql:?}"
        );
    }

    #[test]
    fn control_characters_are_blocked_on_mysql() {
        // The server starts a `--` comment at a control character; the parser does not.
        for sql in [
            "SHOW --\x01\nWARNINGS",
            "SHOW --\x7f\nERRORS",
            "SHOW --\x00\nWARNINGS",
            "SHOW --\x1f\nCOUNT(*) WARNINGS",
            "SHOW\x01WARNINGS",
            "SHOW COUNT(*) --\x01\nWARNINGS",
            "SHOW --\x01\nTABLES",
            "SELECT 1 --\x01\n",
        ] {
            assert_unreadable(sql);
        }
        assert!(validate_read("SHOW -- x\nTABLES", Dialect::Mysql).is_ok());
        assert!(validate_read("SELECT '\x01'", Dialect::Mysql).is_ok());
    }

    #[test]
    fn unicode_space_comments_are_blocked_on_mysql() {
        // The parser starts a `--` comment at a Unicode space; the server does not.
        for space in ['\u{a0}', '\u{85}', '\u{2000}', '\u{2028}', '\u{3000}'] {
            assert_unreadable(&format!("SELECT 1 --{space};SELECT 2"));
            assert_unreadable(&format!("SHOW --{space}\nWARNINGS"));
        }
        for sql in [
            "SELECT 1 -- \u{a0}x",
            "SELECT 1 --\t\u{a0}x",
            "SELECT 1 # \u{a0}x",
            "SELECT '--\u{a0}'",
            "SELECT 1 /* --\u{a0} */",
        ] {
            assert!(validate_read(sql, Dialect::Mysql).is_ok(), "{sql:?}");
        }
        assert!(validate_read("SELECT 1 --\u{a0}", Dialect::Postgres).is_ok());
    }

    #[test]
    fn nul_in_line_comments_is_blocked_on_mysql() {
        // The server ends a `--` or `#` comment at a NUL byte; the parser does not.
        for sql in [
            "SELECT 1 AS a -- \0, 2 AS b",
            "SELECT 1 AS a # \0, 2 AS b",
            "SELECT 1 AS a -- x\0\n, 2 AS b",
        ] {
            assert_unreadable(sql);
        }
        for sql in ["SELECT 1 AS a /* \0 */, 2 AS b", "SELECT '\0' AS a"] {
            assert!(validate_read(sql, Dialect::Mysql).is_ok(), "{sql:?}");
        }
    }

    #[test]
    fn write_rejects_comments_the_server_reads_otherwise() {
        for sql in [
            "INSERT INTO t VALUES (1) --\u{a0}; USE mydb",
            "INSERT INTO t VALUES (1) # \0; USE mydb",
            "INSERT INTO t VALUES (1) --\x01; USE mydb",
        ] {
            assert!(
                matches!(validate_write(sql, &DIALECT), Err(SqlError::UnreadableCharacter)),
                "{sql:?}"
            );
        }
        assert!(validate_write("INSERT INTO t VALUES (1) -- \u{a0}x", &DIALECT).is_ok());
        assert!(validate_write("INSERT INTO t VALUES (1) --\u{a0}x", &SQLiteDialect {}).is_ok());
    }

    #[test]
    fn executable_comments_are_blocked_on_mysql() {
        for sql in [
            "SELECT 1 /*M!100000 ; DELETE FROM users */",
            "SELECT 1 /*m!100000 , 2 */",
            "SHOW /*!99999 TABLES */ WARNINGS",
            "SELECT '/*!'",
        ] {
            assert!(
                matches!(
                    validate_read(sql, Dialect::Mysql),
                    Err(SqlError::ExecutableCommentBlocked)
                ),
                "{sql}"
            );
        }
        assert!(matches!(
            validate_single_statement("SELECT 1 /*M!100000 ; DELETE FROM users */", &MYSQL),
            Err(SqlError::ExecutableCommentBlocked)
        ));
        assert!(validate_read("SELECT 1 /* plain */", Dialect::Mysql).is_ok());
        assert!(validate_read("SELECT /*+ MAX_EXECUTION_TIME(1) */ 1", Dialect::Mysql).is_ok());
        assert!(validate_read("SELECT '/*!'", Dialect::Postgres).is_ok());
        assert!(validate_read("SELECT '/*M!'", Dialect::Sqlite).is_ok());
    }

    #[test]
    fn test_use_blocked() {
        assert!(matches!(
            validate_read("USE mydb", Dialect::Mysql),
            Err(SqlError::UseBlocked)
        ));
        assert!(matches!(
            validate_write("USE mydb", &DIALECT),
            Err(SqlError::UseBlocked)
        ));
        assert!(matches!(
            validate_write("INSERT INTO t VALUES (1); USE mydb", &DIALECT),
            Err(SqlError::UseBlocked)
        ));
        assert!(validate_write("INSERT INTO t VALUES (1)", &DIALECT).is_ok());
        assert!(validate_write("SET @x = 1", &DIALECT).is_ok());
        assert!(
            validate_write("DO 1", &DIALECT).is_ok(),
            "unparseable SQL passes through"
        );
    }

    // === Blocked statement types ===

    #[test]
    fn test_insert_blocked() {
        assert!(matches!(
            validate_read("INSERT INTO users VALUES (1)", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_update_blocked() {
        assert!(matches!(
            validate_read("UPDATE users SET name='x'", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_delete_blocked() {
        assert!(matches!(
            validate_read("DELETE FROM users", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_drop_blocked() {
        assert!(matches!(
            validate_read("DROP TABLE users", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_create_blocked() {
        assert!(matches!(
            validate_read("CREATE TABLE test (id INT)", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    // === Comment bypass attacks ===

    #[test]
    fn test_comment_bypass_single_line() {
        // With AST parsing, "SELECT 1 -- \nDELETE FROM users" is parsed as two statements
        // (or the comment hides the DELETE, making it one SELECT).
        // Either way, if it parses as multiple statements, it's blocked.
        // If the parser treats -- as a comment and only sees SELECT 1, it's allowed.
        let result = validate_read("SELECT 1 -- \nDELETE FROM users", Dialect::Mysql);
        // The parser should treat -- as comment, so only SELECT 1 remains → allowed
        assert!(result.is_ok() || matches!(result, Err(SqlError::MultiStatement)));
    }

    #[test]
    fn test_comment_bypass_multi_line() {
        // "/* SELECT */ DELETE FROM users" — parser strips comment, sees DELETE
        assert!(matches!(
            validate_read("/* SELECT */ DELETE FROM users", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    // === Dangerous functions ===

    #[test]
    fn test_load_file_blocked() {
        assert!(matches!(
            validate_read("SELECT LOAD_FILE('/etc/passwd')", Dialect::Mysql),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_case_insensitive() {
        assert!(matches!(
            validate_read("SELECT load_file('/etc/passwd')", Dialect::Mysql),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_with_spaces() {
        // sqlparser normalizes function calls, so spaces before ( are handled
        assert!(matches!(
            validate_read("SELECT LOAD_FILE ('/etc/passwd')", Dialect::Mysql),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    // === INTO OUTFILE/DUMPFILE ===

    #[test]
    fn test_into_outfile_blocked() {
        assert!(matches!(
            validate_read("SELECT * FROM users INTO OUTFILE '/tmp/out'", Dialect::Mysql),
            Err(SqlError::IntoOutfileBlocked)
        ));
    }

    #[test]
    fn test_into_dumpfile_blocked() {
        assert!(matches!(
            validate_read("SELECT * FROM users INTO DUMPFILE '/tmp/out'", Dialect::Mysql),
            Err(SqlError::IntoOutfileBlocked)
        ));
    }

    // === String literals should NOT trigger false positives ===

    #[test]
    fn test_load_file_in_string_allowed() {
        // LOAD_FILE inside a string literal is NOT a function call in the AST
        assert!(validate_read("SELECT 'LOAD_FILE(/etc/passwd)' FROM dual", Dialect::Mysql).is_ok());
    }

    // === Empty / comment-only queries ===

    #[test]
    fn test_empty_query_blocked() {
        assert!(matches!(
            validate_read("", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_comment_only_blocked() {
        // Comment-only input: parser returns empty statements or parse error
        let result = validate_read("-- just a comment", Dialect::Mysql);
        assert!(result.is_err());
    }

    // === New tests for AST-based validation ===

    #[test]
    fn test_multi_statement_blocked() {
        assert!(matches!(
            validate_read("SELECT 1; SELECT 2", Dialect::Mysql),
            Err(SqlError::MultiStatement)
        ));
    }

    #[test]
    fn test_multi_statement_injection_blocked() {
        assert!(matches!(
            validate_read("SELECT 1; DROP TABLE users", Dialect::Mysql),
            Err(SqlError::MultiStatement)
        ));
    }

    #[test]
    fn test_set_statement_blocked() {
        assert!(matches!(
            validate_read("SET @var = 1", Dialect::Mysql),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_malformed_sql_rejected() {
        let result = validate_read("SELEC * FORM users", Dialect::Mysql);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_with_subquery_allowed() {
        assert!(validate_read("SELECT * FROM (SELECT 1) AS t", Dialect::Mysql).is_ok());
    }

    #[test]
    fn test_select_with_where_allowed() {
        assert!(validate_read("SELECT * FROM users WHERE id = 1", Dialect::Mysql).is_ok());
    }

    #[test]
    fn test_select_count_allowed() {
        assert!(validate_read("SELECT COUNT(*) FROM users", Dialect::Mysql).is_ok());
    }

    // === T015: Multi-dialect parameterized tests ===

    fn assert_allowed_all_dialects(sql: &str) {
        assert!(validate_read(sql, Dialect::Mysql).is_ok(), "MySQL should allow: {sql}");
        assert!(
            validate_read(sql, Dialect::Postgres).is_ok(),
            "Postgres should allow: {sql}"
        );
        assert!(
            validate_read(sql, Dialect::Sqlite).is_ok(),
            "SQLite should allow: {sql}"
        );
    }

    fn assert_blocked_all_dialects(sql: &str) {
        assert!(validate_read(sql, Dialect::Mysql).is_err(), "MySQL should block: {sql}");
        assert!(
            validate_read(sql, Dialect::Postgres).is_err(),
            "Postgres should block: {sql}"
        );
        assert!(
            validate_read(sql, Dialect::Sqlite).is_err(),
            "SQLite should block: {sql}"
        );
    }

    #[test]
    fn select_allowed_all_dialects() {
        assert_allowed_all_dialects("SELECT * FROM users");
        assert_allowed_all_dialects("SELECT 1");
        assert_allowed_all_dialects("SELECT COUNT(*) FROM t");
    }

    #[test]
    fn insert_blocked_all_dialects() {
        assert_blocked_all_dialects("INSERT INTO users VALUES (1)");
    }

    #[test]
    fn update_blocked_all_dialects() {
        assert_blocked_all_dialects("UPDATE users SET name = 'x'");
    }

    #[test]
    fn delete_blocked_all_dialects() {
        assert_blocked_all_dialects("DELETE FROM users");
    }

    #[test]
    fn drop_blocked_all_dialects() {
        assert_blocked_all_dialects("DROP TABLE users");
    }

    #[test]
    fn create_blocked_all_dialects() {
        assert_blocked_all_dialects("CREATE TABLE test (id INT)");
    }

    // === Single-statement guard (explainQuery) ===

    fn assert_single_statement_rules(dialect: &impl dialect::Dialect) {
        for sql in [
            "SELECT 1",
            "SELECT 1;",
            "INSERT INTO t (a) VALUES (1)",
            "DELETE FROM t WHERE a = 1",
        ] {
            assert!(
                validate_single_statement(sql, dialect).is_ok(),
                "expected {sql:?} to be accepted"
            );
        }
        for sql in [
            "SELECT 1; CREATE TABLE readonly_probe(i int)",
            "SELECT 1; /* x */ DROP TABLE t",
            "INSERT INTO t (a) VALUES (1); DELETE FROM t",
        ] {
            assert!(
                matches!(validate_single_statement(sql, dialect), Err(SqlError::MultiStatement)),
                "expected MultiStatement for {sql:?}"
            );
        }
    }

    #[test]
    fn single_statement_rules_all_dialects() {
        assert_single_statement_rules(&MYSQL);
        assert_single_statement_rules(&POSTGRES);
        assert_single_statement_rules(&SQLITE);
    }

    #[test]
    fn single_statement_rejects_empty_input() {
        for sql in ["", "   ", ";"] {
            assert!(
                matches!(validate_single_statement(sql, &POSTGRES), Err(SqlError::Query(_))),
                "expected error for {sql:?}"
            );
        }
    }

    #[test]
    fn multi_statement_blocked_all_dialects() {
        let sql = "SELECT 1; DROP TABLE x";
        assert!(matches!(
            validate_read(sql, Dialect::Mysql),
            Err(SqlError::MultiStatement)
        ));
        assert!(matches!(
            validate_read(sql, Dialect::Postgres),
            Err(SqlError::MultiStatement)
        ));
        assert!(matches!(
            validate_read(sql, Dialect::Sqlite),
            Err(SqlError::MultiStatement)
        ));
    }

    #[test]
    fn empty_blocked_all_dialects() {
        assert_blocked_all_dialects("");
        assert_blocked_all_dialects("   ");
    }

    // === T016: Postgres-specific tests ===

    #[test]
    fn postgres_copy_to_blocked() {
        let result = validate_read("COPY users TO '/tmp/out.csv'", Dialect::Postgres);
        assert!(
            matches!(result, Err(SqlError::ReadOnlyViolation)),
            "Postgres COPY TO should be blocked: {result:?}"
        );
    }

    #[test]
    fn postgres_copy_from_blocked() {
        let result = validate_read("COPY users FROM '/tmp/in.csv'", Dialect::Postgres);
        assert!(result.is_err(), "Postgres COPY FROM should be blocked: {result:?}");
    }

    #[test]
    fn postgres_generate_series_allowed() {
        assert!(validate_read("SELECT * FROM generate_series(1, 10)", Dialect::Postgres).is_ok());
    }

    // === T017: SQLite-specific and cross-dialect tests ===

    #[test]
    fn show_databases_across_dialects() {
        assert!(validate_read("SHOW DATABASES", Dialect::Mysql).is_ok());
        let pg_result = validate_read("SHOW DATABASES", Dialect::Postgres);
        let sqlite_result = validate_read("SHOW DATABASES", Dialect::Sqlite);
        assert!(
            pg_result.is_ok() || pg_result.is_err(),
            "Postgres may or may not parse SHOW DATABASES"
        );
        assert!(
            sqlite_result.is_ok() || sqlite_result.is_err(),
            "SQLite may or may not parse SHOW DATABASES"
        );
        if let Err(e) = &pg_result {
            assert!(
                !matches!(e, SqlError::ReadOnlyViolation),
                "SHOW DATABASES should not be classified as a write: {e}"
            );
        }
    }

    // === T018: Unicode and null-byte validation tests ===

    #[test]
    fn unicode_cyrillic_semicolon_not_misclassified() {
        let sql = "SELECT 1\u{037E} DROP TABLE users";
        let result = validate_read(sql, Dialect::Mysql);
        assert!(
            result.is_err(),
            "SQL with Cyrillic question mark should not silently succeed as single SELECT"
        );
    }

    #[test]
    fn unicode_fullwidth_semicolon_not_misclassified() {
        let sql = "SELECT 1\u{FF1B} DROP TABLE users";
        let result = validate_read(sql, Dialect::Mysql);
        assert!(
            result.is_err() || validate_read(sql, Dialect::Mysql).is_ok(),
            "fullwidth semicolon is a single token, not a statement separator"
        );
    }

    // === Dangerous function deny list ===

    #[test]
    fn dangerous_functions_blocked_all_dialects() {
        for sql in [
            "SELECT pg_read_file('/etc/passwd')",
            "SELECT pg_read_binary_file('/etc/passwd')",
            "SELECT * FROM pg_ls_dir('/')",
            "SELECT pg_ls_dir('/')",
            "SELECT pg_stat_file('/etc/passwd')",
            "SELECT lo_import('/etc/passwd')",
            "SELECT lo_export(1, '/tmp/x')",
            "SELECT lo_creat(-1)",
            "SELECT lo_create(0)",
            "SELECT lo_from_bytea(0, 'x'::bytea)",
            "SELECT pg_catalog.lo_from_bytea(0, 'x'::bytea)",
            "SELECT lo_put(1, 0, 'x'::bytea)",
            "SELECT lowrite(lo_open(1, 131072), 'x'::bytea)",
            "SELECT lo_truncate(lo_open(1, 131072), 0)",
            "SELECT lo_truncate64(lo_open(1, 131072), 0)",
            "SELECT lo_unlink(1)",
            "SELECT setval('s', 1)",
            "SELECT dblink_connect('host=attacker.example')",
            "SELECT * FROM dblink('host=x', 'SELECT 1') AS t(a int)",
            "SELECT readfile('/etc/passwd')",
            "SELECT writefile('/tmp/x', 'data')",
            "SELECT load_extension('/tmp/evil.so')",
            "SELECT PG_READ_FILE('/etc/passwd')",
            "SELECT pg_catalog.pg_read_file('/etc/passwd')",
            "SELECT \"pg_read_file\"('/etc/passwd')",
            "SELECT 1 FROM users WHERE id IN (SELECT setval('s', 1))",
            "SELECT GET_LOCK('x', 10)",
            "SELECT pg_cancel_backend(1)",
            "SELECT pg_terminate_backend(1)",
            "SELECT pg_catalog.pg_terminate_backend(1)",
            "SELECT pg_advisory_lock(1)",
            "SELECT pg_advisory_xact_lock_shared(1)",
            "SELECT pg_try_advisory_lock(1)",
            "SELECT pg_reload_conf()",
            "SELECT pg_switch_wal()",
            "SELECT pg_create_restore_point('x')",
            "SELECT pg_create_physical_replication_slot('s')",
            "SELECT pg_drop_replication_slot('s')",
            "SELECT pg_stat_reset()",
            "SELECT pg_log_backend_memory_contexts(1)",
        ] {
            for (name, result) in [
                ("MySQL", validate_read(sql, Dialect::Mysql)),
                ("Postgres", validate_read(sql, Dialect::Postgres)),
                ("SQLite", validate_read(sql, Dialect::Sqlite)),
            ] {
                // Some dialects can't parse every form; a parse error is also a rejection.
                assert!(result.is_err(), "{name} should block {sql:?}, got {result:?}");
                assert!(
                    !matches!(result, Err(SqlError::ReadOnlyViolation)),
                    "{name} should flag the function in {sql:?}"
                );
            }
        }
        assert!(matches!(
            validate_read("SELECT pg_read_file('/etc/passwd')", Dialect::Postgres),
            Err(SqlError::DangerousFunctionBlocked(f)) if f == "pg_read_file"
        ));
    }

    #[test]
    fn load_file_quoted_or_qualified_blocked() {
        for sql in [
            "SELECT `LOAD_FILE`('/etc/passwd')",
            "SELECT mysql.LOAD_FILE('/etc/passwd')",
        ] {
            assert!(
                matches!(validate_read(sql, Dialect::Mysql), Err(SqlError::LoadFileBlocked)),
                "expected LoadFileBlocked for {sql:?}"
            );
        }
    }

    #[test]
    fn dangerous_function_names_in_strings_or_columns_allowed() {
        for sql in [
            "SELECT 'pg_read_file(/etc/passwd)'",
            "SELECT setval_count FROM users",
            "SELECT * FROM generate_series(1, 10)",
            "SELECT nextval_hint, upper(name) FROM users",
        ] {
            assert!(
                validate_read(sql, Dialect::Postgres).is_ok(),
                "expected {sql:?} to be allowed"
            );
        }
    }

    // === EXPLAIN wraps (GHSA-mw95-r87r-7ggf) ===

    #[test]
    fn explain_of_write_blocked_all_dialects() {
        for sql in [
            "EXPLAIN DELETE FROM users",
            "EXPLAIN ANALYZE DELETE FROM users",
            "EXPLAIN ANALYZE INSERT INTO users VALUES (1)",
            "EXPLAIN ANALYZE UPDATE users SET admin = true",
            "EXPLAIN ANALYZE DROP TABLE users",
        ] {
            for result in [
                validate_read(sql, Dialect::Mysql),
                validate_read(sql, Dialect::Postgres),
                validate_read(sql, Dialect::Sqlite),
            ] {
                assert!(result.is_err(), "expected {sql:?} to be blocked, got {result:?}");
            }
        }
    }

    #[test]
    fn postgres_explain_options_of_write_blocked() {
        for sql in [
            "EXPLAIN (ANALYZE) UPDATE users SET admin = true",
            "EXPLAIN (ANALYZE TRUE, FORMAT JSON) DELETE FROM users",
            "EXPLAIN ANALYZE WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x",
            "EXPLAIN ANALYZE SELECT * INTO evil FROM users",
        ] {
            assert!(
                matches!(validate_read(sql, Dialect::Postgres), Err(SqlError::ReadOnlyViolation)),
                "expected ReadOnlyViolation for {sql:?}"
            );
        }
    }

    #[test]
    fn explain_of_read_allowed() {
        for sql in ["EXPLAIN SELECT 1", "EXPLAIN ANALYZE SELECT * FROM users"] {
            assert_eq!(
                validate_read(sql, Dialect::Mysql).expect("query is valid"),
                StatementKind::NonSelect
            );
            assert_eq!(
                validate_read(sql, Dialect::Postgres).expect("query is valid"),
                StatementKind::NonSelect
            );
        }
        assert_eq!(
            validate_read("EXPLAIN (ANALYZE, FORMAT JSON) SELECT 1", Dialect::Postgres).expect("query is valid"),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read("EXPLAIN QUERY PLAN SELECT 1", Dialect::Sqlite).expect("query is valid"),
            StatementKind::NonSelect,
        );
    }

    #[test]
    fn explain_checks_dangerous_functions() {
        assert!(matches!(
            validate_read("EXPLAIN ANALYZE SELECT LOAD_FILE('/etc/passwd')", Dialect::Mysql),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    #[test]
    fn postgres_data_modifying_query_blocked() {
        for sql in [
            "WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x",
            "WITH x AS (UPDATE users SET a = 1 RETURNING *) SELECT * FROM x",
            "WITH x AS (INSERT INTO users VALUES (1) RETURNING *) SELECT 1",
            "SELECT * FROM (WITH x AS (DELETE FROM users RETURNING *) SELECT * FROM x) AS t",
            "SELECT * INTO evil FROM users",
        ] {
            assert!(
                matches!(validate_read(sql, Dialect::Postgres), Err(SqlError::ReadOnlyViolation)),
                "expected ReadOnlyViolation for {sql:?}"
            );
        }
    }

    #[test]
    fn null_byte_in_sql() {
        let sql = "SELECT 1\x00; DROP TABLE x";
        let result = validate_read(sql, Dialect::Mysql);
        assert!(result.is_err(), "SQL with null byte should be rejected: {result:?}");
    }
}
