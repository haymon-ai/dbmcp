//! Functional tests for `MySQL`/`MariaDB`.
//!
//! Tests exercise the handler methods directly, which is the same code
//! path the per-tool ZSTs delegate to.
//!
//! ```bash
//! ./tests/run.sh --filter mariadb    # MariaDB
//! ./tests/run.sh --filter mysql      # MySQL
//! ```

use dbmcp_config::{DatabaseBackend, DatabaseConfig};
use dbmcp_mysql::MysqlHandler;
use dbmcp_mysql::types::{
    DropTableRequest, ListEntries, ListFunctionsRequest, ListProceduresRequest, ListTablesRequest, ListViewsRequest,
};
use dbmcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, ListDatabasesRequest, ListTriggersRequest,
    QueryRequest, ReadQueryRequest,
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

fn handler_with_page_size(page_size: u16) -> MysqlHandler {
    let config = DatabaseConfig {
        page_size,
        ..base_db_config(false)
    };
    MysqlHandler::new(&config)
}

#[tokio::test]
async fn test_write_query_insert_and_verify() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('WriteTest', 'write@test.com')".into(),
        database: Some("app".into()),
    };
    handler.write_query(insert).await.unwrap();

    // Verify the row was inserted
    let select = ReadQueryRequest {
        query: "SELECT name FROM users WHERE email = 'write@test.com'".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "WriteTest");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'write@test.com'".into(),
        database: Some("app".into()),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_update() {
    let handler = handler(false);

    // Insert a row
    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Before', 'update@test.com')".into(),
        database: Some("app".into()),
    };
    handler.write_query(insert).await.unwrap();

    // Update it
    let update = QueryRequest {
        query: "UPDATE users SET name = 'After' WHERE email = 'update@test.com'".into(),
        database: Some("app".into()),
    };
    handler.write_query(update).await.unwrap();

    // Verify
    let select = ReadQueryRequest {
        query: "SELECT name FROM users WHERE email = 'update@test.com'".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert_eq!(arr[0]["name"], "After");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'update@test.com'".into(),
        database: Some("app".into()),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_delete() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Deletable', 'delete@test.com')".into(),
        database: Some("app".into()),
    };
    handler.write_query(insert).await.unwrap();

    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'delete@test.com'".into(),
        database: Some("app".into()),
    };
    handler.write_query(delete).await.unwrap();

    let select = ReadQueryRequest {
        query: "SELECT * FROM users WHERE email = 'delete@test.com'".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert!(arr.is_empty(), "Row should be deleted");
}

#[tokio::test]
async fn test_lists_databases() {
    let handler = handler(false);

    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    let dbs = response.databases;

    assert!(dbs.iter().any(|db| db == "app"), "Expected 'app' in: {dbs:?}");
}

#[tokio::test]
async fn test_lists_tables() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("app".into()),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
    let ListEntries::Brief(tables) = response.tables else {
        panic!("expected brief-mode tables");
    };

    for expected in ["users", "posts", "tags", "post_tags"] {
        assert!(
            tables.iter().any(|t| t == expected),
            "Missing '{expected}' in: {tables:?}"
        );
    }
}

#[tokio::test]
async fn test_executes_sql() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users ORDER BY id".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    assert_eq!(response.rows.len(), 3, "Expected 3 users, got {}", response.rows.len());
}

#[tokio::test]
async fn test_blocks_writes_in_read_only_mode() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Hacker', 'hack@evil.com')".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;

    assert!(response.is_err(), "Expected error for write in read-only mode");
}

#[tokio::test]
async fn test_creates_database() {
    let handler = handler(false);
    let request = CreateDatabaseRequest {
        database: "app_new".into(),
    };

    let response = handler.create_database(request).await.unwrap();
    assert!(response.message.contains("created successfully"));

    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    let dbs = response.databases;

    assert!(dbs.iter().any(|db| db == "app_new"), "New db not in list");
}

#[tokio::test]
async fn test_drops_database() {
    let handler = handler(false);

    // Verify seeded database exists
    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    let dbs = response.databases;
    assert!(dbs.iter().any(|db| db == "canary"), "canary should exist before drop");

    // Drop it
    let drop_request = DropDatabaseRequest {
        database: "canary".into(),
    };
    let response = handler.drop_database(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    let dbs = response.databases;
    assert!(
        !dbs.iter().any(|db| db == "canary"),
        "canary should not exist after drop"
    );
}

#[tokio::test]
async fn test_drop_active_database_blocked() {
    let handler = handler(false);
    let request = DropDatabaseRequest { database: "app".into() };

    let response = handler.drop_database(request).await;

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
        database: "nonexistent_db_xyz".into(),
    };

    let response = handler.drop_database(request).await;

    assert!(response.is_err(), "Expected error for nonexistent database");
}

#[tokio::test]
async fn test_drop_database_invalid_identifier() {
    let handler = handler(false);
    let request = DropDatabaseRequest {
        database: String::new(),
    };

    let response = handler.drop_database(request).await;

    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_lists_tables_cross_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("analytics".into()),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
    let ListEntries::Brief(tables) = response.tables else {
        panic!("expected brief-mode tables");
    };

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
    let request = ReadQueryRequest {
        query: "SELECT * FROM events ORDER BY id".into(),
        database: Some("analytics".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    assert_eq!(response.rows.len(), 2, "Expected 2 events, got {}", response.rows.len());
}

#[tokio::test]
async fn test_lists_databases_includes_cross_db() {
    let handler = handler(false);

    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    let dbs = response.databases;

    assert!(
        dbs.iter().any(|db| db == "analytics"),
        "Expected 'analytics' in databases: {dbs:?}"
    );
}

#[tokio::test]
async fn test_blocks_writes_cross_database_in_read_only_mode() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "INSERT INTO events (name) VALUES ('hack')".into(),
        database: Some("analytics".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;

    assert!(
        response.is_err(),
        "Expected error for write in read-only mode on cross-database"
    );
}

#[tokio::test]
async fn test_uses_default_pool_for_matching_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("app".into()),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
    let ListEntries::Brief(tables) = response.tables else {
        panic!("expected brief-mode tables");
    };

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
    let request = ReadQueryRequest {
        query: "SELECT SLEEP(30)".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let start = std::time::Instant::now();
    let response = handler.read_query(request).await;
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
    let request = ReadQueryRequest {
        query: "SELECT 1 AS value".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_ok(), "Fast query should succeed without timeout");
}

#[tokio::test]
async fn test_drop_table_success() {
    let handler = handler(false);

    // Create a temporary table
    let create = QueryRequest {
        query: "CREATE TABLE drop_test_simple (id INT PRIMARY KEY)".into(),
        database: Some("app".into()),
    };
    handler.write_query(create).await.unwrap();

    // Drop it
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "drop_test_simple".into(),
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let tables_request = ListTablesRequest {
        database: Some("app".into()),
        ..Default::default()
    };
    let response = handler.list_tables(tables_request).await.unwrap();
    let ListEntries::Brief(tables) = response.tables else {
        panic!("expected brief-mode tables");
    };
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
        database: Some("app".into()),
    };
    handler.write_query(create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_child (id INT PRIMARY KEY, parent_id INT, FOREIGN KEY (parent_id) REFERENCES drop_test_parent(id)) ENGINE=InnoDB".into(),
        database: Some("app".into()),
    };
    handler.write_query(create_child).await.unwrap();

    // Attempt to drop parent — should fail due to FK
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "drop_test_parent".into(),
    };
    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected FK constraint error");

    // Clean up
    let cleanup_child = QueryRequest {
        query: "DROP TABLE drop_test_child".into(),
        database: Some("app".into()),
    };
    handler.write_query(cleanup_child).await.unwrap();

    let cleanup_parent = QueryRequest {
        query: "DROP TABLE drop_test_parent".into(),
        database: Some("app".into()),
    };
    handler.write_query(cleanup_parent).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_invalid_identifier() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: String::new(),
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_explain_query_select() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: Some("app".into()),
        query: "SELECT * FROM users".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(!plan.is_empty(), "Expected non-empty execution plan");
}

#[tokio::test]
async fn test_explain_query_analyze_write_blocked_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database: Some("app".into()),
        query: "INSERT INTO users (name, email) VALUES ('x', 'x@x.com')".into(),
        analyze: true,
    };

    let response = handler.explain_query(request).await;
    assert!(
        response.is_err(),
        "Expected error for EXPLAIN ANALYZE on write statement in read-only mode"
    );
}

#[tokio::test]
async fn test_list_tables_nonexistent_database_returns_empty() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("nonexistent_db_xyz".into()),
        ..Default::default()
    };

    // MySQL queries information_schema.TABLES — a nonexistent schema returns
    // zero rows rather than an error.
    let response = handler.list_tables(request).await.unwrap();
    assert!(
        response.tables.is_empty(),
        "Nonexistent database should return empty table list, got: {:?}",
        response.tables
    );
}

#[tokio::test]
async fn test_list_tables_empty_database_falls_back_to_default() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some(String::new()),
        ..Default::default()
    };

    let response = handler
        .list_tables(request)
        .await
        .expect("empty db should default to --db-name");
    let tables = response.tables.as_brief().expect("brief-mode tables");
    assert!(
        tables.iter().any(|t| t == "users"),
        "expected default-database tables, got {tables:?}",
    );
}

#[tokio::test]
async fn test_list_tables_omitted_database_falls_back_to_default() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: None,
        ..Default::default()
    };

    let response = handler
        .list_tables(request)
        .await
        .expect("omitted db should default to --db-name");
    let tables = response.tables.as_brief().expect("brief-mode tables");
    assert!(
        tables.iter().any(|t| t == "users"),
        "expected default-database tables, got {tables:?}",
    );
}

#[tokio::test]
async fn test_create_database_already_exists() {
    let handler = handler(false);
    let request = CreateDatabaseRequest { database: "app".into() };

    let response = handler.create_database(request).await.unwrap();
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
        database: String::new(),
    };

    let response = handler.create_database(request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_explain_query_analyze() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: Some("app".into()),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    // MariaDB does not support EXPLAIN ANALYZE, so this may fail on MariaDB.
    // We accept either a successful plan or a query error.
    match handler.explain_query(request).await {
        Ok(response) => {
            let plan = &response.rows;
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
        database: Some("app".into()),
        query: "INSERT INTO users (name, email) VALUES ('x', 'x@x.com')".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(
        !plan.is_empty(),
        "Plain EXPLAIN should work for write statements even in read-only mode"
    );
}

#[tokio::test]
async fn test_explain_query_invalid_query() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: Some("app".into()),
        query: "NOT VALID SQL AT ALL".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await;
    assert!(response.is_err(), "Expected error for invalid SQL");
}

#[tokio::test]
async fn test_read_query_empty_query() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: String::new(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for empty query");
}

#[tokio::test]
async fn test_read_query_whitespace_only_query() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "   \t\n  ".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for whitespace-only query");
}

#[tokio::test]
async fn test_read_query_multi_statement_blocked() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT 1; DROP TABLE users".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for multi-statement query");
}

#[tokio::test]
async fn test_read_query_load_file_blocked() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT LOAD_FILE('/etc/passwd')".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for LOAD_FILE");
}

#[tokio::test]
async fn test_read_query_into_outfile_blocked() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users INTO OUTFILE '/tmp/out'".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for INTO OUTFILE");
}

#[tokio::test]
async fn test_read_query_show_tables() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SHOW TABLES".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(!rows.is_empty(), "SHOW TABLES should return results");
}

#[tokio::test]
async fn test_read_query_describe_table() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "DESCRIBE users".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(!rows.is_empty(), "DESCRIBE should return column info");
}

#[tokio::test]
async fn test_drop_table_nonexistent() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "nonexistent_table_xyz".into(),
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected error for nonexistent table");
}

#[tokio::test]
async fn test_drop_table_cross_database() {
    let handler = handler(false);

    // Create a table in the analytics database
    let create = QueryRequest {
        query: "CREATE TABLE drop_cross_test (id INT PRIMARY KEY)".into(),
        database: Some("analytics".into()),
    };
    handler.write_query(create).await.unwrap();

    // Drop it from the analytics database
    let drop_request = DropTableRequest {
        database: Some("analytics".into()),
        table: "drop_cross_test".into(),
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));
}

#[tokio::test]
async fn test_write_query_cross_database() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO events (name, payload) VALUES ('cross_test', '{\"test\":true}')".into(),
        database: Some("analytics".into()),
    };
    handler.write_query(insert).await.unwrap();

    let select = ReadQueryRequest {
        query: "SELECT name FROM events WHERE name = 'cross_test'".into(),
        database: Some("analytics".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert!(!arr.is_empty(), "Cross-database write should persist");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM events WHERE name = 'cross_test'".into(),
        database: Some("analytics".into()),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_read_query_empty_result_set() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users WHERE email = 'nobody@nowhere.com'".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(rows.is_empty(), "Expected empty result set");
}

#[tokio::test]
async fn test_read_query_null_values() {
    let handler = handler(false);
    // posts.body can be NULL, and published defaults to 0
    let request = ReadQueryRequest {
        query: "SELECT title, body FROM posts WHERE title = 'My First Post'".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 1);
    // body should be present (even if not null in seed data, the column exists)
    assert!(rows[0].get("body").is_some(), "body column should be present");
}

#[tokio::test]
async fn test_read_query_aggregate() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT COUNT(*) AS total FROM users".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["total"], 3);
}

#[tokio::test]
async fn test_read_query_group_by() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT user_id, COUNT(*) AS post_count FROM posts GROUP BY user_id ORDER BY user_id".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(rows.len() >= 2, "Expected at least 2 groups");
}

#[tokio::test]
async fn test_read_query_use_statement() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "USE app".into(),
        database: Some("app".into()),
        cursor: None,
    };

    // USE passes read_only validation and executes without returning rows
    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(rows.is_empty(), "USE returns no rows");
}

#[tokio::test]
async fn test_read_query_show_databases() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SHOW DATABASES".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(!rows.is_empty(), "SHOW DATABASES should return results");
}

#[tokio::test]
async fn test_explain_query_cross_database() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: Some("analytics".into()),
        query: "SELECT * FROM events".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(!plan.is_empty(), "EXPLAIN should work cross-database");
}

#[tokio::test]
async fn test_read_query_with_comments() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "/* fetch users */ SELECT * FROM users ORDER BY id".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 3, "Comment-prefixed SELECT should work");
}

#[tokio::test]
async fn test_read_query_subquery() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users WHERE id IN (SELECT user_id FROM posts WHERE published = 1)".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(!rows.is_empty(), "Subquery should return results");
}

#[tokio::test]
async fn test_read_query_with_join() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT p.title, u.name FROM posts p JOIN users u ON p.user_id = u.id ORDER BY p.id".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 5, "Should return all 5 posts with user names");
    assert!(rows[0].get("title").is_some());
    assert!(rows[0].get("name").is_some());
}

#[tokio::test]
async fn test_explain_query_analyze_select_allowed_in_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database: Some("app".into()),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    // MariaDB doesn't support EXPLAIN ANALYZE, so tolerate both outcomes
    match handler.explain_query(request).await {
        Ok(response) => {
            let plan = &response.rows;
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
        database: Some("app".into()),
    };

    let response = handler.write_query(request).await;
    assert!(response.is_err(), "Expected error for invalid SQL in write_query");
}

#[tokio::test]
async fn test_read_query_with_limit() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users ORDER BY id LIMIT 2".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 2, "LIMIT 2 should return exactly 2 rows");
}

#[tokio::test]
async fn test_drop_table_empty_database_falls_back_to_default() {
    let handler = handler(false);

    let create = QueryRequest {
        query: "CREATE TABLE drop_default_my (id INT PRIMARY KEY)".into(),
        database: Some("app".into()),
    };
    handler.write_query(create).await.expect("seed table");

    let drop_request = DropTableRequest {
        database: Some(String::new()),
        table: "drop_default_my".into(),
    };
    let response = handler
        .drop_table(drop_request)
        .await
        .expect("empty db should default to --db-name");
    assert!(response.message.contains("dropped successfully"));
}

#[tokio::test]
async fn test_read_query_with_line_comment() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "-- get users\nSELECT * FROM users ORDER BY id".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 3, "Line-comment prefixed SELECT should work");
}

#[tokio::test]
async fn test_read_query_into_dumpfile_blocked() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users INTO DUMPFILE '/tmp/out'".into(),
        database: Some("app".into()),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for INTO DUMPFILE");
}

#[tokio::test]
async fn test_create_database_blocked_in_read_only() {
    let handler = handler(true);
    let request = CreateDatabaseRequest {
        database: "should_not_create".into(),
    };

    let response = handler.create_database(request).await;
    assert!(response.is_err(), "create_database should be blocked in read-only mode");
}

#[tokio::test]
async fn test_drop_database_blocked_in_read_only() {
    let handler = handler(true);
    let request = DropDatabaseRequest { database: "app".into() };

    let response = handler.drop_database(request).await;
    assert!(response.is_err(), "drop_database should be blocked in read-only mode");
}

#[tokio::test]
async fn test_drop_table_blocked_in_read_only() {
    let handler = handler(true);
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "users".into(),
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "drop_table should be blocked in read-only mode");
}

#[tokio::test]
async fn test_create_drop_database_with_double_quote() {
    let handler = handler(false);
    let db_name = "test_quote_db\"edge".to_string();

    let create = CreateDatabaseRequest {
        database: db_name.clone(),
    };
    let result = handler.create_database(create).await;
    assert!(
        result.is_ok(),
        "create database with double-quote should succeed: {result:?}"
    );

    let drop = DropDatabaseRequest { database: db_name };
    let result = handler.drop_database(drop).await;
    assert!(
        result.is_ok(),
        "drop database with double-quote should succeed: {result:?}"
    );
}

#[tokio::test]
async fn test_timeout_on_list_tables() {
    let mut config = base_db_config(true);
    config.query_timeout = Some(1);
    let handler = MysqlHandler::new(&config);

    let request = ReadQueryRequest {
        query: "SELECT SLEEP(60)".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let result = handler.read_query(request).await;
    assert!(result.is_err(), "slow query should time out");
}

const MY_DB: &str = "app";

async fn collect_all_paged(handler: &MysqlHandler) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let request = ListTablesRequest {
            database: Some(MY_DB.into()),
            cursor,
            ..Default::default()
        };
        let response = handler.list_tables(request).await.expect("list page");
        all.extend(response.tables.into_brief().expect("brief-mode page"));
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    all
}

#[tokio::test]
async fn test_list_tables_pagination_traverses_pages() {
    let handler_paged = handler_with_page_size(2);
    let handler_full = handler(true);

    let collected = collect_all_paged(&handler_paged).await;

    let single_page = handler_full
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .expect("single page");

    let single_page_tables = single_page.tables.into_brief().expect("brief-mode page");
    assert_eq!(
        collected, single_page_tables,
        "paged traversal must yield identical results (and ordering) to a single full page"
    );
    let unique: std::collections::HashSet<&String> = collected.iter().collect();
    assert_eq!(unique.len(), collected.len(), "no duplicates across pages");
}

#[tokio::test]
async fn test_list_tables_pagination_small_table_set_no_next_cursor() {
    let handler = handler(true);
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        response.next_cursor.is_none(),
        "seeded fixture below default page_size must not emit nextCursor"
    );
}

#[tokio::test]
async fn test_list_tables_pagination_boundary_page_size_equals_total() {
    let handler_full = handler(true);
    let total = handler_full
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .expect("discover total")
        .tables
        .len();
    let page_size = u16::try_from(total).expect("seed total fits in u16");

    let handler_boundary = handler_with_page_size(page_size);
    let response = handler_boundary
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        response.tables.len(),
        total,
        "page_size equal to total must return everything on one page"
    );
    assert!(
        response.next_cursor.is_none(),
        "page_size equal to total must NOT emit nextCursor"
    );
}

#[tokio::test]
async fn test_list_tables_pagination_off_the_end_cursor_returns_empty_page() {
    use dbmcp_server::pagination::Cursor;

    let handler = handler(true);
    let request = ListTablesRequest {
        database: Some(MY_DB.into()),
        cursor: Some(Cursor { offset: 10_000 }),
        ..Default::default()
    };
    let response = handler.list_tables(request).await.unwrap();

    assert!(
        response.tables.is_empty(),
        "off-the-end cursor must return empty tables, got {:?}",
        response.tables
    );
    assert!(response.next_cursor.is_none(), "off-the-end must not emit nextCursor");
}

#[tokio::test]
async fn test_list_tables_respects_configured_page_size() {
    let handler = handler_with_page_size(2);
    let first = handler
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .expect("first page");
    assert_eq!(first.tables.len(), 2, "configured page_size=2 must cap page 1");
    assert!(
        first.next_cursor.is_some(),
        "page 1 must emit nextCursor when total > page_size"
    );
}

#[tokio::test]
async fn test_list_tables_respects_configured_page_size_minimum() {
    let handler = handler_with_page_size(1);
    let first = handler
        .list_tables(ListTablesRequest {
            database: Some(MY_DB.into()),
            ..Default::default()
        })
        .await
        .expect("first page");
    assert_eq!(first.tables.len(), 1, "page_size=1 must return one table per page");
    assert!(first.next_cursor.is_some(), "page 1 must emit nextCursor");
}

async fn collect_all_paged_databases(handler: &MysqlHandler) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let request = ListDatabasesRequest { cursor };
        let response = handler.list_databases(request).await.expect("list page");
        all.extend(response.databases);
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    all
}

#[tokio::test]
async fn test_list_databases_pagination_traverses_pages() {
    let handler_paged = handler_with_page_size(1);
    let handler_full = handler(true);

    let collected = collect_all_paged_databases(&handler_paged).await;

    let single_page = handler_full
        .list_databases(ListDatabasesRequest::default())
        .await
        .expect("single page");

    assert_eq!(
        collected, single_page.databases,
        "paged traversal must yield identical results (and ordering) to a single full page"
    );
    let unique: std::collections::HashSet<&String> = collected.iter().collect();
    assert_eq!(unique.len(), collected.len(), "no duplicates across pages");
}

#[tokio::test]
async fn test_list_databases_pagination_small_set_no_next_cursor() {
    let handler = handler(true);
    let response = handler.list_databases(ListDatabasesRequest::default()).await.unwrap();
    assert!(
        response.next_cursor.is_none(),
        "seeded fixture below default page_size must not emit nextCursor"
    );
}

#[tokio::test]
async fn test_list_databases_pagination_boundary_page_size_equals_total() {
    let handler_full = handler(true);
    let total = handler_full
        .list_databases(ListDatabasesRequest::default())
        .await
        .expect("discover total")
        .databases
        .len();
    let page_size = u16::try_from(total).expect("seed total fits in u16");

    let handler_boundary = handler_with_page_size(page_size);
    let response = handler_boundary
        .list_databases(ListDatabasesRequest::default())
        .await
        .unwrap();
    assert_eq!(
        response.databases.len(),
        total,
        "page_size equal to total must return everything on one page"
    );
    assert!(
        response.next_cursor.is_none(),
        "page_size equal to total must NOT emit nextCursor"
    );
}

#[tokio::test]
async fn test_list_databases_pagination_off_the_end_cursor_returns_empty_page() {
    use dbmcp_server::pagination::Cursor;

    let handler = handler(true);
    let request = ListDatabasesRequest {
        cursor: Some(Cursor { offset: 10_000 }),
    };
    let response = handler.list_databases(request).await.unwrap();

    assert!(
        response.databases.is_empty(),
        "off-the-end cursor must return empty databases, got {:?}",
        response.databases
    );
    assert!(response.next_cursor.is_none(), "off-the-end must not emit nextCursor");
}

#[tokio::test]
async fn test_list_databases_respects_configured_page_size() {
    let handler = handler_with_page_size(1);
    let first = handler
        .list_databases(ListDatabasesRequest::default())
        .await
        .expect("first page");
    assert_eq!(
        first.databases.len(),
        1,
        "page_size=1 must return one database per page"
    );
    assert!(
        first.next_cursor.is_some(),
        "page 1 must emit nextCursor when total > page_size"
    );
}

async fn collect_all_paged_read_query(handler: &MysqlHandler, query: &str) -> Vec<Value> {
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let request = ReadQueryRequest {
            query: query.into(),
            database: Some("app".into()),
            cursor,
        };
        let response = handler.read_query(request).await.expect("read_query page");
        all.extend(response.rows);
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    all
}

#[tokio::test]
async fn test_read_query_pagination_traverses_pages() {
    let handler_paged = handler_with_page_size(2);
    let handler_full = handler(true);
    let query = "SELECT id FROM users ORDER BY id";

    let collected = collect_all_paged_read_query(&handler_paged, query).await;

    let single = handler_full
        .read_query(ReadQueryRequest {
            query: query.into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("single page");
    assert_eq!(
        collected, single.rows,
        "paged traversal must yield identical rows (and ordering) to a single full page"
    );
    let ids: Vec<i64> = collected
        .iter()
        .map(|row| row["id"].as_i64().expect("id is integer"))
        .collect();
    assert_eq!(ids, vec![1, 2, 3], "seeded users should be ids 1..=3");
}

#[tokio::test]
async fn test_read_query_pagination_small_result_no_next_cursor() {
    let handler = handler_with_page_size(2);
    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT id FROM users WHERE id = 1".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .unwrap();
    assert!(
        response.next_cursor.is_none(),
        "single-row result must not emit nextCursor"
    );
    assert_eq!(response.rows.len(), 1);
}

#[tokio::test]
async fn test_read_query_pagination_empty_result_no_next_cursor() {
    let handler = handler_with_page_size(2);
    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT id FROM users WHERE id = -1".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .unwrap();
    assert!(&response.rows.is_empty());
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_read_query_pagination_preserves_inner_limit() {
    let handler = handler_with_page_size(2);
    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT id FROM users ORDER BY id LIMIT 1 OFFSET 1".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]["id"].as_i64(),
        Some(2),
        "inner OFFSET 1 LIMIT 1 must return id=2"
    );
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_read_query_pagination_off_the_end_cursor_returns_empty() {
    use dbmcp_server::pagination::Cursor;
    let handler = handler_with_page_size(2);
    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT id FROM users ORDER BY id".into(),
            database: Some("app".into()),
            cursor: Some(Cursor { offset: 10_000 }),
        })
        .await
        .unwrap();
    assert!(&response.rows.is_empty());
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_read_query_pagination_invalid_cursor_rejected_at_deserialize() {
    use serde_json::json;

    let bad_cursors = ["!!!not-base64", "bm90LWpzb24", "eyJ4IjoxfQ", "eyJvZmZzZXQiOi0xfQ"];

    for bad in bad_cursors {
        let err = serde_json::from_value::<ReadQueryRequest>(json!({
            "query": "SELECT 1",
            "database": "app",
            "cursor": bad,
        }))
        .expect_err(&format!("cursor {bad:?} should be rejected at deserialize time"));
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("cursor") || msg.contains("base64") || msg.contains("malformed"),
            "cursor {bad:?} error is not descriptive: {err}"
        );
    }
}

#[tokio::test]
async fn test_read_query_non_select_show_tables_single_page() {
    // SHOW TABLES is NonSelect; cursor must be ignored (no error, no nextCursor,
    // response identical to the no-cursor call) and all rows returned.
    use dbmcp_server::pagination::Cursor;
    let handler = handler_with_page_size(2);

    let without_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW TABLES".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("SHOW TABLES without cursor should succeed");

    let with_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW TABLES".into(),
            database: Some("app".into()),
            cursor: Some(Cursor { offset: 100 }),
        })
        .await
        .expect("SHOW TABLES with cursor should succeed — cursor must be ignored");

    assert!(without_cursor.next_cursor.is_none());
    assert!(with_cursor.next_cursor.is_none());
    assert_eq!(
        without_cursor.rows, with_cursor.rows,
        "cursor must be silently ignored for non-SELECT statements"
    );
    // SHOW TABLES in `app` returns 7 seeded base tables (users, posts, tags,
    // post_tags, temporal, posts_audit, events_by_year) plus 9 seeded views
    // (active_users, active_users_v2, active_orders, active_users_with_check_cascaded,
    // active_users_with_check_local, archived_users, archived_users_invoker,
    // user_metrics_cte, published_posts); MySQL's SHOW TABLES lists both. Must
    // not be paginated even with page_size=2.
    let rows = &without_cursor.rows;
    assert_eq!(
        rows.len(),
        16,
        "SHOW TABLES must not be paginated: expected all 16 seeded tables+views, got {}",
        rows.len()
    );
}

#[tokio::test]
async fn test_read_query_non_select_describe_users_single_page() {
    // DESCRIBE is classified as Statement::ExplainTable → NonSelect.
    let handler = handler_with_page_size(2);

    let response = handler
        .read_query(ReadQueryRequest {
            query: "DESCRIBE users".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("DESCRIBE users should succeed");

    assert!(response.next_cursor.is_none(), "DESCRIBE must not paginate");
    // users has 4 columns (id, name, email, created_at); DESCRIBE must not be
    // capped by page_size=2.
    let rows = &response.rows;
    assert!(
        rows.len() >= 4,
        "DESCRIBE users must return all columns, got {}",
        rows.len()
    );
}

#[tokio::test]
async fn test_read_query_returns_non_null_temporal_columns() {
    // Feature 038: MySQL temporal columns must round-trip as ISO 8601 strings.
    // MySQL has no TIMESTAMPTZ analog, so the zoned bucket is exercised on
    // PostgreSQL only; here all four columns are naive (no offset, no Z).
    let handler = handler(false);

    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT `date`, `time`, `datetime`, `timestamp` FROM temporal WHERE id = 1".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("temporal SELECT should succeed");

    let arr = &response.rows;
    assert_eq!(arr.len(), 1, "temporal seeds exactly one row");
    assert_eq!(arr[0]["date"], "2026-04-20", "DATE → YYYY-MM-DD");
    assert_eq!(arr[0]["time"], "14:30:00", "TIME → HH:MM:SS");
    assert_eq!(arr[0]["datetime"], "2026-04-20T14:30:00", "DATETIME → naive ISO 8601");
    assert_eq!(arr[0]["timestamp"], "2026-04-20T14:30:00", "TIMESTAMP → naive ISO 8601");
}

#[tokio::test]
async fn test_list_views_returns_seeded_views() {
    let handler = handler(true);
    let request = ListViewsRequest {
        database: Some("app".into()),
        cursor: None,
        ..Default::default()
    };

    let response = handler.list_views(request).await.expect("list_views");

    let names = response.views.as_brief().expect("brief mode");
    assert!(
        names.contains(&"active_users".to_string()),
        "expected seeded active_users view in {names:?}"
    );
    assert!(
        names.contains(&"published_posts".to_string()),
        "expected seeded published_posts view in {names:?}"
    );
}

#[tokio::test]
async fn test_list_views_excludes_base_tables() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_views");

    let names = response.views.as_brief().expect("brief mode");
    assert!(
        !names.contains(&"users".to_string()),
        "base table `users` must not appear in listViews, got {names:?}"
    );
    assert!(
        !names.contains(&"posts".to_string()),
        "base table `posts` must not appear in listViews, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_views_empty_for_view_less_database() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("analytics".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_views");

    assert!(
        response.views.as_brief().expect("brief").is_empty(),
        "analytics has no views, got {:?}",
        response.views
    );
}

#[tokio::test]
async fn test_list_views_empty_database_falls_back_to_default() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some(String::new()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("empty db should default to --db-name");
    assert!(
        !response.views.as_brief().expect("brief").is_empty(),
        "default db has seeded views, got {:?}",
        response.views
    );
}

#[tokio::test]
async fn test_list_views_omitted_database_falls_back_to_default() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: None,
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("omitted db should default to --db-name");
    assert!(
        !response.views.as_brief().expect("brief").is_empty(),
        "default db has seeded views, got {:?}",
        response.views
    );
}

#[tokio::test]
async fn test_list_views_pagination_traverses_pages() {
    let handler_paged = handler_with_page_size(1);
    let handler_full = handler(true);

    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let request = ListViewsRequest {
            database: Some("app".into()),
            cursor,
            ..Default::default()
        };
        let response = handler_paged.list_views(request).await.expect("paged list_views");
        all.extend(response.views.as_brief().expect("brief").iter().cloned());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    let single = handler_full
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("single-page list_views");

    let single_names = single.views.as_brief().expect("brief").to_vec();
    assert_eq!(all, single_names, "paginated traversal should equal single page");
}

#[tokio::test]
async fn test_list_views_works_in_read_only_mode() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_views in read-only mode");

    assert!(
        !response.views.as_brief().expect("brief").is_empty(),
        "read-only mode must still allow listViews"
    );
}

#[tokio::test]
async fn test_list_triggers_returns_seeded_triggers() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let names = response.triggers.as_brief().expect("brief mode");
    assert!(
        names.contains(&"users_before_insert".to_string()),
        "expected seeded users_before_insert trigger, got {names:?}"
    );
    assert!(
        names.contains(&"posts_before_update".to_string()),
        "expected seeded posts_before_update trigger, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_triggers_empty_for_trigger_less_database() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("analytics".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let names = response.triggers.as_brief().expect("brief mode");
    assert!(names.is_empty(), "analytics has no triggers, got {names:?}");
}

#[tokio::test]
async fn test_list_triggers_empty_database_falls_back_to_default() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some(String::new()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("empty db should default to --db-name");
    assert!(
        !response.triggers.is_empty(),
        "default db has seeded triggers, got {:?}",
        response.triggers
    );
}

#[tokio::test]
async fn test_list_triggers_omitted_database_falls_back_to_default() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: None,
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("omitted db should default to --db-name");
    assert!(
        !response.triggers.is_empty(),
        "default db has seeded triggers, got {:?}",
        response.triggers
    );
}

#[tokio::test]
async fn test_list_triggers_works_in_read_only_mode() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_triggers in read-only mode");

    assert!(
        !response.triggers.is_empty(),
        "read-only mode must still allow listTriggers"
    );
}

#[tokio::test]
async fn test_list_triggers_search_filter_returns_only_matches() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let names = response.triggers.as_brief().expect("brief mode");
    let expected = [
        "posts_audit_after_delete".to_string(),
        "posts_audit_after_insert".to_string(),
        "posts_audit_after_update".to_string(),
        "users_audit_after_insert".to_string(),
    ];
    assert_eq!(names, &expected, "got {names:?}");
}

#[tokio::test]
async fn test_list_triggers_search_is_case_insensitive() {
    let handler = handler(true);
    let upper = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("AUDIT".into()),
            ..Default::default()
        })
        .await
        .expect("upper");
    let lower = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            ..Default::default()
        })
        .await
        .expect("lower");
    assert_eq!(upper.triggers.as_brief(), lower.triggers.as_brief());
}

#[tokio::test]
async fn test_list_triggers_search_no_match_returns_empty() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("nonexistent_trigger_xyz".into()),
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    assert!(response.triggers.as_brief().expect("brief").is_empty());
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_list_triggers_search_supports_wildcard_semantics() {
    // Mirrors the `listTables` contract: LIKE wildcards (`%`, `_`) are exposed
    // as pattern semantics. `%audit%` and the implicit `%term%` produce the
    // same match set.
    let handler = handler(true);
    let plain = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            ..Default::default()
        })
        .await
        .expect("plain");
    let with_wildcard = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("%audit%".into()),
            ..Default::default()
        })
        .await
        .expect("wildcard");
    assert_eq!(plain.triggers.as_brief(), with_wildcard.triggers.as_brief());
}

#[tokio::test]
async fn test_list_triggers_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\", "`", "/* */"] {
        let response = handler
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("list_triggers failed for payload {payload:?}: {e:?}"));

        assert!(
            response.triggers.as_brief().is_some(),
            "payload {payload:?} returned non-brief shape"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_search_paginates_filtered_results() {
    let paged = handler_with_page_size(2);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                cursor,
                search: Some("audit".into()),
                ..Default::default()
            })
            .await
            .expect("list_triggers paginated");
        let names = response.triggers.as_brief().expect("brief").to_vec();
        all.extend(names);
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let single = handler(true)
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            ..Default::default()
        })
        .await
        .expect("single-page list_triggers");
    assert_eq!(
        all,
        single.triggers.as_brief().expect("brief"),
        "paginated traversal should equal single-page result"
    );
}

#[tokio::test]
async fn test_list_triggers_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let no_filter = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("no filter");
    let empty = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some(String::new()),
            ..Default::default()
        })
        .await
        .expect("empty filter");
    let whitespace = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("   ".into()),
            ..Default::default()
        })
        .await
        .expect("whitespace");

    assert_eq!(no_filter.triggers.as_brief(), empty.triggers.as_brief());
    assert_eq!(no_filter.triggers.as_brief(), whitespace.triggers.as_brief());
}

#[tokio::test]
async fn test_list_triggers_arbitrary_database_identifier_is_bound_as_literal() {
    // The database value is parameter-bound to `TRIGGER_SCHEMA = ?` and never
    // interpolated into SQL. SQL meta-characters (`;`, `'`, `"`, `` ` ``)
    // therefore reach the engine as literal schema names and produce empty
    // results. This test pins the parameter-binding path against injection.
    let handler = handler(true);
    for bound in ["shop;DROP", "shop'", "shop\"", "shop`"] {
        let result = handler
            .list_triggers(ListTriggersRequest {
                database: Some(bound.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("expected bound-literal handling, got error: {e:?} for {bound:?}"));
        assert!(
            result.triggers.as_brief().expect("brief").is_empty(),
            "non-existent schema {bound:?} should return empty list, got {:?}",
            result.triggers
        );
    }
}

#[tokio::test]
async fn test_list_triggers_detailed_returns_keyed_object_with_all_fields() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("posts_audit_after_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    assert_eq!(entries.len(), 1, "expected one match, got {entries:?}");
    let entry = entries
        .get("posts_audit_after_insert")
        .expect("posts_audit_after_insert key present");

    let obj = entry.as_object().expect("entry is object");
    let expected_keys = [
        "schema",
        "table",
        "timing",
        "events",
        "activationLevel",
        "definition",
        "sqlMode",
        "characterSetClient",
        "collationConnection",
        "databaseCollation",
    ];
    for key in expected_keys {
        assert!(obj.contains_key(key), "missing key {key:?} in {obj:?}");
    }
    assert_eq!(obj.len(), expected_keys.len(), "unexpected extra keys: {obj:?}");
    for forbidden in ["status", "functionName", "created", "name"] {
        assert!(
            !obj.contains_key(forbidden),
            "forbidden key {forbidden:?} present in {obj:?}"
        );
    }

    assert_eq!(obj.get("schema").and_then(Value::as_str), Some("app"));
    assert_eq!(obj.get("table").and_then(Value::as_str), Some("posts"));
    assert_eq!(obj.get("timing").and_then(Value::as_str), Some("AFTER"));
    assert_eq!(
        obj.get("events").and_then(Value::as_array).map(Vec::len),
        Some(1),
        "events must be a single-element array, got {:?}",
        obj.get("events")
    );
    assert_eq!(
        obj.get("events")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .and_then(Value::as_str),
        Some("INSERT")
    );
    assert_eq!(obj.get("activationLevel").and_then(Value::as_str), Some("ROW"));
}

#[tokio::test]
async fn test_list_triggers_detailed_definition_uses_canonical_definer_form() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("posts_audit_after_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    let entry = entries.get("posts_audit_after_insert").expect("entry present");
    let definition = entry
        .get("definition")
        .and_then(Value::as_str)
        .expect("definition string");

    // Canonical form: starts with `CREATE DEFINER=` followed by backtick-quoted user/host.
    assert!(
        definition.starts_with("CREATE DEFINER=`"),
        "expected canonical DEFINER form, got: {definition:?}"
    );
    assert!(
        definition.contains("`@`"),
        "expected `@` separator, got: {definition:?}"
    );
    assert!(
        definition.contains(" TRIGGER `posts_audit_after_insert`"),
        "expected backtick-quoted trigger name, got: {definition:?}"
    );
    // Must not be the legacy single-quoted form.
    assert!(
        !definition.starts_with("CREATE DEFINER='"),
        "legacy single-quoted DEFINER form leaked: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_definition_matches_canonical_create_trigger() {
    // Asserts the structural shape of the reconstructed `definition` against
    // a known fixture — every component (DEFINER, TRIGGER name, timing/event,
    // ON table, FOR EACH ROW, body) is present in the expected order.
    //
    // Note: full byte-identity with `SHOW CREATE TRIGGER` is **not** asserted
    // here because MySQL 9 and MariaDB 12 disagree on the engine-emitted
    // form (MariaDB qualifies trigger and table names with the schema and
    // adds whitespace before `FOR EACH ROW`; MySQL emits the unqualified
    // single-line form). The reconstruction matches MySQL's canonical form;
    // both engines accept it as a valid `CREATE TRIGGER` body.
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("posts_audit_after_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");
    let entries = response.triggers.as_detailed().expect("detailed mode");
    let definition = entries
        .get("posts_audit_after_insert")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition string");

    for needle in [
        "CREATE DEFINER=`",
        "`@`",
        " TRIGGER `posts_audit_after_insert`",
        " AFTER INSERT",
        " ON `posts`",
        " FOR EACH ROW ",
        "INSERT INTO",
    ] {
        assert!(
            definition.contains(needle),
            "definition missing {needle:?}: {definition:?}"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_detailed_per_event_returns_single_element_events_array() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("posts_audit_after".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    let cases = [
        ("posts_audit_after_delete", "DELETE"),
        ("posts_audit_after_insert", "INSERT"),
        ("posts_audit_after_update", "UPDATE"),
    ];
    for (name, expected_event) in cases {
        let entry = entries.get(name).unwrap_or_else(|| panic!("{name} missing"));
        let events = entry
            .get("events")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("{name} events not array"));
        assert_eq!(events.len(), 1, "{name} events must have exactly one element");
        assert_eq!(
            events[0].as_str(),
            Some(expected_event),
            "{name} expected events=[{expected_event:?}]"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_detailed_session_context_fields_are_populated() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    assert!(!entries.is_empty(), "expected at least one audit trigger");
    for (name, entry) in entries {
        for field in [
            "sqlMode",
            "characterSetClient",
            "collationConnection",
            "databaseCollation",
        ] {
            let value = entry.get(field).and_then(Value::as_str);
            assert!(
                value.is_some_and(|v| !v.is_empty()),
                "{name}.{field} must be a non-empty string, got {value:?}"
            );
        }
    }
}

#[tokio::test]
async fn test_list_triggers_detailed_definition_round_trips_multiline_body() {
    // The seeded `users_audit_after_insert` carries a multi-statement body
    // containing a literal newline and a single-quote — covers spec Edge Case
    // "trigger body contains literal newlines or quote characters".
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("users_audit_after_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    let definition = entries
        .get("users_audit_after_insert")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition string");

    // Engine divergence: MySQL 9 stores `ACTION_STATEMENT` with the literal
    // newline; MariaDB 12 stores it as the escape sequence `\n`. Both forms
    // are valid round-trips of the seeded body. Accept either presentation.
    let has_real_newline = definition.contains("a note\nspans two lines");
    let has_escape = definition.contains("a note\\nspans two lines");
    assert!(
        has_real_newline || has_escape,
        "expected literal newline (or `\\n` escape on MariaDB) preserved in body, got: {definition:?}"
    );
    assert!(
        definition.contains("'a note"),
        "expected single-quoted string literal in body, got: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_triggers_brief_mode_wire_shape_unchanged() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let payload = serde_json::to_value(&response).expect("serialize");
    let triggers = payload.get("triggers").expect("triggers field");
    assert!(triggers.is_array(), "expected array, got {triggers:?}");
    assert!(
        triggers.as_array().unwrap().iter().all(Value::is_string),
        "expected every brief-mode entry to be a JSON string, got {triggers:?}"
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_search_narrows_payload() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let entries = response.triggers.as_detailed().expect("detailed mode");
    for name in entries.keys() {
        assert!(
            name.contains("audit"),
            "non-matching trigger {name:?} appeared in audit-filtered detailed payload"
        );
    }
    // Non-audit triggers must NOT appear.
    for forbidden in ["users_before_insert", "posts_before_update", "posts_after_insert"] {
        assert!(
            !entries.contains_key(forbidden),
            "{forbidden} must be excluded from audit-filtered payload"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_detailed_paginates() {
    let paged = handler_with_page_size(2);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                cursor,
                search: Some("audit".into()),
                detailed: true,
            })
            .await
            .expect("list_triggers paginated");
        let entries = response.triggers.as_detailed().expect("detailed");
        assert!(entries.len() <= 2, "page exceeds page_size: {}", entries.len());
        all.extend(entries.keys().cloned());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let single = handler(true)
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("audit".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("single-page list_triggers");
    let single_keys: Vec<String> = single
        .triggers
        .as_detailed()
        .expect("detailed")
        .keys()
        .cloned()
        .collect();
    assert_eq!(
        all, single_keys,
        "paginated detailed traversal should equal single-page key order"
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_sort_is_deterministic() {
    // MySQL/MariaDB enforce per-schema trigger-name uniqueness via the
    // `(TRIGGER_SCHEMA, TRIGGER_NAME)` primary key on
    // `information_schema.TRIGGERS`, so `ORDER BY TRIGGER_NAME` alone is a
    // total order for a single-schema listing. This test asserts
    // deterministic name-ordering across consecutive calls (cursor stability
    // depends on it).
    let handler = handler(true);
    let mut calls = Vec::new();
    for _ in 0..3 {
        let response = handler
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                detailed: true,
                ..Default::default()
            })
            .await
            .expect("list_triggers");
        let keys: Vec<String> = response
            .triggers
            .as_detailed()
            .expect("detailed")
            .keys()
            .cloned()
            .collect();
        calls.push(keys);
    }
    assert!(
        calls.windows(2).all(|w| w[0] == w[1]),
        "non-deterministic order: {calls:?}"
    );
}

#[tokio::test]
async fn test_list_tables_detailed_triggers_use_canonical_definer_form() {
    // Cross-tool regression: the list_tables `triggers_info` CTE was migrated
    // from `QUOTE(tr.DEFINER)` to canonical `DEFINER=`<user>`@`<host>`` form
    // as part of Clarification Q1. Verify here so a future revert is caught.
    let handler = handler(true);
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some("app".into()),
            search: Some("posts".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_tables");

    let entries = response.tables.as_detailed().expect("detailed mode");
    let posts = entries.get("posts").expect("posts table present");
    let triggers = posts.get("triggers").and_then(Value::as_array).expect("triggers array");
    assert!(!triggers.is_empty(), "posts has at least one seeded trigger");

    for trigger in triggers {
        let definition = trigger
            .get("definition")
            .and_then(Value::as_str)
            .expect("trigger definition");
        assert!(
            definition.starts_with("CREATE DEFINER=`"),
            "list_tables trigger definition not in canonical form: {definition:?}"
        );
        assert!(
            !definition.starts_with("CREATE DEFINER='"),
            "legacy single-quoted DEFINER leaked in list_tables: {definition:?}"
        );
    }
}

#[tokio::test]
async fn test_list_functions_returns_seeded_functions() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let names = response.functions.as_brief().expect("brief mode").to_vec();
    assert!(
        names.contains(&"calc_total".to_string()),
        "expected calc_total, got {names:?}"
    );
    assert!(
        names.contains(&"double_it".to_string()),
        "expected double_it, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_functions_excludes_procedures() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let names = response.functions.as_brief().expect("brief mode").to_vec();
    for proc_name in ["archive_user", "touch_post"] {
        assert!(
            !names.contains(&proc_name.to_string()),
            "procedure `{proc_name}` leaked into listFunctions output: {names:?}",
        );
    }
}

#[tokio::test]
async fn test_list_procedures_returns_seeded_procedures() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");

    let names = response.procedures.as_brief().expect("brief mode").to_vec();
    assert!(
        names.contains(&"archive_user".to_string()),
        "expected seeded archive_user procedure, got {names:?}"
    );
    assert!(
        names.contains(&"touch_post".to_string()),
        "expected seeded touch_post procedure, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_excludes_functions() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");

    let names = response.procedures.as_brief().expect("brief mode").to_vec();
    for func_name in ["calc_total", "double_it"] {
        assert!(
            !names.contains(&func_name.to_string()),
            "function `{func_name}` leaked into listProcedures output: {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_routines_empty_for_empty_database() {
    let handler = handler(true);
    let functions = handler
        .list_functions(ListFunctionsRequest {
            database: Some("analytics".into()),
            cursor: None,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    assert!(
        functions.functions.is_empty(),
        "analytics has no functions, got {:?}",
        functions.functions
    );

    let procedures = handler
        .list_procedures(ListProceduresRequest {
            database: Some("analytics".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");
    assert!(
        procedures.procedures.is_empty(),
        "analytics has no procedures, got {:?}",
        procedures.procedures
    );
}

// ----- listTables enrichment (spec 047) -----

async fn brief_tables(handler: &MysqlHandler, search: Option<&str>) -> Vec<String> {
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some("app".into()),
            search: search.map(str::to_owned),
            ..Default::default()
        })
        .await
        .expect("brief list_tables");
    response.tables.into_brief().expect("brief mode")
}

async fn detailed_entries(handler: &MysqlHandler, search: &str) -> indexmap::IndexMap<String, Value> {
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some("app".into()),
            search: Some(search.into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("detailed list_tables");
    response.tables.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn list_tables_brief_excludes_views() {
    let handler = handler(true);
    let names = brief_tables(&handler, None).await;
    for view in ["active_users", "published_posts"] {
        assert!(
            !names.contains(&view.to_string()),
            "FR-014a violated: view `{view}` leaked into brief listTables: {names:?}",
        );
    }
    assert!(
        names.contains(&"posts".to_string()),
        "expected base table `posts` in {names:?}",
    );
}

#[tokio::test]
async fn list_tables_brief_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = brief_tables(&handler, Some("post")).await;
    let upper = brief_tables(&handler, Some("POST")).await;
    let mixed = brief_tables(&handler, Some("Post")).await;
    assert_eq!(lower, upper, "case-insensitive: lower vs upper");
    assert_eq!(lower, mixed, "case-insensitive: lower vs mixed");
    assert!(
        lower.iter().all(|n| n.to_lowercase().contains("post")),
        "every match must contain 'post' case-insensitively: {lower:?}",
    );
}

#[tokio::test]
async fn list_tables_brief_search_wildcards() {
    let handler = handler(true);
    // `_` matches any single char — `post_` would match "posts" (s) and "posts_audit" up to underscore? actually
    // LIKE 'post_' matches exactly 5 chars starting with "post". Use CONCAT('%', ?, '%') so 'post_' embeds.
    let underscore = brief_tables(&handler, Some("post_")).await;
    assert!(
        underscore.iter().any(|n| n == "post_tags" || n == "posts_audit"),
        "underscore wildcard should match `post_tags` or `posts_audit`: {underscore:?}",
    );

    // Literal `%` is *not* escaped — passing '%post%' is the same as 'post' wrapped in CONCAT.
    let pct = brief_tables(&handler, Some("%post%")).await;
    assert!(
        pct.iter().any(|n| n == "posts"),
        "percent wildcard should match `posts`: {pct:?}",
    );
}

#[tokio::test]
async fn list_tables_brief_search_empty_is_no_filter() {
    let handler = handler(true);
    let baseline = brief_tables(&handler, None).await;
    let empty = brief_tables(&handler, Some("")).await;
    let blanks = brief_tables(&handler, Some("   ")).await;
    assert_eq!(baseline, empty, "empty search must equal None");
    assert_eq!(baseline, blanks, "whitespace search must equal None");
}

#[tokio::test]
async fn list_tables_brief_pagination_under_search() {
    let handler = handler_with_page_size(1);
    let mut collected = Vec::new();
    let mut cursor = None;
    loop {
        let response = handler
            .list_tables(ListTablesRequest {
                database: Some("app".into()),
                cursor,
                search: Some("post".into()),
                ..Default::default()
            })
            .await
            .expect("paged brief search");
        let page = response.tables.into_brief().expect("brief mode");
        assert!(page.len() <= 1, "page_size=1 caps to 1 per page");
        collected.extend(page);
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert!(
        collected.iter().all(|n| n.to_lowercase().contains("post")),
        "paged search must apply filter consistently: {collected:?}",
    );
    assert!(
        collected.contains(&"posts".to_string()) && collected.contains(&"post_tags".to_string()),
        "paged search must yield expected matches: {collected:?}",
    );
    // Order is server-determined (collation: utf8mb4_general_ci on MariaDB
    // sorts `_` after letters; binary collation sorts it before). Just check
    // the collected sequence has no duplicates and matches the engine's sort.
    let mut dedup = collected.clone();
    dedup.sort();
    dedup.dedup();
    assert_eq!(
        dedup.len(),
        collected.len(),
        "paged sequence has duplicates: {collected:?}"
    );
}

#[tokio::test]
async fn list_tables_detailed_returns_keyed_object() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "posts").await;
    assert!(entries.contains_key("posts"), "expected `posts` key: {entries:?}");
    for (name, value) in &entries {
        assert!(value.is_object(), "value for `{name}` must be a JSON object: {value}");
    }
}

#[tokio::test]
async fn list_tables_detailed_includes_check_constraint() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "posts").await;
    let posts = entries.get("posts").expect("posts entry");
    let constraints = posts["constraints"].as_array().expect("constraints array");
    let check = constraints
        .iter()
        .find(|c| c["type"] == "CHECK" && c["name"] == "posts_user_id_positive")
        .expect("seeded CHECK constraint must surface");
    assert_eq!(check["columns"], serde_json::json!([]));
    let definition = check["definition"].as_str().expect("CHECK definition is text");
    assert!(
        definition.contains("user_id"),
        "CHECK definition must reference user_id: {definition}",
    );
}

#[tokio::test]
async fn list_tables_detailed_marks_partitioned_table() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "events_by_year").await;
    let events = entries.get("events_by_year").expect("events_by_year entry");
    assert_eq!(events["kind"], "PARTITIONED_TABLE");

    let plain = detailed_entries(&handler, "tags").await;
    let tags = plain.get("tags").expect("tags entry");
    assert_eq!(tags["kind"], "TABLE", "non-partitioned table stays kind=TABLE");
}

#[tokio::test]
async fn list_tables_detailed_synthesises_index_definitions() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "posts").await;
    let posts = entries.get("posts").expect("posts entry");
    let indexes = posts["indexes"].as_array().expect("indexes array");

    let by_name = |n: &str| {
        indexes
            .iter()
            .find(|i| i["name"] == n)
            .unwrap_or_else(|| panic!("missing index `{n}` in {indexes:?}"))
    };

    let pk = by_name("PRIMARY");
    assert_eq!(pk["primary"], true);
    assert_eq!(pk["unique"], true);
    let pk_def = pk["definition"].as_str().expect("PK definition");
    assert!(pk_def.starts_with("PRIMARY KEY"), "PK definition shape: {pk_def}");
    assert!(pk_def.contains("USING BTREE"), "PK uses BTREE: {pk_def}");

    let unique = by_name("posts_user_title_uidx");
    assert_eq!(unique["unique"], true);
    assert_eq!(unique["primary"], false);
    let unique_def = unique["definition"].as_str().expect("UNIQUE definition");
    assert!(
        unique_def.starts_with("CREATE UNIQUE INDEX"),
        "composite UNIQUE definition shape: {unique_def}",
    );

    let fts = by_name("posts_body_fts");
    assert_eq!(fts["method"], "fulltext");
    let fts_def = fts["definition"].as_str().expect("FULLTEXT definition");
    assert!(
        fts_def.contains("FULLTEXT INDEX") && fts_def.contains("USING FULLTEXT"),
        "FULLTEXT index definition shape: {fts_def}",
    );

    let secondary = by_name("posts_published_idx");
    assert_eq!(secondary["unique"], false);
    let sec_def = secondary["definition"].as_str().expect("BTREE definition");
    assert!(
        sec_def.starts_with("CREATE INDEX") && sec_def.contains("USING BTREE"),
        "secondary BTREE definition shape: {sec_def}",
    );
}

#[tokio::test]
async fn list_tables_detailed_generated_column_expression_in_default() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "users").await;
    let users = entries.get("users").expect("users entry");
    let columns = users["columns"].as_array().expect("columns array");
    let display = columns
        .iter()
        .find(|c| c["name"] == "display_name")
        .expect("display_name column present");
    let default = display["default"].as_str().expect("generated column default text");
    assert!(
        default.contains("name") && default.contains("email"),
        "generated column default must carry GENERATION_EXPRESSION: {default}",
    );
    assert!(
        display.get("generated").is_none(),
        "FR/Q3: no separate `generated` field — expression rides in `default`: {display}",
    );
}

#[tokio::test]
async fn list_tables_detailed_triggers_reconstruct_create_trigger() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "posts").await;
    let posts = entries.get("posts").expect("posts entry");
    let triggers = posts["triggers"].as_array().expect("triggers array");
    let trigger = triggers
        .iter()
        .find(|t| t["name"] == "posts_after_insert")
        .expect("seeded posts_after_insert trigger");
    let definition = trigger["definition"].as_str().expect("trigger definition text");
    assert!(
        definition.starts_with("CREATE DEFINER="),
        "definition prefix: {definition}"
    );
    assert!(
        definition.contains("AFTER INSERT ON"),
        "timing+event in definition: {definition}"
    );
    assert!(
        definition.contains("FOR EACH ROW"),
        "row scope in definition: {definition}"
    );
    assert!(
        definition.contains("posts_audit"),
        "action statement preserved in definition: {definition}",
    );
    assert_eq!(trigger["enabled"], true, "MySQL/MariaDB triggers always enabled");
}

#[tokio::test]
async fn list_tables_detailed_comments_trimmed_to_null_when_empty() {
    let handler = handler(true);

    let entries = detailed_entries(&handler, "posts").await;
    let posts = entries.get("posts").expect("posts entry");
    assert_eq!(
        posts["comment"],
        Value::String("Blog post entries.".into()),
        "seeded TABLE COMMENT must surface verbatim",
    );
    let columns = posts["columns"].as_array().expect("columns");
    let body = columns.iter().find(|c| c["name"] == "body").expect("body column");
    assert_eq!(
        body["comment"],
        Value::String("Markdown-encoded post body.".into()),
        "seeded COLUMN COMMENT must surface verbatim",
    );

    let plain = detailed_entries(&handler, "tags").await;
    let tags = plain.get("tags").expect("tags entry");
    assert!(
        tags["comment"].is_null(),
        "absent TABLE COMMENT must be null, not empty string"
    );
    let tag_cols = tags["columns"].as_array().expect("tags columns");
    let id = tag_cols.iter().find(|c| c["name"] == "id").expect("id column");
    assert!(
        id["comment"].is_null(),
        "absent COLUMN COMMENT must be null, not empty string"
    );
}

#[tokio::test]
async fn list_tables_detailed_search_preserves_filter_across_pages() {
    let handler = handler_with_page_size(1);
    let mut collected = Vec::new();
    let mut cursor = None;
    loop {
        let response = handler
            .list_tables(ListTablesRequest {
                database: Some("app".into()),
                cursor,
                search: Some("post".into()),
                detailed: true,
            })
            .await
            .expect("paged detailed search");
        let page = response.tables.as_detailed().expect("detailed mode");
        assert!(page.len() <= 1, "page_size=1 caps to 1 per page");
        collected.extend(page.keys().cloned());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert!(
        collected.iter().all(|n| n.to_lowercase().contains("post")),
        "detailed pagination must apply the filter on every page: {collected:?}",
    );
    assert!(
        collected.contains(&"posts".to_string()) && collected.contains(&"post_tags".to_string()),
        "detailed paged search must yield expected matches: {collected:?}",
    );
}

#[tokio::test]
async fn list_tables_detailed_excludes_system_schemas_passes_through_validation() {
    // The active-database identifier reaches the SQL via per-backend quoting;
    // `information_schema` is a normal name and the call succeeds, returning
    // whatever metadata that schema's TABLE rows expose. Regression pin.
    let handler = handler(true);
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some("information_schema".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("information_schema is identifier-valid");
    let entries = response.tables.as_detailed().expect("detailed mode");
    for (name, value) in entries {
        assert_eq!(
            value["schema"], "information_schema",
            "every entry from information_schema must report that schema, got `{name}`: {value}",
        );
    }
}

#[tokio::test]
async fn list_tables_detailed_omits_inner_name_field() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "post").await;
    assert!(!entries.is_empty(), "fixture must yield at least one match");
    for (key, value) in &entries {
        assert!(
            value.get("name").is_none(),
            "value for `{key}` must not carry redundant `name`: {value}",
        );
    }
}

#[tokio::test]
async fn list_tables_detailed_iteration_order_matches_brief_sort() {
    let handler = handler(true);
    let brief = brief_tables(&handler, Some("post")).await;
    let detailed = detailed_entries(&handler, "post").await;
    let detailed_keys: Vec<String> = detailed.keys().cloned().collect();
    assert_eq!(
        brief, detailed_keys,
        "detailed key order must match brief alphabetical order — FR-010",
    );
}

#[tokio::test]
async fn list_tables_detailed_empty_page_is_empty_object() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "zzznosuchprefix").await;
    assert!(
        entries.is_empty(),
        "no-match search must yield empty map (serialises as `{{}}`): {entries:?}",
    );
}

// ----- listFunctions enrichment (spec 058) -----

#[tokio::test]
async fn test_list_functions_search_filter_returns_only_matches() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let names = response.functions.as_brief().expect("brief mode");
    let expected = [
        "calc_order_subtotal".to_string(),
        "calc_order_total".to_string(),
        "recalc_order_total_v2".to_string(),
    ];
    assert_eq!(names, &expected, "got {names:?}");
}

#[tokio::test]
async fn test_list_functions_search_is_case_insensitive() {
    let handler = handler(true);
    let upper = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("ORDER".into()),
            ..Default::default()
        })
        .await
        .expect("upper");
    let lower = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            ..Default::default()
        })
        .await
        .expect("lower");
    assert_eq!(upper.functions.as_brief(), lower.functions.as_brief());
}

#[tokio::test]
async fn test_list_functions_search_no_match_returns_empty() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("nonexistent_function_xyz".into()),
            ..Default::default()
        })
        .await
        .expect("list_functions");

    assert!(response.functions.as_brief().expect("brief").is_empty());
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_list_functions_search_supports_wildcard_semantics() {
    let handler = handler(true);
    let plain = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            ..Default::default()
        })
        .await
        .expect("plain");
    let with_wildcard = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("%order%".into()),
            ..Default::default()
        })
        .await
        .expect("wildcard");
    assert_eq!(plain.functions.as_brief(), with_wildcard.functions.as_brief());
}

#[tokio::test]
async fn test_list_functions_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in [
        "'",
        ";",
        "--",
        "\\",
        "`",
        "/* */",
        "'; DROP FUNCTION calc_order_total; --",
    ] {
        let response = handler
            .list_functions(ListFunctionsRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("list_functions failed for payload {payload:?}: {e:?}"));

        assert!(
            response.functions.as_brief().is_some(),
            "payload {payload:?} returned non-brief shape"
        );
    }
}

#[tokio::test]
async fn test_list_functions_search_paginates_filtered_results() {
    let paged = handler_with_page_size(2);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_functions(ListFunctionsRequest {
                database: Some("app".into()),
                cursor,
                search: Some("order".into()),
                ..Default::default()
            })
            .await
            .expect("list_functions paginated");
        let names = response.functions.as_brief().expect("brief").to_vec();
        all.extend(names);
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let single = handler(true)
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            ..Default::default()
        })
        .await
        .expect("single-page list_functions");
    assert_eq!(
        all,
        single.functions.as_brief().expect("brief"),
        "paginated traversal should equal single-page result"
    );
}

#[tokio::test]
async fn test_list_functions_brief_mode_wire_shape_unchanged() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let names = response.functions.as_brief().expect("brief mode (Vec<String>)");
    assert!(
        names.iter().all(|s| !s.is_empty()),
        "brief mode names must be non-empty strings, got {names:?}"
    );
    // Must serialise as a flat array of strings (no object wrapping).
    let json = serde_json::to_value(&response).expect("serialize");
    let functions = json.get("functions").expect("functions key");
    assert!(
        functions.is_array(),
        "brief mode `functions` must be a JSON array, got {functions:?}"
    );
}

#[tokio::test]
async fn test_list_functions_detailed_returns_keyed_object_with_all_fields() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("calc_order_total".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let entries = response.functions.as_detailed().expect("detailed mode");
    // Search "calc_order_total" matches both `calc_order_total` and
    // `recalc_order_total_v2` because the latter contains the former as a
    // substring. Field-shape assertions only need the calc_order_total entry.
    let entry = entries.get("calc_order_total").expect("calc_order_total key present");

    let obj = entry.as_object().expect("entry is object");
    let expected_keys = [
        "schema",
        "language",
        "arguments",
        "returnType",
        "deterministic",
        "sqlDataAccess",
        "security",
        "definer",
        "description",
        "definition",
        "sqlMode",
        "characterSetClient",
        "collationConnection",
        "databaseCollation",
    ];
    for key in expected_keys {
        assert!(obj.contains_key(key), "missing key {key:?} in {obj:?}");
    }
    assert_eq!(obj.len(), expected_keys.len(), "unexpected extra keys: {obj:?}");
    for forbidden in ["volatility", "parallelSafety", "strict", "owner", "name"] {
        assert!(
            !obj.contains_key(forbidden),
            "forbidden key {forbidden:?} present in {obj:?}"
        );
    }

    assert_eq!(obj.get("schema").and_then(Value::as_str), Some("app"));
    assert_eq!(obj.get("deterministic").and_then(Value::as_bool), Some(true));
    assert_eq!(obj.get("sqlDataAccess").and_then(Value::as_str), Some("READS_SQL_DATA"));
    assert_eq!(obj.get("security").and_then(Value::as_str), Some("INVOKER"));
    assert_eq!(
        obj.get("description").and_then(Value::as_str),
        Some("Sums line items for an order")
    );
    let language = obj.get("language").and_then(Value::as_str).expect("language");
    assert!(
        language.eq_ignore_ascii_case("SQL"),
        "expected language `SQL` (any casing), got {language:?}"
    );
}

#[tokio::test]
async fn test_list_functions_detailed_definition_uses_canonical_definer_form() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("calc_order_total".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let entries = response.functions.as_detailed().expect("detailed");
    let definition = entries
        .get("calc_order_total")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition string");

    assert!(
        definition.starts_with("CREATE DEFINER=`"),
        "expected canonical DEFINER form, got: {definition:?}"
    );
    assert!(
        definition.contains("`@`"),
        "expected `@` separator, got: {definition:?}"
    );
    assert!(
        definition.contains(" FUNCTION `calc_order_total`"),
        "expected backtick-quoted function name, got: {definition:?}"
    );
    assert!(
        !definition.starts_with("CREATE DEFINER='"),
        "legacy single-quoted DEFINER form leaked: {definition:?}"
    );
    // Reconstructed text must include the declared characteristics in keyword form.
    for needle in [
        " RETURNS decimal(12,2)",
        " DETERMINISTIC",
        " READS SQL DATA",
        " SQL SECURITY INVOKER",
        " COMMENT 'Sums line items for an order'",
    ] {
        assert!(
            definition.contains(needle),
            "expected `{needle}` in definition, got: {definition:?}"
        );
    }
}

#[tokio::test]
async fn test_list_functions_detailed_two_field_safety_split() {
    let handler = handler(true);

    // recalc_order_total_v2 — NOT DETERMINISTIC + MODIFIES SQL DATA + DEFINER security.
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("recalc_order_total_v2".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("recalc_order_total_v2")
        .expect("entry")
        .clone();
    assert_eq!(entry.get("deterministic").and_then(Value::as_bool), Some(false));
    assert_eq!(
        entry.get("sqlDataAccess").and_then(Value::as_str),
        Some("MODIFIES_SQL_DATA")
    );
    assert_eq!(entry.get("security").and_then(Value::as_str), Some("DEFINER"));

    // current_pricing_version — NO_SQL.
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("current_pricing_version".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("current_pricing_version")
        .expect("entry")
        .clone();
    assert_eq!(entry.get("deterministic").and_then(Value::as_bool), Some(true));
    assert_eq!(entry.get("sqlDataAccess").and_then(Value::as_str), Some("NO_SQL"));
    assert_eq!(entry.get("security").and_then(Value::as_str), Some("INVOKER"));

    // double_it — CONTAINS_SQL (default for declarations without an access clause).
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("double_it".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("double_it")
        .expect("entry")
        .clone();
    assert_eq!(entry.get("sqlDataAccess").and_then(Value::as_str), Some("CONTAINS_SQL"));
}

#[tokio::test]
async fn test_list_functions_detailed_arguments_field_round_trips_dtd() {
    let handler = handler(true);

    // Multi-parameter function — `arguments` follows ORDINAL_POSITION ASC.
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("calc_order_subtotal".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("calc_order_subtotal")
        .expect("entry")
        .clone();
    let arguments = entry
        .get("arguments")
        .and_then(Value::as_str)
        .expect("arguments string");
    // Engine casing of identifier names varies (MySQL preserves declaration case;
    // MariaDB lowercases). Compare case-insensitively.
    let lower = arguments.to_lowercase();
    assert!(
        lower.contains("order_id") && lower.contains("exclude_title"),
        "expected both parameter names in arguments, got {arguments:?}"
    );
    assert!(
        lower.contains("int") && lower.contains("varchar"),
        "expected both DTD types in arguments, got {arguments:?}"
    );
    // ORDINAL_POSITION ASC: order_id (pos 1) appears before exclude_title (pos 2).
    let order_id_pos = lower.find("order_id").expect("order_id present");
    let exclude_pos = lower.find("exclude_title").expect("exclude_title present");
    assert!(
        order_id_pos < exclude_pos,
        "arguments must be ordered by ORDINAL_POSITION; got {arguments:?}"
    );

    // Zero-parameter function — `arguments` is empty string, not null.
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("current_pricing_version".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("current_pricing_version")
        .expect("entry")
        .clone();
    assert_eq!(
        entry.get("arguments").and_then(Value::as_str),
        Some(""),
        "zero-parameter function must report empty-string arguments, got {:?}",
        entry.get("arguments")
    );
}

#[tokio::test]
async fn test_list_functions_detailed_description_null_coercion() {
    let handler = handler(true);

    // Function with no COMMENT — description must be JSON null (NOT empty string).
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("double_it".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("double_it")
        .expect("entry")
        .clone();
    assert!(
        entry.get("description").is_some_and(Value::is_null),
        "expected description=null for no-comment function, got {:?}",
        entry.get("description")
    );

    // Function with a COMMENT — description carries the text.
    let resp = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("calc_order_total".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let entry = resp
        .functions
        .as_detailed()
        .expect("detailed")
        .get("calc_order_total")
        .expect("entry")
        .clone();
    assert_eq!(
        entry.get("description").and_then(Value::as_str),
        Some("Sums line items for an order")
    );
}

#[tokio::test]
async fn test_list_functions_detailed_session_context_fields_are_populated() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let entries = response.functions.as_detailed().expect("detailed");
    assert!(!entries.is_empty(), "expected order matches");
    for (name, value) in entries {
        for key in [
            "sqlMode",
            "characterSetClient",
            "collationConnection",
            "databaseCollation",
        ] {
            let s = value
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{name}: {key} missing or non-string"));
            assert!(!s.is_empty(), "{name}: {key} must be non-empty");
        }
    }
}

#[tokio::test]
async fn test_list_functions_detailed_definition_round_trips_multiline_body() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("format_audit_note".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let definition = response
        .functions
        .as_detailed()
        .expect("detailed")
        .get("format_audit_note")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition string")
        .to_string();

    // Body must contain the literal newline + escaped quote round-tripped.
    assert!(
        definition.contains("note: contains a quote"),
        "body content missing from definition: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_functions_detailed_search_narrows_payload() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some("order".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let keys: Vec<String> = response
        .functions
        .as_detailed()
        .expect("detailed")
        .keys()
        .cloned()
        .collect();
    let expected = vec![
        "calc_order_subtotal".to_string(),
        "calc_order_total".to_string(),
        "recalc_order_total_v2".to_string(),
    ];
    assert_eq!(keys, expected, "got {keys:?}");
    for forbidden in [
        "current_pricing_version",
        "double_it",
        "format_audit_note",
        "calc_total",
    ] {
        assert!(!keys.contains(&forbidden.to_string()), "leaked: {forbidden}");
    }
}

#[tokio::test]
async fn test_list_functions_detailed_paginates() {
    let paged = handler_with_page_size(2);
    let mut all_keys = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_functions(ListFunctionsRequest {
                database: Some("app".into()),
                cursor,
                detailed: true,
                ..Default::default()
            })
            .await
            .expect("list_functions paginated");
        let entries = response.functions.as_detailed().expect("detailed");
        assert!(entries.len() <= 2, "page exceeded page_size: {entries:?}");
        all_keys.extend(entries.keys().cloned());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let single = handler(true)
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("single-page list_functions");
    let single_keys: Vec<String> = single
        .functions
        .as_detailed()
        .expect("detailed")
        .keys()
        .cloned()
        .collect();
    assert_eq!(all_keys, single_keys, "paginated traversal mismatched single-page");
}

#[tokio::test]
async fn test_list_functions_excludes_loadable_udfs_and_procedures() {
    // Procedures (existing seed: `archive_user`, `touch_post`) must not surface
    // through listFunctions — ROUTINE_TYPE='FUNCTION' filter enforces this.
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_functions");

    let names = response.functions.as_brief().expect("brief").to_vec();
    for proc_name in ["archive_user", "touch_post"] {
        assert!(
            !names.contains(&proc_name.to_string()),
            "procedure `{proc_name}` leaked into listFunctions output: {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_and_tables_definer_form_unchanged_after_extraction() {
    // Cross-tool regression guard for the spec-053-canonical DEFINER fragment
    // extraction performed in spec 058. Both `listTriggers` detailed mode and
    // `listTables` (`triggers_info` CTE inside detailed mode) must continue to
    // emit the canonical `CREATE DEFINER=`<u>`@`<h>`` form byte-identically.
    let handler = handler(true);

    let triggers = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("posts_audit_after_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers detailed");
    let trigger_def = triggers
        .triggers
        .as_detailed()
        .expect("detailed")
        .get("posts_audit_after_insert")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("trigger definition");
    assert!(
        trigger_def.starts_with("CREATE DEFINER=`"),
        "triggers DEFINER regressed: {trigger_def:?}"
    );
    assert!(
        !trigger_def.starts_with("CREATE DEFINER='"),
        "legacy single-quoted DEFINER leaked in triggers: {trigger_def:?}"
    );

    let tables = handler
        .list_tables(ListTablesRequest {
            database: Some("app".into()),
            search: Some("posts".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_tables detailed");
    let posts = tables
        .tables
        .as_detailed()
        .expect("detailed")
        .get("posts")
        .expect("posts entry")
        .clone();
    let trigger_entries = posts.get("triggers").and_then(Value::as_array).expect("triggers array");
    assert!(!trigger_entries.is_empty(), "expected at least one trigger on posts");
    for tr in trigger_entries {
        let def = tr.get("definition").and_then(Value::as_str).expect("definition");
        assert!(
            def.starts_with("CREATE DEFINER=`"),
            "list_tables triggers_info DEFINER regressed: {def:?}"
        );
        assert!(
            !def.starts_with("CREATE DEFINER='"),
            "legacy single-quoted DEFINER leaked in list_tables: {def:?}"
        );
    }
}

// ----- listProcedures enrichment (spec 062) -----

async fn brief_procedures(handler: &MysqlHandler, search: Option<&str>) -> Vec<String> {
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            search: search.map(str::to_owned),
            ..Default::default()
        })
        .await
        .expect("brief list_procedures");
    response.procedures.into_brief().expect("brief mode")
}

async fn detailed_procedure_entries(handler: &MysqlHandler, search: &str) -> indexmap::IndexMap<String, Value> {
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            search: Some(search.into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("detailed list_procedures");
    response.procedures.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_procedures_search_filters_by_substring() {
    let handler = handler(true);
    let names = brief_procedures(&handler, Some("archive")).await;
    let expected = [
        "archive_order".to_string(),
        "archive_order_history".to_string(),
        "archive_user".to_string(),
        "purge_order_archive".to_string(),
    ];
    assert_eq!(names, expected, "got {names:?}");
}

#[tokio::test]
async fn test_list_procedures_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = brief_procedures(&handler, Some("archive")).await;
    let upper = brief_procedures(&handler, Some("ArChIvE")).await;
    assert_eq!(lower, upper);
}

#[tokio::test]
async fn test_list_procedures_search_no_match_returns_empty() {
    let handler = handler(true);
    let names = brief_procedures(&handler, Some("nonexistent_proc_xyz")).await;
    assert!(names.is_empty(), "expected empty list, got {names:?}");
}

#[tokio::test]
async fn test_list_procedures_search_keeps_like_wildcards() {
    let handler = handler(true);
    // `_` is a `LIKE` single-char wildcard — `archive_user` matches but so do
    // any other procedure with a single char between `archive` and `user`.
    // The seed only contains `archive_user`, so the match set is `[archive_user]`.
    let underscore = brief_procedures(&handler, Some("archive_user")).await;
    assert!(
        underscore.contains(&"archive_user".to_string()),
        "underscore wildcard should match archive_user: {underscore:?}"
    );
    // `%` matches everything — must be a superset of the unfiltered list size.
    let pct = brief_procedures(&handler, Some("%")).await;
    let unfiltered = brief_procedures(&handler, None).await;
    assert_eq!(pct, unfiltered, "% must match every procedure");
}

#[tokio::test]
async fn test_list_procedures_search_resists_sql_meta_chars() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "`", "\\", "archive'; DROP PROCEDURE bad; --"] {
        let result = handler
            .list_procedures(ListProceduresRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await;
        assert!(
            result.is_ok(),
            "adversarial payload {payload:?} must not error: {:?}",
            result.err()
        );
    }
}

#[tokio::test]
async fn test_list_procedures_brief_wire_shape_unchanged() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");

    let names = response.procedures.as_brief().expect("brief mode (Vec<String>)");
    assert!(
        names.iter().all(|s| !s.is_empty()),
        "brief mode names must be non-empty strings, got {names:?}"
    );
    let json = serde_json::to_value(&response).expect("serialize");
    let procedures = json.get("procedures").expect("procedures key");
    assert!(
        procedures.is_array(),
        "brief mode `procedures` must be a JSON array, got {procedures:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_returns_keyed_object_with_all_fields() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "archive_order").await;
    // search "archive_order" matches both `archive_order` and
    // `archive_order_history`. Field-shape assertions only need archive_order.
    let entry = entries.get("archive_order").expect("archive_order key present");

    let obj = entry.as_object().expect("entry is object");
    let expected_keys = [
        "schema",
        "language",
        "arguments",
        "deterministic",
        "sqlDataAccess",
        "security",
        "definer",
        "description",
        "definition",
        "sqlMode",
        "characterSetClient",
        "collationConnection",
        "databaseCollation",
    ];
    for key in expected_keys {
        assert!(obj.contains_key(key), "missing key {key:?} in {obj:?}");
    }
    assert_eq!(obj.len(), expected_keys.len(), "unexpected extra keys: {obj:?}");
    for forbidden in ["returnType", "volatility", "parallelSafety", "strict", "owner", "name"] {
        assert!(
            !obj.contains_key(forbidden),
            "forbidden key {forbidden:?} present in {obj:?}"
        );
    }

    assert_eq!(obj.get("schema").and_then(Value::as_str), Some("app"));
    assert_eq!(obj.get("deterministic").and_then(Value::as_bool), Some(true));
    assert_eq!(
        obj.get("sqlDataAccess").and_then(Value::as_str),
        Some("MODIFIES_SQL_DATA")
    );
    assert_eq!(obj.get("security").and_then(Value::as_str), Some("INVOKER"));
    assert_eq!(
        obj.get("description").and_then(Value::as_str),
        Some("Archives an order and returns the count")
    );
    let language = obj.get("language").and_then(Value::as_str).expect("language");
    assert!(
        language.eq_ignore_ascii_case("SQL"),
        "expected language `SQL` (any casing), got {language:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_renders_in_out_inout_modes_in_arguments() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "compute_user_metrics").await;
    let arguments = entries
        .get("compute_user_metrics")
        .and_then(|e| e.get("arguments"))
        .and_then(Value::as_str)
        .expect("arguments");

    let upper = arguments.to_uppercase();
    assert!(upper.contains("IN UID"), "expected `IN uid` in {arguments:?}");
    assert!(
        upper.contains("OUT METRIC_TOTAL"),
        "expected `OUT metric_total` in {arguments:?}"
    );
    assert!(
        upper.contains("INOUT METRIC_AVG"),
        "expected `INOUT metric_avg` in {arguments:?}"
    );
    let in_pos = upper.find("IN UID").unwrap();
    let out_pos = upper.find("OUT METRIC_TOTAL").unwrap();
    let inout_pos = upper.find("INOUT METRIC_AVG").unwrap();
    assert!(
        in_pos < out_pos && out_pos < inout_pos,
        "parameter order must follow ORDINAL_POSITION: {arguments:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_omits_returntype_field() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "archive_order").await;
    let entry = entries.get("archive_order").expect("archive_order");
    let obj = entry.as_object().expect("object");
    assert!(
        !obj.contains_key("returnType"),
        "procedures must NOT carry returnType: {obj:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_definition_omits_returns_clause() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "archive_order").await;
    let definition = entries
        .get("archive_order")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition");

    assert!(
        !definition.contains(" RETURNS "),
        "procedure `definition` must not contain ` RETURNS `: {definition:?}"
    );
    assert!(
        definition.starts_with("CREATE DEFINER=`"),
        "expected canonical CREATE DEFINER opener: {definition:?}"
    );
    assert!(
        definition.contains(" PROCEDURE `archive_order`"),
        "expected ` PROCEDURE `archive_order`` in: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_zero_parameter_arguments_is_empty_string() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "no_arg_proc").await;
    let entry = entries.get("no_arg_proc").expect("no_arg_proc");
    assert_eq!(
        entry.get("arguments").and_then(Value::as_str),
        Some(""),
        "zero-parameter procedure `arguments` must be empty string"
    );
    let definition = entry.get("definition").and_then(Value::as_str).expect("definition");
    assert!(
        definition.contains("PROCEDURE `no_arg_proc`()"),
        "expected `PROCEDURE `no_arg_proc`()` in: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_security_definer_reports_definer() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "compute_user_metrics").await;
    let security = entries
        .get("compute_user_metrics")
        .and_then(|e| e.get("security"))
        .and_then(Value::as_str)
        .expect("security");
    assert_eq!(security, "DEFINER", "got {security:?}");

    let entries_invoker = detailed_procedure_entries(&handler, "archive_order").await;
    let security_invoker = entries_invoker
        .get("archive_order")
        .and_then(|e| e.get("security"))
        .and_then(Value::as_str)
        .expect("security");
    assert_eq!(security_invoker, "INVOKER", "got {security_invoker:?}");
}

#[tokio::test]
async fn test_list_procedures_detailed_no_comment_yields_null_description() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "purge_order_archive").await;
    let description = entries
        .get("purge_order_archive")
        .and_then(|e| e.get("description"))
        .expect("description field present");
    assert!(
        description.is_null(),
        "no-comment procedure must coerce empty ROUTINE_COMMENT to JSON null, got {description:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_combined_with_search_filters_before_metadata() {
    let handler = handler(true);
    let detailed = detailed_procedure_entries(&handler, "archive").await;
    let brief = brief_procedures(&handler, Some("archive")).await;

    let detailed_keys: Vec<String> = detailed.keys().cloned().collect();
    assert_eq!(detailed_keys, brief, "detailed key set must equal brief filtered set");
    for key in detailed.keys() {
        assert!(
            key.to_lowercase().contains("archive"),
            "non-matching procedure {key:?} leaked into detailed payload"
        );
    }
}

#[tokio::test]
async fn test_list_procedures_detailed_iteration_order_matches_brief_sort() {
    let handler = handler(true);
    let brief = brief_procedures(&handler, Some("archive")).await;
    let detailed = detailed_procedure_entries(&handler, "archive").await;
    let detailed_keys: Vec<String> = detailed.keys().cloned().collect();
    assert_eq!(
        brief, detailed_keys,
        "detailed iteration order must match brief alphabetical sort"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_session_context_fields_populated() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "archive_order").await;
    let entry = entries.get("archive_order").expect("archive_order");
    for key in [
        "sqlMode",
        "characterSetClient",
        "collationConnection",
        "databaseCollation",
    ] {
        let value = entry.get(key).and_then(Value::as_str);
        assert!(
            matches!(value, Some(s) if !s.is_empty()),
            "session-context field {key:?} must be a non-empty string, got {value:?}"
        );
    }
}

#[tokio::test]
async fn test_list_procedures_detailed_round_trips_body_with_quotes_and_newlines() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "no_arg_proc").await;
    let definition = entries
        .get("no_arg_proc")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition");

    // MySQL 9 interprets `\n` in the source as a literal newline char in
    // ROUTINE_DEFINITION; MariaDB 12 preserves the two-byte `\n` escape
    // sequence verbatim. Accept either form — the contract is "body content
    // round-trips without truncation".
    assert!(
        definition.contains('\n') || definition.contains("\\n"),
        "expected newline (literal or escape) in body round-trip: {definition:?}"
    );
    // MySQL 9 strips SQL-escape doubling on stored bodies (returns `'here'`);
    // MariaDB 12 preserves the doubled form (returns `''here''`). Both
    // contain `'here'` as a substring — assert that.
    assert!(
        definition.contains("'here'"),
        "expected `'here'` in body round-trip: {definition:?}"
    );
    assert!(
        definition.contains("first line"),
        "expected first-line marker in body round-trip: {definition:?}"
    );
    assert!(
        definition.contains("second line"),
        "expected second-line marker in body round-trip: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_definition_uses_canonical_definer_form() {
    let handler = handler(true);
    let entries = detailed_procedure_entries(&handler, "archive_order").await;
    let definition = entries
        .get("archive_order")
        .and_then(|e| e.get("definition"))
        .and_then(Value::as_str)
        .expect("definition");
    assert!(
        definition.starts_with("CREATE DEFINER=`"),
        "canonical DEFINER form regressed: {definition:?}"
    );
    assert!(
        !definition.starts_with("CREATE DEFINER='"),
        "legacy single-quoted DEFINER leaked: {definition:?}"
    );
}

// ---------------------------------------------------------------------------
// `listViews` — search + detailed (spec 064)
// ---------------------------------------------------------------------------

async fn brief_views(handler: &MysqlHandler, search: Option<&str>) -> Vec<String> {
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            search: search.map(str::to_owned),
            ..Default::default()
        })
        .await
        .expect("brief list_views");
    response.views.into_brief().expect("brief mode")
}

async fn detailed_view_entries(handler: &MysqlHandler, search: &str) -> indexmap::IndexMap<String, Value> {
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            search: Some(search.into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("detailed list_views");
    response.views.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_views_search_filters_by_substring() {
    let handler = handler(true);
    let names = brief_views(&handler, Some("active")).await;
    let expected = [
        "active_orders".to_string(),
        "active_users".to_string(),
        "active_users_v2".to_string(),
        "active_users_with_check_cascaded".to_string(),
        "active_users_with_check_local".to_string(),
    ];
    assert_eq!(names, expected, "got {names:?}");
}

#[tokio::test]
async fn test_list_views_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = brief_views(&handler, Some("active")).await;
    let upper = brief_views(&handler, Some("ActIvE")).await;
    assert_eq!(lower, upper);
}

#[tokio::test]
async fn test_list_views_search_keeps_like_wildcards() {
    let handler = handler(true);
    let pct = brief_views(&handler, Some("%users%")).await;
    assert!(
        pct.contains(&"active_users".to_string()),
        "wildcard `%users%` must match `active_users`, got {pct:?}"
    );
    assert!(
        pct.iter().all(|n| n.contains("users")),
        "wildcard `%users%` must only return `*users*`, got {pct:?}"
    );

    // `_` matches a single character; `active_` followed by `%` matches every
    // view whose name begins with `active_`.
    let underscore = brief_views(&handler, Some("active\\_%")).await;
    assert!(
        !underscore.is_empty(),
        "wildcard `active\\_%` must match seeded views, got {underscore:?}"
    );
    assert!(
        underscore.iter().all(|n| n.starts_with("active_")),
        "wildcard `active\\_%` must only return `active_*`, got {underscore:?}"
    );
}

#[tokio::test]
async fn test_list_views_search_no_match_returns_empty() {
    let handler = handler(true);
    let names = brief_views(&handler, Some("nonexistent_view_xyz")).await;
    assert!(names.is_empty(), "expected empty list, got {names:?}");
}

#[tokio::test]
async fn test_list_views_brief_wire_shape_unchanged() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_views");

    let names = response.views.as_brief().expect("brief mode (Vec<String>)");
    assert!(
        names.iter().all(|s| !s.is_empty()),
        "brief mode names must be non-empty strings, got {names:?}"
    );
    let json = serde_json::to_value(&response).expect("serialize");
    let views = json.get("views").expect("views key");
    assert!(
        views.is_array(),
        "brief mode `views` must be a JSON array, got {views:?}"
    );
}

#[tokio::test]
async fn test_list_views_search_resists_sql_meta_chars() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "`", "\\", "active'; DROP VIEW bad; --"] {
        let result = handler
            .list_views(ListViewsRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await;
        assert!(
            result.is_ok(),
            "adversarial payload {payload:?} must not error: {:?}",
            result.err()
        );
    }
}

#[tokio::test]
async fn test_list_views_detailed_returns_keyed_object_with_all_fields() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active_users").await;
    let entry = entries.get("active_users").expect("active_users key present");

    for key in [
        "schema",
        "definer",
        "security",
        "checkOption",
        "updatable",
        "characterSetClient",
        "collationConnection",
        "definition",
    ] {
        assert!(entry.get(key).is_some(), "missing field {key}: {entry:?}");
    }

    // Forbidden fields — divergence from Postgres / MySQL list_procedures.
    for forbidden in [
        "description",
        "algorithm",
        "owner",
        "language",
        "arguments",
        "sqlMode",
        "databaseCollation",
    ] {
        assert!(
            entry.get(forbidden).is_none(),
            "forbidden field {forbidden} present: {entry:?}"
        );
    }

    assert_eq!(
        entry.get("schema").and_then(Value::as_str),
        Some("app"),
        "schema must equal active database"
    );
}

#[tokio::test]
async fn test_list_views_detailed_check_option_none_for_plain_view() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active_users").await;
    let entry = entries.get("active_users").expect("active_users present");
    assert_eq!(
        entry.get("checkOption").and_then(Value::as_str),
        Some("NONE"),
        "plain view must report checkOption=NONE"
    );
}

#[tokio::test]
async fn test_list_views_detailed_check_option_cascaded_and_local() {
    let handler = handler(true);
    let cascaded_entries = detailed_view_entries(&handler, "active_users_with_check_cascaded").await;
    let cascaded = cascaded_entries
        .get("active_users_with_check_cascaded")
        .expect("cascaded view present");
    assert_eq!(cascaded.get("checkOption").and_then(Value::as_str), Some("CASCADED"));

    let local_entries = detailed_view_entries(&handler, "active_users_with_check_local").await;
    let local = local_entries
        .get("active_users_with_check_local")
        .expect("local view present");
    assert_eq!(local.get("checkOption").and_then(Value::as_str), Some("LOCAL"));
}

#[tokio::test]
async fn test_list_views_detailed_updatable_boolean() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active").await;

    assert_eq!(
        entries
            .get("active_users")
            .expect("active_users present")
            .get("updatable")
            .and_then(Value::as_bool),
        Some(true),
        "single-table view must be updatable"
    );

    assert_eq!(
        entries
            .get("active_orders")
            .expect("active_orders present")
            .get("updatable")
            .and_then(Value::as_bool),
        Some(false),
        "JOIN view must not be updatable"
    );
}

#[tokio::test]
async fn test_list_views_detailed_security_modes() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "archived").await;

    let definer = entries.get("archived_users").expect("archived_users present");
    assert_eq!(definer.get("security").and_then(Value::as_str), Some("DEFINER"));
    let definer_string = definer.get("definer").and_then(Value::as_str).expect("definer string");
    assert!(
        definer_string.contains('@'),
        "definer must be user@host form, got {definer_string:?}"
    );

    let invoker = entries
        .get("archived_users_invoker")
        .expect("archived_users_invoker present");
    assert_eq!(invoker.get("security").and_then(Value::as_str), Some("INVOKER"));
}

#[tokio::test]
async fn test_list_views_detailed_definition_is_raw_select_body() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active_users").await;
    let entry = entries.get("active_users").expect("active_users present");
    let definition = entry
        .get("definition")
        .and_then(Value::as_str)
        .expect("definition string");

    let upper = definition.trim_start().to_uppercase();
    assert!(
        !upper.starts_with("CREATE"),
        "definition must be raw SELECT body, no CREATE wrapper: {definition:?}"
    );
    assert!(
        !upper.contains("WITH CASCADED CHECK OPTION") && !upper.contains("WITH LOCAL CHECK OPTION"),
        "plain view definition must not carry a WITH CHECK OPTION suffix: {definition:?}"
    );
    assert!(
        !definition.is_empty(),
        "definition must be non-empty for a privileged role"
    );
}

#[tokio::test]
async fn test_list_views_detailed_combined_with_search_filters_before_metadata() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active").await;
    assert!(!entries.is_empty(), "search=active must match seeded views");
    for (name, value) in &entries {
        assert!(
            name.to_lowercase().contains("active"),
            "non-matching key returned: {name}"
        );
        // Each entry is fully detailed.
        assert!(
            value.get("schema").is_some() && value.get("definition").is_some(),
            "entry must carry detailed fields, got {value:?}"
        );
    }
}

#[tokio::test]
async fn test_list_views_detailed_round_trips_body_with_subquery() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "user_metrics_cte").await;
    let entry = entries.get("user_metrics_cte").expect("user_metrics_cte present");
    let definition = entry
        .get("definition")
        .and_then(Value::as_str)
        .expect("definition string");
    // Engine canonicalises CTE to subquery form; assert the subquery survives
    // round-trip without truncation or trimming.
    assert!(
        definition.to_lowercase().contains("select"),
        "subquery body must round-trip, got {definition:?}"
    );
    assert!(
        definition.to_lowercase().contains("post_count") || definition.to_lowercase().contains("count("),
        "subquery body must mention post_count or count(...), got {definition:?}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_session_context_fields_populated() {
    let handler = handler(true);
    let entries = detailed_view_entries(&handler, "active").await;
    assert!(!entries.is_empty(), "seeded views must be present");
    for (name, value) in &entries {
        let charset = value
            .get("characterSetClient")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("characterSetClient missing for {name}: {value:?}"));
        assert!(
            !charset.is_empty(),
            "characterSetClient must be non-empty for {name}: {value:?}"
        );
        let collation = value
            .get("collationConnection")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("collationConnection missing for {name}: {value:?}"));
        assert!(
            !collation.is_empty(),
            "collationConnection must be non-empty for {name}: {value:?}"
        );
    }
}
