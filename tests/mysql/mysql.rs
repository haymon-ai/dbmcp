//! Functional integration tests for MySQL/MariaDB.
//!
//! ```bash
//! ./tests/run.sh --filter mariadb    # MariaDB
//! ./tests/run.sh --filter mysql      # MySQL
//! ```

use sql_mcp::config::Config;
use sql_mcp::db::backend::Backend;
use sql_mcp::db::mysql::MysqlBackend;
use sql_mcp::tools::database;

fn test_config() -> Config {
    Config {
        db_host: std::env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        db_port: std::env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3306),
        db_user: std::env::var("DB_USER").unwrap_or_else(|_| "root".into()),
        db_password: std::env::var("DB_PASSWORD").unwrap_or_else(|_| "password".into()),
        db_name: Some("mcp".into()),
        read_only: false,
        ..Config::default()
    }
}

async fn backend() -> Backend {
    let config = test_config();
    Backend::Mysql(
        MysqlBackend::new(&config)
            .await
            .expect("MySQL connection failed"),
    )
}

async fn readonly_backend() -> Backend {
    let config = Config {
        read_only: true,
        ..test_config()
    };
    Backend::Mysql(
        MysqlBackend::new(&config)
            .await
            .expect("MySQL connection failed"),
    )
}

#[tokio::test]
async fn it_lists_databases() {
    let b = backend().await;
    let result = database::list_databases(&b).await.expect("failed");
    let dbs: Vec<String> = serde_json::from_str(&result).expect("bad json");
    assert!(
        dbs.iter().any(|db| db == "mcp"),
        "Expected 'mcp' in: {dbs:?}"
    );
}

#[tokio::test]
async fn it_lists_tables() {
    let b = backend().await;
    let result = database::list_tables(&b, "mcp").await.expect("failed");
    let tables: Vec<String> = serde_json::from_str(&result).expect("bad json");
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
    let result = database::get_table_schema(&b, "mcp", "users")
        .await
        .expect("failed");
    let schema: serde_json::Value = serde_json::from_str(&result).expect("bad json");
    let columns: Vec<String> = schema
        .as_object()
        .expect("object")
        .keys()
        .cloned()
        .collect();
    for col in ["id", "name", "email", "created_at"] {
        assert!(
            columns.iter().any(|c| c == col),
            "Missing '{col}' in: {columns:?}"
        );
    }
}

#[tokio::test]
async fn it_gets_table_relations() {
    let b = backend().await;
    let result = database::get_table_schema_with_relations(&b, "mcp", "posts")
        .await
        .expect("failed");
    assert!(
        result.contains("user_id") || result.contains("users"),
        "Expected foreign key reference in: {result}"
    );
}

#[tokio::test]
async fn it_executes_sql() {
    let b = backend().await;
    let result = database::tool_execute_sql(&b, "SELECT * FROM users ORDER BY id", "mcp", None)
        .await
        .expect("failed");
    let rows: Vec<serde_json::Value> = serde_json::from_str(&result).expect("bad json");
    assert_eq!(rows.len(), 3, "Expected 3 users, got {}", rows.len());
}

#[tokio::test]
async fn it_blocks_writes_in_read_only_mode() {
    let b = readonly_backend().await;
    let result = database::tool_execute_sql(
        &b,
        "INSERT INTO users (name, email) VALUES ('Hacker', 'hack@evil.com')",
        "mcp",
        None,
    )
    .await;
    assert!(
        result.is_err(),
        "Expected error for write in read-only mode"
    );
}

#[tokio::test]
async fn it_creates_database() {
    let b = backend().await;
    let result = database::create_database(&b, "mcp_new")
        .await
        .expect("failed");
    assert!(!result.is_empty());
    let list = database::list_databases(&b).await.expect("list failed");
    let dbs: Vec<String> = serde_json::from_str(&list).unwrap_or_default();
    assert!(dbs.iter().any(|db| db == "mcp_new"), "New db not in list");
}

#[tokio::test]
async fn it_has_consistent_seed_data() {
    let b = backend().await;

    async fn check(b: &Backend, table: &str, expected: usize) {
        let sql = format!("SELECT CAST(COUNT(*) AS CHAR) as cnt FROM {table}");
        let result = database::tool_execute_sql(b, &sql, "mcp", None)
            .await
            .unwrap_or_else(|e| panic!("count {table}: {e}"));
        let rows: Vec<serde_json::Value> = serde_json::from_str(&result).unwrap();
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
