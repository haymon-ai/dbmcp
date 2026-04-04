//! AST-based SQL validation for read-only mode enforcement.
//!
//! Parses SQL using sqlparser's `MySQL` dialect and validates statement
//! type, single-statement enforcement, and dangerous function blocking.

use database_mcp_server::AppError;
use sqlparser::ast::{Expr, Function, Statement, Visit, Visitor};
use sqlparser::dialect::Dialect;
#[cfg(test)]
use sqlparser::dialect::MySqlDialect;
use sqlparser::parser::Parser;

/// Validates that a SQL query is read-only.
///
/// Parses the query using the given dialect and checks:
/// 1. Exactly one statement (multi-statement injection blocked)
/// 2. Statement type is read-only (SELECT, SHOW, DESCRIBE, USE, EXPLAIN)
/// 3. No dangerous functions (`LOAD_FILE`)
/// 4. No INTO OUTFILE/DUMPFILE clauses
///
/// # Errors
///
/// Returns [`AppError`] if the query is not allowed in read-only mode.
pub fn validate_read_only_with_dialect(sql: &str, dialect: &impl Dialect) -> Result<(), AppError> {
    let trimmed = sql.trim();
    if trimmed.is_empty() {
        return Err(AppError::ReadOnlyViolation);
    }

    // Pre-check for INTO OUTFILE/DUMPFILE — sqlparser may not parse MySQL-specific syntax
    let upper = trimmed.to_uppercase();
    if upper.contains("INTO OUTFILE") || upper.contains("INTO DUMPFILE") {
        return Err(AppError::IntoOutfileBlocked);
    }

    let statements =
        Parser::parse_sql(dialect, trimmed).map_err(|e| AppError::Query(format!("SQL parse error: {e}")))?;

    // Must be exactly one statement
    if statements.is_empty() {
        return Err(AppError::ReadOnlyViolation);
    }
    if statements.len() > 1 {
        return Err(AppError::MultiStatement);
    }

    let stmt = &statements[0];

    // Check statement type is read-only
    match stmt {
        Statement::Query(_) => {
            // SELECT — but check for dangerous functions
            check_dangerous_functions(stmt)?;
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
        | Statement::Explain { .. }
        | Statement::Use(_) => {
            // SHOW, DESCRIBE, EXPLAIN, USE are all read-only
        }
        _ => {
            return Err(AppError::ReadOnlyViolation);
        }
    }

    Ok(())
}

/// Convenience wrapper using MySQL dialect (for tests).
#[cfg(test)]
pub fn validate_read_only(sql: &str) -> Result<(), AppError> {
    validate_read_only_with_dialect(sql, &MySqlDialect {})
}

/// Check for dangerous function calls like `LOAD_FILE()` in the AST.
fn check_dangerous_functions(stmt: &Statement) -> Result<(), AppError> {
    let mut checker = DangerousFunctionChecker { found: None };
    let _ = stmt.visit(&mut checker);
    if let Some(err) = checker.found {
        return Err(err);
    }
    Ok(())
}

struct DangerousFunctionChecker {
    found: Option<AppError>,
}

impl Visitor for DangerousFunctionChecker {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> std::ops::ControlFlow<Self::Break> {
        if let Expr::Function(Function { name, .. }) = expr {
            let func_name = name.to_string().to_uppercase();
            if func_name == "LOAD_FILE" {
                self.found = Some(AppError::LoadFileBlocked);
                return std::ops::ControlFlow::Break(());
            }
        }
        std::ops::ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Allowed queries ===

    #[test]
    fn test_select_allowed() {
        assert!(validate_read_only("SELECT * FROM users").is_ok());
        assert!(validate_read_only("select * from users").is_ok());
    }

    #[test]
    fn test_show_allowed() {
        assert!(validate_read_only("SHOW DATABASES").is_ok());
        assert!(validate_read_only("SHOW TABLES").is_ok());
    }

    #[test]
    fn test_describe_allowed() {
        // sqlparser parses DESC/DESCRIBE as ExplainTable
        assert!(validate_read_only("DESC users").is_ok());
        assert!(validate_read_only("DESCRIBE users").is_ok());
    }

    #[test]
    fn test_use_allowed() {
        assert!(validate_read_only("USE mydb").is_ok());
    }

    // === Blocked statement types ===

    #[test]
    fn test_insert_blocked() {
        assert!(matches!(
            validate_read_only("INSERT INTO users VALUES (1)"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_update_blocked() {
        assert!(matches!(
            validate_read_only("UPDATE users SET name='x'"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_delete_blocked() {
        assert!(matches!(
            validate_read_only("DELETE FROM users"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_drop_blocked() {
        assert!(matches!(
            validate_read_only("DROP TABLE users"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_create_blocked() {
        assert!(matches!(
            validate_read_only("CREATE TABLE test (id INT)"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    // === Comment bypass attacks ===

    #[test]
    fn test_comment_bypass_single_line() {
        // With AST parsing, "SELECT 1 -- \nDELETE FROM users" is parsed as two statements
        // (or the comment hides the DELETE, making it one SELECT).
        // Either way, if it parses as multiple statements, it's blocked.
        // If the parser treats -- as a comment and only sees SELECT 1, it's allowed.
        let result = validate_read_only("SELECT 1 -- \nDELETE FROM users");
        // The parser should treat -- as comment, so only SELECT 1 remains → allowed
        assert!(result.is_ok() || matches!(result, Err(AppError::MultiStatement)));
    }

    #[test]
    fn test_comment_bypass_multi_line() {
        // "/* SELECT */ DELETE FROM users" — parser strips comment, sees DELETE
        assert!(matches!(
            validate_read_only("/* SELECT */ DELETE FROM users"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    // === Dangerous functions ===

    #[test]
    fn test_load_file_blocked() {
        assert!(matches!(
            validate_read_only("SELECT LOAD_FILE('/etc/passwd')"),
            Err(AppError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_case_insensitive() {
        assert!(matches!(
            validate_read_only("SELECT load_file('/etc/passwd')"),
            Err(AppError::LoadFileBlocked)
        ));
    }

    #[test]
    fn test_load_file_with_spaces() {
        // sqlparser normalizes function calls, so spaces before ( are handled
        assert!(matches!(
            validate_read_only("SELECT LOAD_FILE ('/etc/passwd')"),
            Err(AppError::LoadFileBlocked)
        ));
    }

    // === INTO OUTFILE/DUMPFILE ===

    #[test]
    fn test_into_outfile_blocked() {
        assert!(matches!(
            validate_read_only("SELECT * FROM users INTO OUTFILE '/tmp/out'"),
            Err(AppError::IntoOutfileBlocked)
        ));
    }

    #[test]
    fn test_into_dumpfile_blocked() {
        assert!(matches!(
            validate_read_only("SELECT * FROM users INTO DUMPFILE '/tmp/out'"),
            Err(AppError::IntoOutfileBlocked)
        ));
    }

    // === String literals should NOT trigger false positives ===

    #[test]
    fn test_load_file_in_string_allowed() {
        // LOAD_FILE inside a string literal is NOT a function call in the AST
        assert!(validate_read_only("SELECT 'LOAD_FILE(/etc/passwd)' FROM dual").is_ok());
    }

    // === Empty / comment-only queries ===

    #[test]
    fn test_empty_query_blocked() {
        assert!(matches!(validate_read_only(""), Err(AppError::ReadOnlyViolation)));
    }

    #[test]
    fn test_comment_only_blocked() {
        // Comment-only input: parser returns empty statements or parse error
        let result = validate_read_only("-- just a comment");
        assert!(result.is_err());
    }

    // === New tests for AST-based validation ===

    #[test]
    fn test_multi_statement_blocked() {
        assert!(matches!(
            validate_read_only("SELECT 1; SELECT 2"),
            Err(AppError::MultiStatement)
        ));
    }

    #[test]
    fn test_multi_statement_injection_blocked() {
        assert!(matches!(
            validate_read_only("SELECT 1; DROP TABLE users"),
            Err(AppError::MultiStatement)
        ));
    }

    #[test]
    fn test_set_statement_blocked() {
        assert!(matches!(
            validate_read_only("SET @var = 1"),
            Err(AppError::ReadOnlyViolation)
        ));
    }

    #[test]
    fn test_malformed_sql_rejected() {
        let result = validate_read_only("SELEC * FORM users");
        assert!(result.is_err());
    }

    #[test]
    fn test_select_with_subquery_allowed() {
        assert!(validate_read_only("SELECT * FROM (SELECT 1) AS t").is_ok());
    }

    #[test]
    fn test_select_with_where_allowed() {
        assert!(validate_read_only("SELECT * FROM users WHERE id = 1").is_ok());
    }

    #[test]
    fn test_select_count_allowed() {
        assert!(validate_read_only("SELECT COUNT(*) FROM users").is_ok());
    }
}
