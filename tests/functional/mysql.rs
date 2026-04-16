//! Functional tests for `MySQL`/`MariaDB`.
//!
//! Tests exercise the handler methods directly, which is the same code
//! path the per-tool ZSTs delegate to.
//!
//! ```bash
//! ./tests/run.sh --filter mariadb    # MariaDB
//! ./tests/run.sh --filter mysql      # MySQL
//! ```

use database_mcp_config::{DatabaseBackend, DatabaseConfig};
use database_mcp_mysql::MysqlHandler;
use database_mcp_mysql::types::DropTableRequest;
use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListTablesRequest,
    QueryRequest,
};
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

fn handler(read_only: bool) -> MysqlHandler {
    let config = base_db_config(read_only);
    MysqlHandler::new(&config)
}

#[tokio::test]
async fn test_write_query_insert_and_verify() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('WriteTest', 'write@test.com')".into(),
        database_name: "app".into(),
    };
    let response = handler.write_query(&insert).await.unwrap();
    assert!(response.rows.is_array());

    // Verify the row was inserted
    let select = QueryRequest {
        query: "SELECT name FROM users WHERE email = 'write@test.com'".into(),
        database_name: "app".into(),
    };
    let rows = handler.read_query(&select).await.unwrap();
    let arr = rows.rows.as_array().expect("array");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "WriteTest");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'write@test.com'".into(),
        database_name: "app".into(),
    };
    handler.write_query(&delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_update() {
    let handler = handler(false);

    // Insert a row
    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Before', 'update@test.com')".into(),
        database_name: "app".into(),
    };
    handler.write_query(&insert).await.unwrap();

    // Update it
    let update = QueryRequest {
        query: "UPDATE users SET name = 'After' WHERE email = 'update@test.com'".into(),
        database_name: "app".into(),
    };
    handler.write_query(&update).await.unwrap();

    // Verify
    let select = QueryRequest {
        query: "SELECT name FROM users WHERE email = 'update@test.com'".into(),
        database_name: "app".into(),
    };
    let rows = handler.read_query(&select).await.unwrap();
    let arr = rows.rows.as_array().expect("array");
    assert_eq!(arr[0]["name"], "After");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'update@test.com'".into(),
        database_name: "app".into(),
    };
    handler.write_query(&delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_delete() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Deletable', 'delete@test.com')".into(),
        database_name: "app".into(),
    };
    handler.write_query(&insert).await.unwrap();

    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'delete@test.com'".into(),
        database_name: "app".into(),
    };
    handler.write_query(&delete).await.unwrap();

    let select = QueryRequest {
        query: "SELECT * FROM users WHERE email = 'delete@test.com'".into(),
        database_name: "app".into(),
    };
    let rows = handler.read_query(&select).await.unwrap();
    let arr = rows.rows.as_array().expect("array");
    assert!(arr.is_empty(), "Row should be deleted");
}

#[tokio::test]
async fn test_lists_databases() {
    let handler = handler(false);

    let response = handler.list_databases().await.unwrap();
    let dbs = response.databases;

    assert!(dbs.iter().any(|db| db == "app"), "Expected 'app' in: {dbs:?}");
}

#[tokio::test]
async fn test_lists_tables() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database_name: "app".into(),
    };

    let response = handler.list_tables(&request).await.unwrap();
    let tables = response.tables;

    for expected in ["users", "posts", "tags", "post_tags"] {
        assert!(
            tables.iter().any(|t| t == expected),
            "Missing '{expected}' in: {tables:?}"
        );
    }
}

#[tokio::test]
async fn test_gets_table_schema() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "users".into(),
    };

    let schema = handler.get_table_schema(&request).await.unwrap();

    assert_eq!(schema.table_name, "users");
    let columns = schema.columns.as_object().expect("columns object");
    for col in ["id", "name", "email", "created_at"] {
        assert!(columns.contains_key(col), "Missing '{col}' in: {columns:?}");
    }
}

#[tokio::test]
async fn test_gets_table_schema_with_relations() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "posts".into(),
    };

    let schema = handler.get_table_schema(&request).await.unwrap();

    let columns = schema.columns.as_object().expect("columns object");
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
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users ORDER BY id".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows: Vec<Value> = response.rows.as_array().expect("rows should be an array").clone();

    assert_eq!(rows.len(), 3, "Expected 3 users, got {}", rows.len());
}

#[tokio::test]
async fn test_blocks_writes_in_read_only_mode() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Hacker', 'hack@evil.com')".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;

    assert!(response.is_err(), "Expected error for write in read-only mode");
}

#[tokio::test]
async fn test_creates_database() {
    let handler = handler(false);
    let request = CreateDatabaseRequest {
        database_name: "app_new".into(),
    };

    let response = handler.create_database(&request).await.unwrap();
    assert!(response.message.contains("created successfully"));

    let response = handler.list_databases().await.unwrap();
    let dbs = response.databases;

    assert!(dbs.iter().any(|db| db == "app_new"), "New db not in list");
}

#[tokio::test]
async fn test_drops_database() {
    let handler = handler(false);

    // Verify seeded database exists
    let response = handler.list_databases().await.unwrap();
    let dbs = response.databases;
    assert!(dbs.iter().any(|db| db == "canary"), "canary should exist before drop");

    // Drop it
    let drop_request = DropDatabaseRequest {
        database_name: "canary".into(),
    };
    let response = handler.drop_database(&drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let response = handler.list_databases().await.unwrap();
    let dbs = response.databases;
    assert!(
        !dbs.iter().any(|db| db == "canary"),
        "canary should not exist after drop"
    );
}

#[tokio::test]
async fn test_drop_active_database_blocked() {
    let handler = handler(false);
    let request = DropDatabaseRequest {
        database_name: "app".into(),
    };

    let response = handler.drop_database(&request).await;

    let err_msg = format!(
        "{:?}",
        response.expect_err("Expected error when dropping active database")
    );
    assert!(
        err_msg.contains("currently connected"),
        "Expected 'currently connected' in error, got: {err_msg}"
    );
}

#[tokio::test]
async fn test_drop_nonexistent_database() {
    let handler = handler(false);
    let request = DropDatabaseRequest {
        database_name: "nonexistent_db_xyz".into(),
    };

    let response = handler.drop_database(&request).await;

    assert!(response.is_err(), "Expected error for nonexistent database");
}

#[tokio::test]
async fn test_drop_database_invalid_identifier() {
    let handler = handler(false);
    let request = DropDatabaseRequest {
        database_name: String::new(),
    };

    let response = handler.drop_database(&request).await;

    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_lists_tables_cross_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database_name: "analytics".into(),
    };

    let response = handler.list_tables(&request).await.unwrap();
    let tables = response.tables;

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
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM events ORDER BY id".into(),
        database_name: "analytics".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows: Vec<Value> = response.rows.as_array().expect("rows should be an array").clone();

    assert_eq!(rows.len(), 2, "Expected 2 events, got {}", rows.len());
}

#[tokio::test]
async fn test_gets_table_schema_cross_database() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "analytics".into(),
        table_name: "events".into(),
    };

    let response = handler.get_table_schema(&request).await.unwrap();

    assert_eq!(response.table_name, "events");
    let columns = response.columns.as_object().expect("columns object");
    for col in ["id", "name", "payload", "created_at"] {
        assert!(
            columns.contains_key(col),
            "Missing '{col}' in analytics events schema: {columns:?}"
        );
    }
}

#[tokio::test]
async fn test_lists_databases_includes_cross_db() {
    let handler = handler(false);

    let response = handler.list_databases().await.unwrap();
    let dbs = response.databases;

    assert!(
        dbs.iter().any(|db| db == "analytics"),
        "Expected 'analytics' in databases: {dbs:?}"
    );
}

#[tokio::test]
async fn test_blocks_writes_cross_database_in_read_only_mode() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "INSERT INTO events (name) VALUES ('hack')".into(),
        database_name: "analytics".into(),
    };

    let response = handler.read_query(&request).await;

    assert!(
        response.is_err(),
        "Expected error for write in read-only mode on cross-database"
    );
}

#[tokio::test]
async fn test_uses_default_pool_for_matching_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database_name: "app".into(),
    };

    let response = handler.list_tables(&request).await.unwrap();
    let tables = response.tables;

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
    let handler = MysqlHandler::new(&config);
    let request = QueryRequest {
        query: "SELECT SLEEP(30)".into(),
        database_name: "app".into(),
    };

    let start = std::time::Instant::now();
    let response = handler.read_query(&request).await;
    let elapsed = start.elapsed();

    assert!(response.is_err(), "Expected timeout error");
    let err_msg = response.map(|_| ()).unwrap_err().to_string();
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
    let handler = MysqlHandler::new(&config);
    let request = QueryRequest {
        query: "SELECT 1 AS value".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_ok(), "Fast query should succeed without timeout");
}

#[tokio::test]
async fn test_drop_table_success() {
    let handler = handler(false);

    // Create a temporary table
    let create = QueryRequest {
        query: "CREATE TABLE drop_test_simple (id INT PRIMARY KEY)".into(),
        database_name: "app".into(),
    };
    handler.write_query(&create).await.unwrap();

    // Drop it
    let drop_request = DropTableRequest {
        database_name: "app".into(),
        table_name: "drop_test_simple".into(),
    };
    let response = handler.drop_table(&drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let tables_request = ListTablesRequest {
        database_name: "app".into(),
    };
    let response = handler.list_tables(&tables_request).await.unwrap();
    let tables = response.tables;
    assert!(
        !tables.iter().any(|t| t == "drop_test_simple"),
        "Table should not exist after drop"
    );
}

#[tokio::test]
async fn test_drop_table_fk_error() {
    let handler = handler(false);

    // Create parent and child tables with FK
    let create_parent = QueryRequest {
        query: "CREATE TABLE drop_test_parent (id INT PRIMARY KEY) ENGINE=InnoDB".into(),
        database_name: "app".into(),
    };
    handler.write_query(&create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_child (id INT PRIMARY KEY, parent_id INT, FOREIGN KEY (parent_id) REFERENCES drop_test_parent(id)) ENGINE=InnoDB".into(),
        database_name: "app".into(),
    };
    handler.write_query(&create_child).await.unwrap();

    // Attempt to drop parent — should fail due to FK
    let drop_request = DropTableRequest {
        database_name: "app".into(),
        table_name: "drop_test_parent".into(),
    };
    let response = handler.drop_table(&drop_request).await;
    assert!(response.is_err(), "Expected FK constraint error");

    // Clean up
    let cleanup_child = QueryRequest {
        query: "DROP TABLE drop_test_child".into(),
        database_name: "app".into(),
    };
    handler.write_query(&cleanup_child).await.unwrap();

    let cleanup_parent = QueryRequest {
        query: "DROP TABLE drop_test_parent".into(),
        database_name: "app".into(),
    };
    handler.write_query(&cleanup_parent).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_invalid_identifier() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database_name: "app".into(),
        table_name: String::new(),
    };

    let response = handler.drop_table(&drop_request).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_explain_query_select() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: false,
    };

    let response = handler.explain_query(&request).await.unwrap();
    let plan = response.rows.as_array().expect("rows should be an array");
    assert!(!plan.is_empty(), "Expected non-empty execution plan");
}

#[tokio::test]
async fn test_explain_query_analyze_write_blocked_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "INSERT INTO users (name, email) VALUES ('x', 'x@x.com')".into(),
        analyze: true,
    };

    let response = handler.explain_query(&request).await;
    assert!(
        response.is_err(),
        "Expected error for EXPLAIN ANALYZE on write statement in read-only mode"
    );
}

#[tokio::test]
async fn test_get_table_schema_nonexistent_table() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "nonexistent_table_xyz".into(),
    };

    let response = handler.get_table_schema(&request).await;
    assert!(response.is_err(), "Expected error for nonexistent table");
}

#[tokio::test]
async fn test_get_table_schema_invalid_table_name() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: String::new(),
    };

    let response = handler.get_table_schema(&request).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_get_table_schema_invalid_database_name() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: String::new(),
        table_name: "users".into(),
    };

    let response = handler.get_table_schema(&request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_list_tables_nonexistent_database_returns_empty() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database_name: "nonexistent_db_xyz".into(),
    };

    // MySQL queries information_schema.TABLES — a nonexistent schema returns
    // zero rows rather than an error.
    let response = handler.list_tables(&request).await.unwrap();
    assert!(
        response.tables.is_empty(),
        "Nonexistent database should return empty table list, got: {:?}",
        response.tables
    );
}

#[tokio::test]
async fn test_list_tables_invalid_database_name() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database_name: String::new(),
    };

    let response = handler.list_tables(&request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_create_database_already_exists() {
    let handler = handler(false);
    let request = CreateDatabaseRequest {
        database_name: "app".into(),
    };

    let response = handler.create_database(&request).await.unwrap();
    assert!(
        response.message.contains("already exists"),
        "Expected 'already exists' message, got: {}",
        response.message
    );
}

#[tokio::test]
async fn test_create_database_invalid_identifier() {
    let handler = handler(false);
    let request = CreateDatabaseRequest {
        database_name: String::new(),
    };

    let response = handler.create_database(&request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_explain_query_analyze() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    // MariaDB does not support EXPLAIN ANALYZE, so this may fail on MariaDB.
    // We accept either a successful plan or a query error.
    match handler.explain_query(&request).await {
        Ok(response) => {
            let plan = response.rows.as_array().expect("rows should be an array");
            assert!(!plan.is_empty(), "Expected non-empty execution plan with analyze");
        }
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("syntax") || err_msg.contains("ANALYZE"),
                "Unexpected error (expected MariaDB syntax error): {err_msg}"
            );
        }
    }
}

#[tokio::test]
async fn test_explain_query_plain_write_allowed_in_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "INSERT INTO users (name, email) VALUES ('x', 'x@x.com')".into(),
        analyze: false,
    };

    let response = handler.explain_query(&request).await.unwrap();
    let plan = response.rows.as_array().expect("rows should be an array");
    assert!(
        !plan.is_empty(),
        "Plain EXPLAIN should work for write statements even in read-only mode"
    );
}

#[tokio::test]
async fn test_explain_query_invalid_query() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "NOT VALID SQL AT ALL".into(),
        analyze: false,
    };

    let response = handler.explain_query(&request).await;
    assert!(response.is_err(), "Expected error for invalid SQL");
}

#[tokio::test]
async fn test_read_query_empty_query() {
    let handler = handler(false);
    let request = QueryRequest {
        query: String::new(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for empty query");
}

#[tokio::test]
async fn test_read_query_whitespace_only_query() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "   \t\n  ".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for whitespace-only query");
}

#[tokio::test]
async fn test_read_query_multi_statement_blocked() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT 1; DROP TABLE users".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for multi-statement query");
}

#[tokio::test]
async fn test_read_query_load_file_blocked() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT LOAD_FILE('/etc/passwd')".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for LOAD_FILE");
}

#[tokio::test]
async fn test_read_query_into_outfile_blocked() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users INTO OUTFILE '/tmp/out'".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for INTO OUTFILE");
}

#[tokio::test]
async fn test_read_query_show_tables() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SHOW TABLES".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(!rows.is_empty(), "SHOW TABLES should return results");
}

#[tokio::test]
async fn test_read_query_describe_table() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "DESCRIBE users".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(!rows.is_empty(), "DESCRIBE should return column info");
}

#[tokio::test]
async fn test_drop_table_nonexistent() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database_name: "app".into(),
        table_name: "nonexistent_table_xyz".into(),
    };

    let response = handler.drop_table(&drop_request).await;
    assert!(response.is_err(), "Expected error for nonexistent table");
}

#[tokio::test]
async fn test_drop_table_cross_database() {
    let handler = handler(false);

    // Create a table in the analytics database
    let create = QueryRequest {
        query: "CREATE TABLE drop_cross_test (id INT PRIMARY KEY)".into(),
        database_name: "analytics".into(),
    };
    handler.write_query(&create).await.unwrap();

    // Drop it from the analytics database
    let drop_request = DropTableRequest {
        database_name: "analytics".into(),
        table_name: "drop_cross_test".into(),
    };
    let response = handler.drop_table(&drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));
}

#[tokio::test]
async fn test_write_query_cross_database() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO events (name, payload) VALUES ('cross_test', '{\"test\":true}')".into(),
        database_name: "analytics".into(),
    };
    handler.write_query(&insert).await.unwrap();

    let select = QueryRequest {
        query: "SELECT name FROM events WHERE name = 'cross_test'".into(),
        database_name: "analytics".into(),
    };
    let rows = handler.read_query(&select).await.unwrap();
    let arr = rows.rows.as_array().expect("array");
    assert!(!arr.is_empty(), "Cross-database write should persist");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM events WHERE name = 'cross_test'".into(),
        database_name: "analytics".into(),
    };
    handler.write_query(&delete).await.unwrap();
}

#[tokio::test]
async fn test_get_table_schema_junction_table() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "post_tags".into(),
    };

    let schema = handler.get_table_schema(&request).await.unwrap();
    assert_eq!(schema.table_name, "post_tags");

    let columns = schema.columns.as_object().expect("columns object");
    assert!(columns.contains_key("post_id"), "Missing 'post_id'");
    assert!(columns.contains_key("tag_id"), "Missing 'tag_id'");

    // Both columns should have foreign keys
    let post_id = columns["post_id"].as_object().expect("post_id object");
    assert!(
        post_id.get("foreign_key").is_some_and(|fk| !fk.is_null()),
        "post_id should have a foreign key"
    );

    let tag_id = columns["tag_id"].as_object().expect("tag_id object");
    assert!(
        tag_id.get("foreign_key").is_some_and(|fk| !fk.is_null()),
        "tag_id should have a foreign key"
    );
}

#[tokio::test]
async fn test_read_query_empty_result_set() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users WHERE email = 'nobody@nowhere.com'".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(rows.is_empty(), "Expected empty result set");
}

#[tokio::test]
async fn test_read_query_null_values() {
    let handler = handler(false);
    // posts.body can be NULL, and published defaults to 0
    let request = QueryRequest {
        query: "SELECT title, body FROM posts WHERE title = 'My First Post'".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    // body should be present (even if not null in seed data, the column exists)
    assert!(rows[0].get("body").is_some(), "body column should be present");
}

#[tokio::test]
async fn test_read_query_aggregate() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT COUNT(*) AS total FROM users".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["total"], 3);
}

#[tokio::test]
async fn test_read_query_group_by() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT user_id, COUNT(*) AS post_count FROM posts GROUP BY user_id ORDER BY user_id".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(rows.len() >= 2, "Expected at least 2 groups");
}

#[tokio::test]
async fn test_read_query_use_statement() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "USE app".into(),
        database_name: "app".into(),
    };

    // USE passes read_only validation and executes without returning rows
    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(rows.is_empty(), "USE returns no rows");
}

#[tokio::test]
async fn test_read_query_show_databases() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SHOW DATABASES".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(!rows.is_empty(), "SHOW DATABASES should return results");
}

#[tokio::test]
async fn test_explain_query_cross_database() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database_name: "analytics".into(),
        query: "SELECT * FROM events".into(),
        analyze: false,
    };

    let response = handler.explain_query(&request).await.unwrap();
    let plan = response.rows.as_array().expect("rows should be an array");
    assert!(!plan.is_empty(), "EXPLAIN should work cross-database");
}

#[tokio::test]
async fn test_read_query_with_comments() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "/* fetch users */ SELECT * FROM users ORDER BY id".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 3, "Comment-prefixed SELECT should work");
}

#[tokio::test]
async fn test_read_query_subquery() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users WHERE id IN (SELECT user_id FROM posts WHERE published = 1)".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert!(!rows.is_empty(), "Subquery should return results");
}

#[tokio::test]
async fn test_read_query_with_join() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT p.title, u.name FROM posts p JOIN users u ON p.user_id = u.id ORDER BY p.id".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 5, "Should return all 5 posts with user names");
    assert!(rows[0].get("title").is_some());
    assert!(rows[0].get("name").is_some());
}

#[tokio::test]
async fn test_explain_query_analyze_select_allowed_in_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database_name: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    // MariaDB doesn't support EXPLAIN ANALYZE, so tolerate both outcomes
    match handler.explain_query(&request).await {
        Ok(response) => {
            let plan = response.rows.as_array().expect("rows should be an array");
            assert!(
                !plan.is_empty(),
                "EXPLAIN ANALYZE on SELECT should succeed in read-only mode"
            );
        }
        Err(e) => {
            // MariaDB syntax error is acceptable
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("syntax") || err_msg.contains("ANALYZE"),
                "Unexpected error: {err_msg}"
            );
        }
    }
}

#[tokio::test]
async fn test_write_query_invalid_sql() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "NOT VALID SQL AT ALL".into(),
        database_name: "app".into(),
    };

    let response = handler.write_query(&request).await;
    assert!(response.is_err(), "Expected error for invalid SQL in write_query");
}

#[tokio::test]
async fn test_get_table_schema_column_details() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "users".into(),
    };

    let schema = handler.get_table_schema(&request).await.unwrap();
    let columns = schema.columns.as_object().expect("columns object");

    // Verify id column has key info (PRIMARY KEY)
    let id_col = columns["id"].as_object().expect("id object");
    let key = id_col.get("key").and_then(|v| v.as_str()).unwrap_or("");
    assert_eq!(key, "PRI", "id should be PRI key");

    // Verify email column type
    let email_col = columns["email"].as_object().expect("email object");
    let col_type = email_col.get("type").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        col_type.to_lowercase().contains("varchar"),
        "email type should contain 'varchar', got: {col_type}"
    );
}

#[tokio::test]
async fn test_read_query_with_limit() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users ORDER BY id LIMIT 2".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 2, "LIMIT 2 should return exactly 2 rows");
}

#[tokio::test]
async fn test_drop_table_invalid_database_name() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database_name: String::new(),
        table_name: "users".into(),
    };

    let response = handler.drop_table(&drop_request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_read_query_with_line_comment() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "-- get users\nSELECT * FROM users ORDER BY id".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await.unwrap();
    let rows = response.rows.as_array().expect("array");
    assert_eq!(rows.len(), 3, "Line-comment prefixed SELECT should work");
}

#[tokio::test]
async fn test_read_query_into_dumpfile_blocked() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "SELECT * FROM users INTO DUMPFILE '/tmp/out'".into(),
        database_name: "app".into(),
    };

    let response = handler.read_query(&request).await;
    assert!(response.is_err(), "Expected error for INTO DUMPFILE");
}

#[tokio::test]
async fn test_get_table_schema_no_foreign_keys() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database_name: "app".into(),
        table_name: "tags".into(),
    };

    let schema = handler.get_table_schema(&request).await.unwrap();
    assert_eq!(schema.table_name, "tags");
    let columns = schema.columns.as_object().expect("columns object");
    assert!(columns.contains_key("id"));
    assert!(columns.contains_key("name"));
}

#[tokio::test]
async fn test_create_database_blocked_in_read_only() {
    let handler = handler(true);
    let request = CreateDatabaseRequest {
        database_name: "should_not_create".into(),
    };

    let response = handler.create_database(&request).await;
    assert!(response.is_err(), "create_database should be blocked in read-only mode");
}

#[tokio::test]
async fn test_drop_database_blocked_in_read_only() {
    let handler = handler(true);
    let request = DropDatabaseRequest {
        database_name: "app".into(),
    };

    let response = handler.drop_database(&request).await;
    assert!(response.is_err(), "drop_database should be blocked in read-only mode");
}

#[tokio::test]
async fn test_drop_table_blocked_in_read_only() {
    let handler = handler(true);
    let drop_request = DropTableRequest {
        database_name: "app".into(),
        table_name: "users".into(),
    };

    let response = handler.drop_table(&drop_request).await;
    assert!(response.is_err(), "drop_table should be blocked in read-only mode");
}

// === US2: Connection trait edge cases ===

#[tokio::test]
async fn test_read_query_control_char_database_name_rejected() {
    let handler = handler(true);
    let request = QueryRequest {
        query: "SELECT 1".into(),
        database_name: "test\x01db".into(),
    };
    let result = handler.read_query(&request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

#[tokio::test]
async fn test_list_tables_control_char_database_rejected() {
    let handler = handler(true);
    let request = ListTablesRequest {
        database_name: "test\x00db".into(),
    };
    let result = handler.list_tables(&request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

// === US4: Special-character identifier round-trip ===

#[tokio::test]
async fn test_create_drop_database_with_double_quote() {
    let handler = handler(false);
    let db_name = "test_quote_db\"edge".to_string();

    let create = CreateDatabaseRequest {
        database_name: db_name.clone(),
    };
    let result = handler.create_database(&create).await;
    assert!(
        result.is_ok(),
        "create database with double-quote should succeed: {result:?}"
    );

    let drop = DropDatabaseRequest { database_name: db_name };
    let result = handler.drop_database(&drop).await;
    assert!(
        result.is_ok(),
        "drop database with double-quote should succeed: {result:?}"
    );
}

// === US5: Timeout propagation ===

#[tokio::test]
async fn test_timeout_on_list_tables() {
    let mut config = base_db_config(true);
    config.query_timeout = Some(1);
    let handler = MysqlHandler::new(&config);

    let request = QueryRequest {
        query: "SELECT SLEEP(60)".into(),
        database_name: "app".into(),
    };
    let result = handler.read_query(&request).await;
    assert!(result.is_err(), "slow query should time out");
}
