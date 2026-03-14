//! Functional integration tests for SQLite.
//!
//! Unlike MySQL/PostgreSQL, SQLite needs no container — it creates a temporary
//! file and seeds it from `tests/sqlite/setup.sql` via sqlx.
//!
//! ```bash
//! ./tests/run.sh --filter sqlite
//! ```

use sql_mcp::db::backend::Backend;
use sql_mcp::db::sqlite::SqliteBackend;
use sql_mcp::tools::database;
use tokio::sync::OnceCell;

static SEEDED: OnceCell<()> = OnceCell::const_new();

async fn seed_db(db_path: &str) {
    let pool = sqlx::SqlitePool::connect(&format!("sqlite:{db_path}?mode=rwc"))
        .await
        .expect("Failed to open SQLite for seeding");
    let seed_sql = include_str!("setup.sql");
    // Strip SQL comments before splitting on semicolons
    let stripped: String = seed_sql
        .lines()
        .filter(|line| !line.trim_start().starts_with("--"))
        .collect::<Vec<_>>()
        .join("\n");
    for statement in stripped.split(';') {
        let trimmed = statement.trim();
        if !trimmed.is_empty() {
            sqlx::query(trimmed)
                .execute(&pool)
                .await
                .unwrap_or_else(|e| panic!("Seed failed on: {trimmed}\nError: {e}"));
        }
    }
    pool.close().await;
}

async fn backend() -> Backend {
    let db_path = std::env::var("DB_PATH").expect("DB_PATH must be set");
    SEEDED.get_or_init(|| seed_db(&db_path)).await;
    Backend::Sqlite(
        SqliteBackend::new(&db_path, false)
            .await
            .expect("SQLite open failed"),
    )
}

async fn readonly_backend() -> Backend {
    let db_path = std::env::var("DB_PATH").expect("DB_PATH must be set");
    SEEDED.get_or_init(|| seed_db(&db_path)).await;
    Backend::Sqlite(
        SqliteBackend::new(&db_path, true)
            .await
            .expect("SQLite open failed"),
    )
}

#[tokio::test]
async fn it_lists_databases() {
    let b = backend().await;
    let result = database::list_databases(&b).await.expect("failed");
    let dbs: Vec<String> = serde_json::from_str(&result).expect("bad json");
    assert!(
        dbs.iter().any(|db| db == "main"),
        "Expected 'main' in: {dbs:?}"
    );
}

#[tokio::test]
async fn it_lists_tables() {
    let b = backend().await;
    let result = database::list_tables(&b, "main").await.expect("failed");
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
    let result = database::get_table_schema(&b, "main", "users")
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
    let result = database::get_table_schema_with_relations(&b, "main", "posts")
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
    let result = database::tool_execute_sql(&b, "SELECT * FROM users ORDER BY id", "main", None)
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
        "main",
        None,
    )
    .await;
    assert!(
        result.is_err(),
        "Expected error for write in read-only mode"
    );
}

#[tokio::test]
async fn it_has_consistent_seed_data() {
    let b = backend().await;

    async fn check(b: &Backend, table: &str, expected: usize) {
        let sql = format!("SELECT CAST(COUNT(*) AS CHAR) as cnt FROM {table}");
        let result = database::tool_execute_sql(b, &sql, "main", None)
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
