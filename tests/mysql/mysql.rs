//! Functional integration tests for MySQL/MariaDB.
//!
//! ```bash
//! ./tests/run.sh --filter mariadb    # MariaDB
//! ./tests/run.sh --filter mysql      # MySQL
//! ```

use database_mcp_config::{DatabaseBackend, DatabaseConfig};
use database_mcp_mysql::MysqlAdapter;
use database_mcp_sql::validation::validate_read_only_with_dialect;

fn test_config() -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Mysql,
        host: std::env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3306),
        user: std::env::var("DB_USER").unwrap_or_else(|_| "root".into()),
        password: std::env::var("DB_PASSWORD").ok(),
        name: Some("app".into()),
        read_only: false,
        ..DatabaseConfig::default()
    }
}

async fn backend() -> MysqlAdapter {
    let config = test_config();
    MysqlAdapter::new(&config).await.expect("MySQL connection failed")
}

async fn readonly_backend() -> MysqlAdapter {
    let config = DatabaseConfig {
        read_only: true,
        ..test_config()
    };
    MysqlAdapter::new(&config).await.expect("MySQL connection failed")
}

#[tokio::test]
async fn it_lists_databases() {
    let b = backend().await;
    let dbs = b.list_databases().await.expect("failed");
    assert!(dbs.iter().any(|db| db == "app"), "Expected 'app' in: {dbs:?}");
}

#[tokio::test]
async fn it_lists_tables() {
    let b = backend().await;
    let tables = b.list_tables("app").await.expect("failed");
    for expected in ["users", "posts", "tags", "post_tags"] {
        assert!(
            tables.iter().any(|t| t == expected),
            "Missing '{expected}' in: {tables:?}"
        );
    }
}

#[tokio::test]
async fn it_gets_table_schema() {
    let b = backend().await;
    let schema = b.get_table_schema("app", "users").await.expect("failed");
    let obj = schema.as_object().expect("object");
    assert!(obj.contains_key("table_name"), "Response should contain table_name");
    assert!(obj.contains_key("columns"), "Response should contain columns");
    let columns = obj["columns"].as_object().expect("columns object");
    for col in ["id", "name", "email", "created_at"] {
        assert!(columns.contains_key(col), "Missing '{col}' in: {columns:?}");
    }
}

#[tokio::test]
async fn it_gets_table_schema_with_relations() {
    let b = backend().await;
    let schema = b.get_table_schema("app", "posts").await.expect("failed");
    let columns = schema["columns"].as_object().expect("columns object");
    assert!(columns.contains_key("user_id"), "Missing 'user_id' column");
    let user_id = columns["user_id"].as_object().expect("user_id object");
    assert!(
        user_id.contains_key("foreign_key"),
        "Missing 'foreign_key' in user_id column"
    );
    assert!(
        !user_id["foreign_key"].is_null(),
        "foreign_key should not be null for user_id"
    );
}

#[tokio::test]
async fn it_executes_sql() {
    let b = backend().await;
    let results = b
        .execute_query("SELECT * FROM users ORDER BY id", Some("app"))
        .await
        .expect("failed");
    let rows = results.as_array().expect("array");
    assert_eq!(rows.len(), 3, "Expected 3 users, got {}", rows.len());
}

#[tokio::test]
async fn it_blocks_writes_in_read_only_mode() {
    let _b = readonly_backend().await;
    let dialect = sqlparser::dialect::MySqlDialect {};
    let result = validate_read_only_with_dialect(
        "INSERT INTO users (name, email) VALUES ('Hacker', 'hack@evil.com')",
        &dialect,
    );
    assert!(result.is_err(), "Expected error for write in read-only mode");
}

#[tokio::test]
async fn it_preserves_json_types() {
    let b = backend().await;

    // COUNT(*) should return a JSON number, not a string or null
    let results = b
        .execute_query("SELECT COUNT(*) as cnt FROM users", Some("app"))
        .await
        .expect("failed");
    let rows = results.as_array().expect("array");
    let cnt = &rows[0]["cnt"];
    assert!(cnt.is_number(), "COUNT(*) should be a number, got: {cnt}");
    assert_eq!(cnt.as_i64(), Some(3), "Expected COUNT(*)=3");

    // Integer and text columns should have correct types
    let results = b
        .execute_query("SELECT id, name FROM users ORDER BY id LIMIT 1", Some("app"))
        .await
        .expect("failed");
    let rows = results.as_array().expect("array");
    assert!(
        rows[0]["id"].is_number(),
        "id should be a number, got: {}",
        rows[0]["id"]
    );
    assert!(
        rows[0]["name"].is_string(),
        "name should be a string, got: {}",
        rows[0]["name"]
    );
}

#[tokio::test]
async fn it_creates_database() {
    let b = backend().await;
    let result = b.create_database("app_new").await.expect("failed");
    assert!(!result.is_null());
    let dbs = b.list_databases().await.expect("list failed");
    assert!(dbs.iter().any(|db| db == "app_new"), "New db not in list");
}

// ---- Cross-database tests ----

#[tokio::test]
async fn it_lists_tables_cross_database() {
    let b = backend().await;
    let tables = b.list_tables("analytics").await.expect("failed");
    assert!(
        tables.iter().any(|t| t == "events"),
        "Expected 'events' in analytics tables: {tables:?}"
    );
    assert!(
        !tables.iter().any(|t| t == "users"),
        "Should not see 'users' from default db in analytics: {tables:?}"
    );
}

#[tokio::test]
async fn it_executes_sql_cross_database() {
    let b = backend().await;
    let results = b
        .execute_query("SELECT * FROM events ORDER BY id", Some("analytics"))
        .await
        .expect("failed");
    let rows = results.as_array().expect("array");
    assert_eq!(rows.len(), 2, "Expected 2 events, got {}", rows.len());
}

#[tokio::test]
async fn it_gets_table_schema_cross_database() {
    let b = backend().await;
    let schema = b.get_table_schema("analytics", "events").await.expect("failed");
    let obj = schema.as_object().expect("object");
    assert!(obj.contains_key("table_name"), "Response should contain table_name");
    let columns = obj["columns"].as_object().expect("columns object");
    for col in ["id", "name", "payload", "created_at"] {
        assert!(
            columns.contains_key(col),
            "Missing '{col}' in analytics events schema: {columns:?}"
        );
    }
}

#[tokio::test]
async fn it_lists_databases_includes_cross_db() {
    let b = backend().await;
    let dbs = b.list_databases().await.expect("failed");
    assert!(
        dbs.iter().any(|db| db == "analytics"),
        "Expected 'analytics' in databases: {dbs:?}"
    );
}

#[tokio::test]
async fn it_blocks_writes_cross_database_in_read_only_mode() {
    let _b = readonly_backend().await;
    let dialect = sqlparser::dialect::MySqlDialect {};
    let result = validate_read_only_with_dialect("INSERT INTO events (name) VALUES ('hack')", &dialect);
    assert!(
        result.is_err(),
        "Expected error for write in read-only mode on cross-database"
    );
}

#[tokio::test]
async fn it_uses_default_pool_for_matching_database() {
    let b = backend().await;
    let tables = b.list_tables("app").await.expect("failed");
    assert!(
        tables.iter().any(|t| t == "users"),
        "Expected 'users' when explicitly passing default db: {tables:?}"
    );
}

#[tokio::test]
async fn it_has_consistent_seed_data() {
    let b = backend().await;

    async fn check(b: &MysqlAdapter, table: &str, expected: usize) {
        let sql = format!("SELECT CAST(COUNT(*) AS CHAR) as cnt FROM {table}");
        let results = b
            .execute_query(&sql, Some("app"))
            .await
            .unwrap_or_else(|e| panic!("count {table}: {e}"));
        let rows = results.as_array().expect("array");
        let count_str = rows[0]
            .get("cnt")
            .and_then(|v| v.as_str())
            .or_else(|| {
                rows[0]
                    .as_object()
                    .and_then(|o| o.values().next())
                    .and_then(|v| v.as_str())
            })
            .unwrap_or_else(|| panic!("No count for {table}: {:?}", rows[0]));
        let count: usize = count_str.parse().unwrap();
        assert_eq!(count, expected, "{table}: expected {expected}, got {count}");
    }

    check(&b, "users", 3).await;
    check(&b, "posts", 5).await;
    check(&b, "tags", 4).await;
    check(&b, "post_tags", 6).await;
}
