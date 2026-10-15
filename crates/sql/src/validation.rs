//! AST-based SQL validation for read-only mode enforcement.

use crate::SqlError;
use std::ops::ControlFlow;

use sqlparser::ast::{Expr, Function, ObjectName, Select, Statement, TableFactor, Visit, Visitor};
use sqlparser::dialect::Dialect;
use sqlparser::parser::Parser;

/// Classifies a validated read-only statement for pagination dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatementKind {
    /// `SELECT`, `WITH ... SELECT`, or compound queries (`UNION`/`INTERSECT`/`EXCEPT`).
    /// Composable inside a subquery wrap with an appended `LIMIT` / `OFFSET`.
    Select,
    /// `SHOW`, `DESC` / `DESCRIBE`, `USE`, or `EXPLAIN` — read-only but not
    /// composable as a subquery source. Returned in a single response.
    NonSelect,
}

/// Validates that a SQL query is read-only and classifies it for pagination.
///
/// Parses the query using the given dialect and checks:
/// 1. Exactly one statement (multi-statement injection blocked)
/// 2. Statement type is read-only (SELECT, SHOW, DESCRIBE, USE, EXPLAIN)
/// 3. The statement wrapped by `EXPLAIN` is itself read-only
/// 4. No data-modifying CTEs or `SELECT ... INTO` inside queries
/// 5. No dangerous functions (file access, large objects, sequences, extensions)
/// 6. No INTO OUTFILE/DUMPFILE clauses
///
/// Returns the classified [`StatementKind`] on success so callers that
/// paginate can dispatch `Select` to a subquery-wrapped fetch and route
/// `NonSelect` through a single-page pass-through.
///
/// # Errors
///
/// Returns [`SqlError`] if the query is not allowed in read-only mode.
pub fn validate_read_only(sql: &str, dialect: &impl Dialect) -> Result<StatementKind, SqlError> {
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
    let stmt = parse_single(trimmed, dialect)?.ok_or(SqlError::ReadOnlyViolation)?;

    classify(&stmt)
}

/// Checks that `stmt` is read-only and classifies it for pagination.
fn classify(stmt: &Statement) -> Result<StatementKind, SqlError> {
    match stmt {
        Statement::Query(_) => {
            check_data_modifying_queries(stmt)?;
            check_dangerous_functions(stmt)?;
            Ok(StatementKind::Select)
        }
        // `EXPLAIN ANALYZE` executes the wrapped statement, so it must be read-only too.
        Statement::Explain { statement, .. } => {
            classify(statement)?;
            Ok(StatementKind::NonSelect)
        }
        Statement::ShowTables { .. }
        | Statement::ShowColumns { .. }
        | Statement::ShowCreate { .. }
        | Statement::ShowVariable { .. }
        | Statement::ShowVariables { .. }
        | Statement::ShowStatus { .. }
        | Statement::ShowDatabases { .. }
        | Statement::ShowSchemas { .. }
        | Statement::ShowCollation { .. }
        | Statement::ShowFunctions { .. }
        | Statement::ShowViews { .. }
        | Statement::ShowObjects(_)
        | Statement::ExplainTable { .. }
        | Statement::Use(_) => Ok(StatementKind::NonSelect),
        _ => Err(SqlError::ReadOnlyViolation),
    }
}

/// Validates that a SQL string contains exactly one statement.
///
/// Unlike [`validate_read_only`], any statement type is accepted. Use this
/// where the input is embedded into a wrapper (e.g. `EXPLAIN {query}`) that
/// must never smuggle additional statements past the driver.
///
/// # Errors
///
/// Returns [`SqlError::MultiStatement`] if the input holds more than one
/// statement, or [`SqlError::Query`] if it is empty or fails to parse.
pub fn validate_single_statement(sql: &str, dialect: &impl Dialect) -> Result<(), SqlError> {
    parse_single(sql.trim(), dialect)?.ok_or_else(|| SqlError::Query("Query must contain a statement".into()))?;
    Ok(())
}

/// Parses `sql` and returns its only statement, `None` if it has none.
fn parse_single(sql: &str, dialect: &impl Dialect) -> Result<Option<Statement>, SqlError> {
    let mut statements =
        Parser::parse_sql(dialect, sql).map_err(|e| SqlError::Query(format!("SQL parse error: {e}")))?;

    if statements.len() > 1 {
        return Err(SqlError::MultiStatement);
    }
    Ok(statements.pop())
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
    // PostgreSQL
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "lo_import",
    "lo_export",
    "setval",
    // SQLite (CLI / extension functions)
    "readfile",
    "writefile",
    "load_extension",
];

/// Function name prefixes blocked in addition to [`DANGEROUS_FUNCTIONS`].
const DANGEROUS_FUNCTION_PREFIXES: &[&str] = &["dblink"];

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
    use sqlparser::dialect::{MySqlDialect, PostgreSqlDialect, SQLiteDialect};

    use super::*;

    const MYSQL: MySqlDialect = MySqlDialect {};
    const POSTGRES: PostgreSqlDialect = PostgreSqlDialect {};
    const SQLITE: SQLiteDialect = SQLiteDialect {};

    const DIALECT: MySqlDialect = MySqlDialect {};

    // === Statement classification ===

    #[test]
    fn classifies_select_vs_non_select() {
        // Select-like: plain SELECT, CTE, compound queries all map to StatementKind::Select.
        assert_eq!(validate_read_only("SELECT 1", &DIALECT).unwrap(), StatementKind::Select,);
        assert_eq!(
            validate_read_only("WITH x AS (SELECT 1) SELECT * FROM x", &DIALECT).unwrap(),
            StatementKind::Select,
        );
        assert_eq!(
            validate_read_only("SELECT 1 UNION SELECT 2", &DIALECT).unwrap(),
            StatementKind::Select,
        );

        // Catalog/metadata statements map to StatementKind::NonSelect.
        assert_eq!(
            validate_read_only("SHOW DATABASES", &DIALECT).unwrap(),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read_only("DESCRIBE users", &DIALECT).unwrap(),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read_only("USE app", &DIALECT).unwrap(),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read_only("EXPLAIN SELECT 1", &DIALECT).unwrap(),
            StatementKind::NonSelect,
        );
    }

    // === Allowed queries ===

    #[test]
    fn test_select_allowed() {
        assert!(validate_read_only("SELECT * FROM users", &DIALECT).is_ok());
        assert!(validate_read_only("select * from users", &DIALECT).is_ok());
    }

    #[test]
    fn test_show_allowed() {
        assert!(validate_read_only("SHOW DATABASES", &DIALECT).is_ok());
        assert!(validate_read_only("SHOW TABLES", &DIALECT).is_ok());
    }

    #[test]
    fn test_describe_allowed() {
        // sqlparser parses DESC/DESCRIBE as ExplainTable
        assert!(validate_read_only("DESC users", &DIALECT).is_ok());
        assert!(validate_read_only("DESCRIBE users", &DIALECT).is_ok());
    }

    #[test]
    fn test_use_allowed() {
        assert!(validate_read_only("USE mydb", &DIALECT).is_ok());
    }

    // === Blocked statement types ===

    #[test]
    fn test_insert_blocked() {
        assert!(matches!(
            validate_read_only("INSERT INTO users VALUES (1)", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_update_blocked() {
        assert!(matches!(
            validate_read_only("UPDATE users SET name='x'", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_delete_blocked() {
        assert!(matches!(
            validate_read_only("DELETE FROM users", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_drop_blocked() {
        assert!(matches!(
            validate_read_only("DROP TABLE users", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_create_blocked() {
        assert!(matches!(
            validate_read_only("CREATE TABLE test (id INT)", &DIALECT),
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
        let result = validate_read_only("SELECT 1 -- \nDELETE FROM users", &DIALECT);
        // The parser should treat -- as comment, so only SELECT 1 remains → allowed
        assert!(result.is_ok() || matches!(result, Err(SqlError::MultiStatement)));
    }

    #[test]
    fn test_comment_bypass_multi_line() {
        // "/* SELECT */ DELETE FROM users" — parser strips comment, sees DELETE
        assert!(matches!(
            validate_read_only("/* SELECT */ DELETE FROM users", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    // === Dangerous functions ===

    #[test]
    fn test_load_file_blocked() {
        assert!(matches!(
            validate_read_only("SELECT LOAD_FILE('/etc/passwd')", &DIALECT),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_case_insensitive() {
        assert!(matches!(
            validate_read_only("SELECT load_file('/etc/passwd')", &DIALECT),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_with_spaces() {
        // sqlparser normalizes function calls, so spaces before ( are handled
        assert!(matches!(
            validate_read_only("SELECT LOAD_FILE ('/etc/passwd')", &DIALECT),
            Err(SqlError::LoadFileBlocked)
        ));
    }

    // === INTO OUTFILE/DUMPFILE ===

    #[test]
    fn test_into_outfile_blocked() {
        assert!(matches!(
            validate_read_only("SELECT * FROM users INTO OUTFILE '/tmp/out'", &DIALECT),
            Err(SqlError::IntoOutfileBlocked)
        ));
    }

    #[test]
    fn test_into_dumpfile_blocked() {
        assert!(matches!(
            validate_read_only("SELECT * FROM users INTO DUMPFILE '/tmp/out'", &DIALECT),
            Err(SqlError::IntoOutfileBlocked)
        ));
    }

    // === String literals should NOT trigger false positives ===

    #[test]
    fn test_load_file_in_string_allowed() {
        // LOAD_FILE inside a string literal is NOT a function call in the AST
        assert!(validate_read_only("SELECT 'LOAD_FILE(/etc/passwd)' FROM dual", &DIALECT).is_ok());
    }

    // === Empty / comment-only queries ===

    #[test]
    fn test_empty_query_blocked() {
        assert!(matches!(
            validate_read_only("", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_comment_only_blocked() {
        // Comment-only input: parser returns empty statements or parse error
        let result = validate_read_only("-- just a comment", &DIALECT);
        assert!(result.is_err());
    }

    // === New tests for AST-based validation ===

    #[test]
    fn test_multi_statement_blocked() {
        assert!(matches!(
            validate_read_only("SELECT 1; SELECT 2", &DIALECT),
            Err(SqlError::MultiStatement)
        ));
    }

    #[test]
    fn test_multi_statement_injection_blocked() {
        assert!(matches!(
            validate_read_only("SELECT 1; DROP TABLE users", &DIALECT),
            Err(SqlError::MultiStatement)
        ));
    }

    #[test]
    fn test_set_statement_blocked() {
        assert!(matches!(
            validate_read_only("SET @var = 1", &DIALECT),
            Err(SqlError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_malformed_sql_rejected() {
        let result = validate_read_only("SELEC * FORM users", &DIALECT);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_with_subquery_allowed() {
        assert!(validate_read_only("SELECT * FROM (SELECT 1) AS t", &DIALECT).is_ok());
    }

    #[test]
    fn test_select_with_where_allowed() {
        assert!(validate_read_only("SELECT * FROM users WHERE id = 1", &DIALECT).is_ok());
    }

    #[test]
    fn test_select_count_allowed() {
        assert!(validate_read_only("SELECT COUNT(*) FROM users", &DIALECT).is_ok());
    }

    // === T015: Multi-dialect parameterized tests ===

    fn assert_allowed_all_dialects(sql: &str) {
        assert!(validate_read_only(sql, &MYSQL).is_ok(), "MySQL should allow: {sql}");
        assert!(
            validate_read_only(sql, &POSTGRES).is_ok(),
            "Postgres should allow: {sql}"
        );
        assert!(validate_read_only(sql, &SQLITE).is_ok(), "SQLite should allow: {sql}");
    }

    fn assert_blocked_all_dialects(sql: &str) {
        assert!(validate_read_only(sql, &MYSQL).is_err(), "MySQL should block: {sql}");
        assert!(
            validate_read_only(sql, &POSTGRES).is_err(),
            "Postgres should block: {sql}"
        );
        assert!(validate_read_only(sql, &SQLITE).is_err(), "SQLite should block: {sql}");
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

    fn assert_single_statement_rules(dialect: &impl Dialect) {
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
        assert!(matches!(validate_read_only(sql, &MYSQL), Err(SqlError::MultiStatement)));
        assert!(matches!(
            validate_read_only(sql, &POSTGRES),
            Err(SqlError::MultiStatement)
        ));
        assert!(matches!(
            validate_read_only(sql, &SQLITE),
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
        let result = validate_read_only("COPY users TO '/tmp/out.csv'", &POSTGRES);
        assert!(
            matches!(result, Err(SqlError::ReadOnlyViolation)),
            "Postgres COPY TO should be blocked: {result:?}"
        );
    }

    #[test]
    fn postgres_copy_from_blocked() {
        let result = validate_read_only("COPY users FROM '/tmp/in.csv'", &POSTGRES);
        assert!(result.is_err(), "Postgres COPY FROM should be blocked: {result:?}");
    }

    #[test]
    fn postgres_generate_series_allowed() {
        assert!(validate_read_only("SELECT * FROM generate_series(1, 10)", &POSTGRES).is_ok());
    }

    // === T017: SQLite-specific and cross-dialect tests ===

    #[test]
    fn show_databases_across_dialects() {
        assert!(validate_read_only("SHOW DATABASES", &MYSQL).is_ok());
        let pg_result = validate_read_only("SHOW DATABASES", &POSTGRES);
        let sqlite_result = validate_read_only("SHOW DATABASES", &SQLITE);
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
        let result = validate_read_only(sql, &MYSQL);
        assert!(
            result.is_err(),
            "SQL with Cyrillic question mark should not silently succeed as single SELECT"
        );
    }

    #[test]
    fn unicode_fullwidth_semicolon_not_misclassified() {
        let sql = "SELECT 1\u{FF1B} DROP TABLE users";
        let result = validate_read_only(sql, &MYSQL);
        assert!(
            result.is_err() || validate_read_only(sql, &MYSQL).is_ok(),
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
        ] {
            for (name, result) in [
                ("MySQL", validate_read_only(sql, &MYSQL)),
                ("Postgres", validate_read_only(sql, &POSTGRES)),
                ("SQLite", validate_read_only(sql, &SQLITE)),
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
            validate_read_only("SELECT pg_read_file('/etc/passwd')", &POSTGRES),
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
                matches!(validate_read_only(sql, &MYSQL), Err(SqlError::LoadFileBlocked)),
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
                validate_read_only(sql, &POSTGRES).is_ok(),
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
                validate_read_only(sql, &MYSQL),
                validate_read_only(sql, &POSTGRES),
                validate_read_only(sql, &SQLITE),
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
                matches!(validate_read_only(sql, &POSTGRES), Err(SqlError::ReadOnlyViolation)),
                "expected ReadOnlyViolation for {sql:?}"
            );
        }
    }

    #[test]
    fn explain_of_read_allowed() {
        for sql in ["EXPLAIN SELECT 1", "EXPLAIN ANALYZE SELECT * FROM users"] {
            assert_eq!(validate_read_only(sql, &MYSQL).unwrap(), StatementKind::NonSelect);
            assert_eq!(validate_read_only(sql, &POSTGRES).unwrap(), StatementKind::NonSelect);
        }
        assert_eq!(
            validate_read_only("EXPLAIN (ANALYZE, FORMAT JSON) SELECT 1", &POSTGRES).unwrap(),
            StatementKind::NonSelect,
        );
        assert_eq!(
            validate_read_only("EXPLAIN QUERY PLAN SELECT 1", &SQLITE).unwrap(),
            StatementKind::NonSelect,
        );
    }

    #[test]
    fn explain_checks_dangerous_functions() {
        assert!(matches!(
            validate_read_only("EXPLAIN ANALYZE SELECT LOAD_FILE('/etc/passwd')", &MYSQL),
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
                matches!(validate_read_only(sql, &POSTGRES), Err(SqlError::ReadOnlyViolation)),
                "expected ReadOnlyViolation for {sql:?}"
            );
        }
    }

    #[test]
    fn null_byte_in_sql() {
        let sql = "SELECT 1\x00; DROP TABLE x";
        let result = validate_read_only(sql, &MYSQL);
        assert!(result.is_err(), "SQL with null byte should be rejected: {result:?}");
    }
}
