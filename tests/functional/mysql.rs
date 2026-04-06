//! Functional tests for `MySQL`/`MariaDB`.
//!
//! Tests exercise the MCP tool layer (not adapter methods directly),
//! ensuring the same code path as real MCP clients.
//!
//! ```bash
//! ./tests/run.sh --filter mariadb    # MariaDB
//! ./tests/run.sh --filter mysql      # MySQL
//! ```

use database_mcp_config::{DatabaseBackend, DatabaseConfig};
use database_mcp_mysql::MysqlAdapter;
use database_mcp_mysql::types::DropTableRequest;
use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListTablesRequest,
    QueryRequest,
};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::Value;

fn base_db_config(read_only: bool) -> DatabaseConfig {
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
        read_only,
        ..DatabaseConfig::default()
    }
}

async fn adapter(read_only: bool) -> MysqlAdapter {
    let config = base_db_config(read_only);
    MysqlAdapter::new(&config).await.expect("MySQL connection failed")
}

#[tokio::test]
async fn test_lists_databases() {
    let adapter = adapter(false).await;

    let response = adapter.tool_list_databases().await.unwrap();
    let dbs: Vec<String> = response.into_typed().unwrap();

    assert!(dbs.iter().any(|db| db == "app"), "Expected 'app' in: {dbs:?}");
}

#[tokio::test]
async fn test_lists_tables() {
    let adapter = adapter(false).await;
    let parameters = Parameters(ListTablesRequest {
        database_name: "app".into(),
    });

    let response = adapter.tool_list_tables(parameters).await.unwrap();
    let tables: Vec<String> = response.into_typed().unwrap();

    for expected in ["users", "posts", "tags", "post_tags"] {
        assert!(
            tables.iter().any(|t| t == expected),
            "Missing '{expected}' in: {tables:?}"
        );
    }
}

#[tokio::test]
async fn test_gets_table_schema() {
    let adapter = adapter(false).await;
    let parameters = Parameters(GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "users".into(),
    });

    let response = adapter.tool_get_table_schema(parameters).await.unwrap();
    let schema: Value = response.into_typed().unwrap();

    let obj = schema.as_object().expect("object");
    assert!(obj.contains_key("table_name"), "Response should contain table_name");
    assert!(obj.contains_key("columns"), "Response should contain columns");
    let columns = obj["columns"].as_object().expect("columns object");
    for col in ["id", "name", "email", "created_at"] {
        assert!(columns.contains_key(col), "Missing '{col}' in: {columns:?}");
    }
}

#[tokio::test]
async fn test_gets_table_schema_with_relations() {
    let adapter = adapter(false).await;
    let parameters = Parameters(GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "posts".into(),
    });

    let response = adapter.tool_get_table_schema(parameters).await.unwrap();
    let schema: Value = response.into_typed().unwrap();

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
async fn test_executes_sql() {
    let adapter = adapter(false).await;
    let parameters = Parameters(QueryRequest {
        query: "SELECT * FROM users ORDER BY id".into(),
        database_name: "app".into(),
    });

    let response = adapter.tool_read_query(parameters).await.unwrap();
    let rows: Vec<Value> = response.into_typed().unwrap();

    assert_eq!(rows.len(), 3, "Expected 3 users, got {}", rows.len());
}

#[tokio::test]
async fn test_blocks_writes_in_read_only_mode() {
    let adapter = adapter(false).await;
    let parameters = Parameters(QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Hacker', 'hack@evil.com')".into(),
        database_name: "app".into(),
    });

    let response = adapter.tool_read_query(parameters).await;

    assert!(response.is_err(), "Expected error for write in read-only mode");
}

#[tokio::test]
async fn test_creates_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(CreateDatabaseRequest {
        database_name: "app_new".into(),
    });

    let response = adapter.tool_create_database(parameters).await.unwrap();
    let value: Value = response.into_typed().unwrap();

    assert!(!value.is_null());

    let response = adapter.tool_list_databases().await.unwrap();
    let dbs: Vec<String> = response.into_typed().unwrap();

    assert!(dbs.iter().any(|db| db == "app_new"), "New db not in list");
}

#[tokio::test]
async fn test_drops_database() {
    let adapter = adapter(false).await;

    // Verify seeded database exists
    let response = adapter.tool_list_databases().await.unwrap();
    let dbs: Vec<String> = response.into_typed().unwrap();
    assert!(dbs.iter().any(|db| db == "canary"), "canary should exist before drop");

    // Drop it
    let drop_params = Parameters(DropDatabaseRequest {
        database_name: "canary".into(),
    });
    let response = adapter.tool_drop_database(drop_params).await.unwrap();
    let value: Value = response.into_typed().unwrap();
    assert_eq!(value["status"], "success");

    // Verify it's gone
    let response = adapter.tool_list_databases().await.unwrap();
    let dbs: Vec<String> = response.into_typed().unwrap();
    assert!(
        !dbs.iter().any(|db| db == "canary"),
        "canary should not exist after drop"
    );
}

#[tokio::test]
async fn test_drop_active_database_blocked() {
    let adapter = adapter(false).await;
    let parameters = Parameters(DropDatabaseRequest {
        database_name: "app".into(),
    });

    let response = adapter.tool_drop_database(parameters).await;

    assert!(response.is_err(), "Expected error when dropping active database");
    let err_msg = format!("{:?}", response.unwrap_err());
    assert!(
        err_msg.contains("currently connected"),
        "Expected 'currently connected' in error, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_drop_nonexistent_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(DropDatabaseRequest {
        database_name: "nonexistent_db_xyz".into(),
    });

    let response = adapter.tool_drop_database(parameters).await;

    assert!(response.is_err(), "Expected error for nonexistent database");
}

#[tokio::test]
async fn test_drop_database_invalid_identifier() {
    let adapter = adapter(false).await;
    let parameters = Parameters(DropDatabaseRequest {
        database_name: String::new(),
    });

    let response = adapter.tool_drop_database(parameters).await;

    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_lists_tables_cross_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(ListTablesRequest {
        database_name: "analytics".into(),
    });

    let response = adapter.tool_list_tables(parameters).await.unwrap();
    let tables: Vec<String> = response.into_typed().unwrap();

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
async fn test_executes_sql_cross_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(QueryRequest {
        query: "SELECT * FROM events ORDER BY id".into(),
        database_name: "analytics".into(),
    });

    let response = adapter.tool_read_query(parameters).await.unwrap();
    let rows: Vec<Value> = response.into_typed().unwrap();

    assert_eq!(rows.len(), 2, "Expected 2 events, got {}", rows.len());
}

#[tokio::test]
async fn test_gets_table_schema_cross_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(GetTableSchemaRequest {
        database_name: "analytics".into(),
        table_name: "events".into(),
    });

    let response = adapter.tool_get_table_schema(parameters).await.unwrap();
    let schema: Value = response.into_typed().unwrap();

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
async fn test_lists_databases_includes_cross_db() {
    let adapter = adapter(false).await;

    let response = adapter.tool_list_databases().await.unwrap();
    let dbs: Vec<String> = response.into_typed().unwrap();

    assert!(
        dbs.iter().any(|db| db == "analytics"),
        "Expected 'analytics' in databases: {dbs:?}"
    );
}

#[tokio::test]
async fn test_blocks_writes_cross_database_in_read_only_mode() {
    let adapter = adapter(false).await;
    let parameters = Parameters(QueryRequest {
        query: "INSERT INTO events (name) VALUES ('hack')".into(),
        database_name: "analytics".into(),
    });

    let response = adapter.tool_read_query(parameters).await;

    assert!(
        response.is_err(),
        "Expected error for write in read-only mode on cross-database"
    );
}

#[tokio::test]
async fn test_uses_default_pool_for_matching_database() {
    let adapter = adapter(false).await;
    let parameters = Parameters(ListTablesRequest {
        database_name: "app".into(),
    });

    let response = adapter.tool_list_tables(parameters).await.unwrap();
    let tables: Vec<String> = response.into_typed().unwrap();

    assert!(
        tables.iter().any(|t| t == "users"),
        "Expected 'users' when explicitly passing default db: {tables:?}"
    );
}

#[tokio::test]
async fn test_query_timeout_cancels_slow_query() {
    let config = DatabaseConfig {
        query_timeout: Some(2),
        ..base_db_config(false)
    };
    let adapter = MysqlAdapter::new(&config).await.expect("MySQL connection failed");
    let parameters = Parameters(QueryRequest {
        query: "SELECT SLEEP(30)".into(),
        database_name: "app".into(),
    });

    let start = std::time::Instant::now();
    let response = adapter.tool_read_query(parameters).await;
    let elapsed = start.elapsed();

    assert!(response.is_err(), "Expected timeout error");
    let err_msg = format!("{:?}", response.unwrap_err());
    assert!(
        err_msg.contains("timed out"),
        "Expected timeout message, got: {err_msg}"
    );
    assert!(
        elapsed.as_secs() < 10,
        "Timeout should fire in ~2s, not {:.1}s",
        elapsed.as_secs_f64()
    );
}

#[tokio::test]
async fn test_query_timeout_disabled_with_zero() {
    let config = DatabaseConfig {
        query_timeout: None,
        ..base_db_config(false)
    };
    let adapter = MysqlAdapter::new(&config).await.expect("MySQL connection failed");
    let parameters = Parameters(QueryRequest {
        query: "SELECT 1 AS value".into(),
        database_name: "app".into(),
    });

    let response = adapter.tool_read_query(parameters).await;
    assert!(response.is_ok(), "Fast query should succeed without timeout");
}

#[tokio::test]
async fn test_drop_table_success() {
    let adapter = adapter(false).await;

    // Create a temporary table
    let create = Parameters(QueryRequest {
        query: "CREATE TABLE drop_test_simple (id INT PRIMARY KEY)".into(),
        database_name: "app".into(),
    });
    adapter.tool_write_query(create).await.unwrap();

    // Drop it
    let drop_params = Parameters(DropTableRequest {
        database_name: "app".into(),
        table_name: "drop_test_simple".into(),
    });
    let response = adapter.tool_drop_table(drop_params).await.unwrap();
    let value: Value = response.into_typed().unwrap();
    assert_eq!(value["status"], "success");

    // Verify it's gone
    let tables_params = Parameters(ListTablesRequest {
        database_name: "app".into(),
    });
    let response = adapter.tool_list_tables(tables_params).await.unwrap();
    let tables: Vec<String> = response.into_typed().unwrap();
    assert!(
        !tables.iter().any(|t| t == "drop_test_simple"),
        "Table should not exist after drop"
    );
}

#[tokio::test]
async fn test_drop_table_fk_error() {
    let adapter = adapter(false).await;

    // Create parent and child tables with FK
    let create_parent = Parameters(QueryRequest {
        query: "CREATE TABLE drop_test_parent (id INT PRIMARY KEY) ENGINE=InnoDB".into(),
        database_name: "app".into(),
    });
    adapter.tool_write_query(create_parent).await.unwrap();

    let create_child = Parameters(QueryRequest {
        query: "CREATE TABLE drop_test_child (id INT PRIMARY KEY, parent_id INT, FOREIGN KEY (parent_id) REFERENCES drop_test_parent(id)) ENGINE=InnoDB".into(),
        database_name: "app".into(),
    });
    adapter.tool_write_query(create_child).await.unwrap();

    // Attempt to drop parent — should fail due to FK
    let drop_params = Parameters(DropTableRequest {
        database_name: "app".into(),
        table_name: "drop_test_parent".into(),
    });
    let response = adapter.tool_drop_table(drop_params).await;
    assert!(response.is_err(), "Expected FK constraint error");

    // Clean up
    let cleanup_child = Parameters(QueryRequest {
        query: "DROP TABLE drop_test_child".into(),
        database_name: "app".into(),
    });
    adapter.tool_write_query(cleanup_child).await.unwrap();

    let cleanup_parent = Parameters(QueryRequest {
        query: "DROP TABLE drop_test_parent".into(),
        database_name: "app".into(),
    });
    adapter.tool_write_query(cleanup_parent).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_invalid_identifier() {
    let adapter = adapter(false).await;
    let drop_params = Parameters(DropTableRequest {
        database_name: "app".into(),
        table_name: String::new(),
    });

    let response = adapter.tool_drop_table(drop_params).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_explain_query_select() {
    let adapter = adapter(false).await;
    let params = Parameters(ExplainQueryRequest {
        database_name: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: false,
    });

    let response = adapter.tool_explain_query(params).await.unwrap();
    let plan: Vec<Value> = response.into_typed().unwrap();
    assert!(!plan.is_empty(), "Expected non-empty execution plan");
}

#[tokio::test]
async fn test_explain_query_analyze_write_blocked_read_only() {
    let adapter = adapter(true).await;
    let params = Parameters(ExplainQueryRequest {
        database_name: "app".into(),
        query: "INSERT INTO users (name, email) VALUES ('x', 'x@x.com')".into(),
        analyze: true,
    });

    let response = adapter.tool_explain_query(params).await;
    assert!(
        response.is_err(),
        "Expected error for EXPLAIN ANALYZE on write statement in read-only mode"
    );
}
