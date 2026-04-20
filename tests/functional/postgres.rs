//! Functional tests for `PostgreSQL`.
//!
//! Tests exercise the handler methods directly, which is the same code
//! path the per-tool ZSTs delegate to.
//!
//! ```bash
//! ./tests/run.sh --filter postgres
//! ```

use database_mcp_config::{DatabaseBackend, DatabaseConfig};
use database_mcp_postgres::PostgresHandler;
use database_mcp_postgres::types::DropTableRequest;
use database_mcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, GetTableSchemaRequest, ListDatabasesRequest,
    ListTablesRequest, QueryRequest, ReadQueryRequest,
};
use serde_json::Value;

fn base_db_config(read_only: bool) -> DatabaseConfig {
    DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        host: std::env::var("DB_HOST").unwrap_or_else(|_| "127.0.0.1".into()),
        port: std::env::var("DB_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(5432),
        user: std::env::var("DB_USER").unwrap_or_else(|_| "postgres".into()),
        password: std::env::var("DB_PASSWORD").ok(),
        name: Some("app".into()),
        read_only,
        ..DatabaseConfig::default()
    }
}

fn handler_with_page_size(page_size: u16) -> PostgresHandler {
    let config = DatabaseConfig {
        page_size,
        ..base_db_config(false)
    };
    PostgresHandler::new(&config)
}

fn handler(read_only: bool) -> PostgresHandler {
    let config = base_db_config(read_only);
    PostgresHandler::new(&config)
}

#[tokio::test]
async fn test_write_query_insert_and_verify() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('WriteTest', 'write@test.com')".into(),
        database: "app".into(),
    };
    handler.write_query(insert).await.unwrap();

    // Verify the row was inserted
    let select = ReadQueryRequest {
        query: "SELECT name FROM users WHERE email = 'write@test.com'".into(),
        database: "app".into(),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["name"], "WriteTest");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'write@test.com'".into(),
        database: "app".into(),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_update() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Before', 'update@test.com')".into(),
        database: "app".into(),
    };
    handler.write_query(insert).await.unwrap();

    let update = QueryRequest {
        query: "UPDATE users SET name = 'After' WHERE email = 'update@test.com'".into(),
        database: "app".into(),
    };
    handler.write_query(update).await.unwrap();

    let select = ReadQueryRequest {
        query: "SELECT name FROM users WHERE email = 'update@test.com'".into(),
        database: "app".into(),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert_eq!(arr[0]["name"], "After");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'update@test.com'".into(),
        database: "app".into(),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_write_query_delete() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Deletable', 'delete@test.com')".into(),
        database: "app".into(),
    };
    handler.write_query(insert).await.unwrap();

    let delete = QueryRequest {
        query: "DELETE FROM users WHERE email = 'delete@test.com'".into(),
        database: "app".into(),
    };
    handler.write_query(delete).await.unwrap();

    let select = ReadQueryRequest {
        query: "SELECT * FROM users WHERE email = 'delete@test.com'".into(),
        database: "app".into(),
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
        database: "app".into(),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
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
        database: "app".into(),
        table: "users".into(),
    };

    let schema = handler.get_table_schema(request).await.unwrap();

    assert_eq!(schema.table, "users");
    let columns = schema.columns.as_object().expect("columns object");
    for col in ["id", "name", "email", "created_at"] {
        assert!(columns.contains_key(col), "Missing '{col}' in: {columns:?}");
    }
}

#[tokio::test]
async fn test_gets_table_schema_with_relations() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: "posts".into(),
    };

    let schema = handler.get_table_schema(request).await.unwrap();

    let columns = schema.columns.as_object().expect("columns object");
    assert!(columns.contains_key("user_id"), "Missing 'user_id' column");
    let user_id = columns["user_id"].as_object().expect("user_id object");
    assert!(
        user_id.contains_key("foreignKey"),
        "Missing 'foreignKey' in user_id column"
    );
    assert!(
        !user_id["foreignKey"].is_null(),
        "foreignKey should not be null for user_id"
    );
}

#[tokio::test]
async fn test_executes_sql() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users ORDER BY id".into(),
        database: "app".into(),
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
        database: "app".into(),
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
        database: "analytics".into(),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
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
    let request = ReadQueryRequest {
        query: "SELECT * FROM events ORDER BY id".into(),
        database: "analytics".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    assert_eq!(response.rows.len(), 2, "Expected 2 events, got {}", response.rows.len());
}

#[tokio::test]
async fn test_gets_table_schema_cross_database() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "analytics".into(),
        table: "events".into(),
    };

    let response = handler.get_table_schema(request).await.unwrap();

    assert_eq!(response.table, "events");
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
        database: "analytics".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await;

    assert!(
        response.is_err(),
        "Expected error for write in read-only mode on cross-database"
    );
}

#[tokio::test]
async fn test_returns_error_for_nonexistent_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: "nonexistent_db_xyz".into(),
        ..Default::default()
    };

    let response = handler.list_tables(request).await;

    assert!(response.is_err(), "Expected error for nonexistent database");
}

#[tokio::test]
async fn test_uses_default_pool_for_matching_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: "app".into(),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
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
    let handler = PostgresHandler::new(&config);
    let request = ReadQueryRequest {
        query: "SELECT pg_sleep(30)".into(),
        database: "app".into(),
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
async fn test_query_timeout_disabled_with_none() {
    let config = DatabaseConfig {
        query_timeout: None,
        ..base_db_config(false)
    };
    let handler = PostgresHandler::new(&config);
    let request = ReadQueryRequest {
        query: "SELECT 1 AS value".into(),
        database: "app".into(),
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
        query: "CREATE TABLE drop_test_simple (id SERIAL PRIMARY KEY)".into(),
        database: "app".into(),
    };
    handler.write_query(create).await.unwrap();

    // Drop it
    let drop_request = DropTableRequest {
        database: "app".into(),
        table: "drop_test_simple".into(),
        cascade: false,
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let tables_request = ListTablesRequest {
        database: "app".into(),
        ..Default::default()
    };
    let response = handler.list_tables(tables_request).await.unwrap();
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
        query: "CREATE TABLE drop_test_parent (id SERIAL PRIMARY KEY)".into(),
        database: "app".into(),
    };
    handler.write_query(create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_child (id SERIAL PRIMARY KEY, parent_id INT REFERENCES drop_test_parent(id))"
            .into(),
        database: "app".into(),
    };
    handler.write_query(create_child).await.unwrap();

    // Attempt to drop parent without cascade — should fail
    let drop_request = DropTableRequest {
        database: "app".into(),
        table: "drop_test_parent".into(),
        cascade: false,
    };
    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected FK constraint error");

    // Clean up
    let cleanup_child = QueryRequest {
        query: "DROP TABLE drop_test_child".into(),
        database: "app".into(),
    };
    handler.write_query(cleanup_child).await.unwrap();

    let cleanup_parent = QueryRequest {
        query: "DROP TABLE drop_test_parent".into(),
        database: "app".into(),
    };
    handler.write_query(cleanup_parent).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_cascade() {
    let handler = handler(false);

    // Create parent and child tables with FK
    let create_parent = QueryRequest {
        query: "CREATE TABLE drop_test_cascade_parent (id SERIAL PRIMARY KEY)".into(),
        database: "app".into(),
    };
    handler.write_query(create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_cascade_child (id SERIAL PRIMARY KEY, parent_id INT REFERENCES drop_test_cascade_parent(id))".into(),
        database: "app".into(),
    };
    handler.write_query(create_child).await.unwrap();

    // Drop parent with cascade — should succeed
    let drop_request = DropTableRequest {
        database: "app".into(),
        table: "drop_test_cascade_parent".into(),
        cascade: true,
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Clean up child table (still exists, just lost FK constraint)
    let cleanup = QueryRequest {
        query: "DROP TABLE IF EXISTS drop_test_cascade_child".into(),
        database: "app".into(),
    };
    handler.write_query(cleanup).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_nonexistent() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: "app".into(),
        table: "nonexistent_table_xyz".into(),
        cascade: false,
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected error for nonexistent table");
}

#[tokio::test]
async fn test_drop_table_invalid_identifier() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: "app".into(),
        table: String::new(),
        cascade: false,
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_explain_query_select() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(!plan.is_empty(), "Expected non-empty execution plan");
}

#[tokio::test]
async fn test_explain_query_analyze() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(!plan.is_empty(), "Expected non-empty execution plan with analyze");
}

#[tokio::test]
async fn test_explain_query_analyze_write_blocked_read_only() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database: "app".into(),
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
async fn test_explain_query_plain_write_allowed() {
    let handler = handler(true);
    let request = ExplainQueryRequest {
        database: "app".into(),
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
        database: "app".into(),
        query: "NOT VALID SQL AT ALL".into(),
        analyze: false,
    };

    let response = handler.explain_query(request).await;
    assert!(response.is_err(), "Expected error for invalid SQL");
}

#[tokio::test]
async fn test_get_table_schema_nonexistent_table() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: "nonexistent_table_xyz".into(),
    };

    let response = handler.get_table_schema(request).await;
    assert!(response.is_err(), "Expected error for nonexistent table");
}

#[tokio::test]
async fn test_get_table_schema_invalid_table_name() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: String::new(),
    };

    let response = handler.get_table_schema(request).await;
    assert!(response.is_err(), "Expected error for empty table name");
}

#[tokio::test]
async fn test_create_database_already_exists() {
    let handler = handler(false);
    let request = CreateDatabaseRequest { database: "app".into() };

    let response = handler.create_database(request).await;
    // PostgreSQL returns an error that contains "already exists"
    let err_msg = format!(
        "{:?}",
        response.expect_err("Expected error when creating existing database")
    );
    assert!(
        err_msg.contains("already exists"),
        "Expected 'already exists' in error, got: {err_msg}"
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
async fn test_list_tables_invalid_database_name() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: String::new(),
        ..Default::default()
    };

    // Empty database is treated as None (uses default pool)
    let response = handler.list_tables(request).await.unwrap();
    let tables = response.tables;
    assert!(
        tables.iter().any(|t| t == "users"),
        "Empty db name should fall back to default: {tables:?}"
    );
}

#[tokio::test]
async fn test_read_query_empty_query() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: String::new(),
        database: "app".into(),
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
        database: "app".into(),
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
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for multi-statement query");
}

#[tokio::test]
async fn test_read_query_into_outfile_blocked() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users INTO OUTFILE '/tmp/out'".into(),
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await;
    assert!(response.is_err(), "Expected error for INTO OUTFILE");
}

#[tokio::test]
async fn test_drop_table_cross_database() {
    let handler = handler(false);

    // Create a table in the analytics database
    let create = QueryRequest {
        query: "CREATE TABLE drop_cross_test (id SERIAL PRIMARY KEY)".into(),
        database: "analytics".into(),
    };
    handler.write_query(create).await.unwrap();

    // Drop it from the analytics database
    let drop_request = DropTableRequest {
        database: "analytics".into(),
        table: "drop_cross_test".into(),
        cascade: false,
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));
}

#[tokio::test]
async fn test_write_query_cross_database() {
    let handler = handler(false);

    let insert = QueryRequest {
        query: "INSERT INTO events (name, payload) VALUES ('cross_test', '{\"test\":true}')".into(),
        database: "analytics".into(),
    };
    handler.write_query(insert).await.unwrap();

    let select = ReadQueryRequest {
        query: "SELECT name FROM events WHERE name = 'cross_test'".into(),
        database: "analytics".into(),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let arr = &rows.rows;
    assert!(!arr.is_empty(), "Cross-database write should persist");

    // Clean up
    let delete = QueryRequest {
        query: "DELETE FROM events WHERE name = 'cross_test'".into(),
        database: "analytics".into(),
    };
    handler.write_query(delete).await.unwrap();
}

#[tokio::test]
async fn test_get_table_schema_junction_table() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: "post_tags".into(),
    };

    let schema = handler.get_table_schema(request).await.unwrap();
    assert_eq!(schema.table, "post_tags");

    let columns = schema.columns.as_object().expect("columns object");
    assert!(columns.contains_key("post_id"), "Missing 'post_id'");
    assert!(columns.contains_key("tag_id"), "Missing 'tag_id'");

    // Both columns should have foreign keys
    let post_id = columns["post_id"].as_object().expect("post_id object");
    assert!(
        post_id.get("foreignKey").is_some_and(|fk| !fk.is_null()),
        "post_id should have a foreign key"
    );

    let tag_id = columns["tag_id"].as_object().expect("tag_id object");
    assert!(
        tag_id.get("foreignKey").is_some_and(|fk| !fk.is_null()),
        "tag_id should have a foreign key"
    );
}

#[tokio::test]
async fn test_read_query_empty_result_set() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users WHERE email = 'nobody@nowhere.com'".into(),
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(rows.is_empty(), "Expected empty result set");
}

#[tokio::test]
async fn test_read_query_aggregate() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT COUNT(*) AS total FROM users".into(),
        database: "app".into(),
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
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert!(rows.len() >= 2, "Expected at least 2 groups");
}

#[tokio::test]
async fn test_explain_query_cross_database() {
    let handler = handler(false);
    let request = ExplainQueryRequest {
        database: "analytics".into(),
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
        database: "app".into(),
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
        query: "SELECT * FROM users WHERE id IN (SELECT user_id FROM posts WHERE published = true)".into(),
        database: "app".into(),
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
        database: "app".into(),
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
        database: "app".into(),
        query: "SELECT * FROM users".into(),
        analyze: true,
    };

    let response = handler.explain_query(request).await.unwrap();
    let plan = &response.rows;
    assert!(
        !plan.is_empty(),
        "EXPLAIN ANALYZE on SELECT should succeed in read-only mode"
    );
}

#[tokio::test]
async fn test_write_query_invalid_sql() {
    let handler = handler(false);
    let request = QueryRequest {
        query: "NOT VALID SQL AT ALL".into(),
        database: "app".into(),
    };

    let response = handler.write_query(request).await;
    assert!(response.is_err(), "Expected error for invalid SQL in write_query");
}

#[tokio::test]
async fn test_get_table_schema_column_details() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: "users".into(),
    };

    let schema = handler.get_table_schema(request).await.unwrap();
    let columns = schema.columns.as_object().expect("columns object");

    // Verify email column type
    let email_col = columns["email"].as_object().expect("email object");
    let col_type = email_col.get("type").and_then(|v| v.as_str()).unwrap_or("");
    assert!(
        col_type.to_lowercase().contains("character varying") || col_type.to_lowercase().contains("varchar"),
        "email type should contain 'character varying' or 'varchar', got: {col_type}"
    );

    // Verify nullable field
    let nullable = email_col.get("nullable").and_then(Value::as_bool);
    assert_eq!(nullable, Some(false), "email should be NOT NULL");
}

#[tokio::test]
async fn test_read_query_with_limit() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "SELECT * FROM users ORDER BY id LIMIT 2".into(),
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 2, "LIMIT 2 should return exactly 2 rows");
}

#[tokio::test]
async fn test_drop_table_invalid_database_name() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: String::new(),
        table: "users".into(),
        cascade: false,
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "Expected error for empty database name");
}

#[tokio::test]
async fn test_read_query_with_line_comment() {
    let handler = handler(false);
    let request = ReadQueryRequest {
        query: "-- get users\nSELECT * FROM users ORDER BY id".into(),
        database: "app".into(),
        cursor: None,
    };

    let response = handler.read_query(request).await.unwrap();
    let rows = &response.rows;
    assert_eq!(rows.len(), 3, "Line-comment prefixed SELECT should work");
}

#[tokio::test]
async fn test_get_table_schema_no_foreign_keys() {
    let handler = handler(false);
    let request = GetTableSchemaRequest {
        database: "app".into(),
        table: "tags".into(),
    };

    let schema = handler.get_table_schema(request).await.unwrap();
    assert_eq!(schema.table, "tags");
    let columns = schema.columns.as_object().expect("columns object");
    assert!(columns.contains_key("id"));
    assert!(columns.contains_key("name"));
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
        database: "app".into(),
        table: "users".into(),
        cascade: false,
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "drop_table should be blocked in read-only mode");
}

// === US2: Connection trait edge cases ===

#[tokio::test]
async fn test_read_query_control_char_database_name_rejected() {
    let handler = handler(true);
    let request = ReadQueryRequest {
        query: "SELECT 1".into(),
        database: "test\x01db".into(),
        cursor: None,
    };
    let result = handler.read_query(request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

#[tokio::test]
async fn test_list_tables_control_char_database_rejected() {
    let handler = handler(true);
    let request = ListTablesRequest {
        database: "test\x00db".into(),
        ..Default::default()
    };
    let result = handler.list_tables(request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

// === US4: Special-character identifier round-trip ===

#[tokio::test]
async fn test_create_drop_database_with_backtick() {
    let handler = handler(false);
    let db_name = "test_backtick_db`edge".to_string();

    let create = CreateDatabaseRequest {
        database: db_name.clone(),
    };
    let result = handler.create_database(create).await;
    assert!(
        result.is_ok(),
        "create database with backtick should succeed: {result:?}"
    );

    let drop = DropDatabaseRequest { database: db_name };
    let result = handler.drop_database(drop).await;
    assert!(result.is_ok(), "drop database with backtick should succeed: {result:?}");
}

// === Pagination ===

const PG_DB: &str = "app";

async fn collect_all_paged(handler: &PostgresHandler) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<database_mcp_server::pagination::Cursor> = None;
    loop {
        let request = ListTablesRequest {
            database: PG_DB.into(),
            cursor,
        };
        let response = handler.list_tables(request).await.expect("list page");
        all.extend(response.tables);
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
            database: PG_DB.into(),
            ..Default::default()
        })
        .await
        .expect("single page");

    assert_eq!(
        collected, single_page.tables,
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
            database: PG_DB.into(),
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
            database: PG_DB.into(),
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
            database: PG_DB.into(),
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
    use database_mcp_server::pagination::Cursor;

    let handler = handler(true);
    let request = ListTablesRequest {
        database: PG_DB.into(),
        cursor: Some(Cursor { offset: 10_000 }),
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
            database: PG_DB.into(),
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
            database: PG_DB.into(),
            ..Default::default()
        })
        .await
        .expect("first page");
    assert_eq!(first.tables.len(), 1, "page_size=1 must return one table per page");
    assert!(first.next_cursor.is_some(), "page 1 must emit nextCursor");
}

async fn collect_all_paged_databases(handler: &PostgresHandler) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<database_mcp_server::pagination::Cursor> = None;
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
    use database_mcp_server::pagination::Cursor;

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

// === read_query pagination (spec 034) ===

async fn collect_all_paged_read_query(handler: &PostgresHandler, query: &str) -> Vec<Value> {
    let mut all = Vec::new();
    let mut cursor: Option<database_mcp_server::pagination::Cursor> = None;
    loop {
        let request = ReadQueryRequest {
            query: query.into(),
            database: "app".into(),
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
            database: "app".into(),
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
            database: "app".into(),
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
            database: "app".into(),
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
            database: "app".into(),
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
    use database_mcp_server::pagination::Cursor;
    let handler = handler_with_page_size(2);
    let response = handler
        .read_query(ReadQueryRequest {
            query: "SELECT id FROM users ORDER BY id".into(),
            database: "app".into(),
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
async fn test_read_query_non_select_show_server_version_single_page() {
    // SHOW is classified as NonSelect; cursor must be ignored.
    use database_mcp_server::pagination::Cursor;
    let handler = handler_with_page_size(2);

    let without_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW server_version".into(),
            database: "app".into(),
            cursor: None,
        })
        .await
        .expect("SHOW server_version should succeed");

    let with_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW server_version".into(),
            database: "app".into(),
            cursor: Some(Cursor { offset: 100 }),
        })
        .await
        .expect("SHOW with cursor should succeed — cursor must be ignored");

    assert!(without_cursor.next_cursor.is_none());
    assert!(with_cursor.next_cursor.is_none());
    assert_eq!(
        without_cursor.rows, with_cursor.rows,
        "cursor must be silently ignored for non-SELECT statements"
    );
}

#[tokio::test]
async fn test_read_query_non_select_explain_single_page() {
    // EXPLAIN <query> is classified as NonSelect; cursor must be ignored.
    let handler = handler_with_page_size(2);

    let response = handler
        .read_query(ReadQueryRequest {
            query: "EXPLAIN SELECT 1".into(),
            database: "app".into(),
            cursor: None,
        })
        .await
        .expect("EXPLAIN should succeed");

    assert!(response.next_cursor.is_none(), "EXPLAIN must not paginate");
    assert!(!&response.rows.is_empty(), "EXPLAIN must return plan rows");
}

#[tokio::test]
async fn test_read_query_returns_non_null_temporal_columns() {
    // Feature 038: PG temporal columns must round-trip as RFC 3339 strings,
    // with TIMESTAMPTZ normalized to UTC and emitted with a trailing Z.
    let handler = handler(false);

    let response = handler
        .read_query(ReadQueryRequest {
            query: r#"SELECT "date", "time", "timestamp", "timestamptz" FROM temporal WHERE id = 1"#.into(),
            database: "app".into(),
            cursor: None,
        })
        .await
        .expect("temporal SELECT should succeed");

    let arr = &response.rows;
    assert_eq!(arr.len(), 1, "temporal seeds exactly one row");
    assert_eq!(arr[0]["date"], "2026-04-20", "DATE → YYYY-MM-DD");
    assert_eq!(arr[0]["time"], "14:30:00", "TIME → HH:MM:SS");
    assert_eq!(
        arr[0]["timestamp"], "2026-04-20T14:30:00",
        "TIMESTAMP (naive) → no Z, no offset (FR-004)"
    );
    assert_eq!(
        arr[0]["timestamptz"], "2026-04-20T12:30:00Z",
        "TIMESTAMPTZ → UTC-normalized from +02:00, with Z suffix (FR-004 / Q2)"
    );
}
