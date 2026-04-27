//! Functional tests for `PostgreSQL`.
//!
//! Tests exercise the handler methods directly, which is the same code
//! path the per-tool ZSTs delegate to.
//!
//! ```bash
//! ./tests/run.sh --filter postgres
//! ```

use dbmcp_config::{DatabaseBackend, DatabaseConfig};
use dbmcp_postgres::PostgresHandler;
use dbmcp_postgres::types::{DropTableRequest, ListTablesRequest, ListTriggersRequest};
use dbmcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, ListDatabasesRequest, ListFunctionsRequest,
    ListMaterializedViewsRequest, ListProceduresRequest, ListViewsRequest, QueryRequest, ReadQueryRequest,
};
use indexmap::IndexMap;
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

    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('Before', 'update@test.com')".into(),
        database: Some("app".into()),
    };
    handler.write_query(insert).await.unwrap();

    let update = QueryRequest {
        query: "UPDATE users SET name = 'After' WHERE email = 'update@test.com'".into(),
        database: Some("app".into()),
    };
    handler.write_query(update).await.unwrap();

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
    let tables = response.tables.as_brief().expect("brief mode").to_vec();

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
    let tables = response.tables.as_brief().expect("brief mode").to_vec();

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
async fn test_returns_error_for_nonexistent_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("nonexistent_db_xyz".into()),
        ..Default::default()
    };

    let response = handler.list_tables(request).await;

    assert!(response.is_err(), "Expected error for nonexistent database");
}

#[tokio::test]
async fn test_uses_default_pool_for_matching_database() {
    let handler = handler(false);
    let request = ListTablesRequest {
        database: Some("app".into()),
        ..Default::default()
    };

    let response = handler.list_tables(request).await.unwrap();
    let tables = response.tables.as_brief().expect("brief mode").to_vec();

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
async fn test_query_timeout_disabled_with_none() {
    let config = DatabaseConfig {
        query_timeout: None,
        ..base_db_config(false)
    };
    let handler = PostgresHandler::new(&config);
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
        query: "CREATE TABLE drop_test_simple (id SERIAL PRIMARY KEY)".into(),
        database: Some("app".into()),
    };
    handler.write_query(create).await.unwrap();

    // Drop it
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "drop_test_simple".into(),
        cascade: false,
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Verify it's gone
    let tables_request = ListTablesRequest {
        database: Some("app".into()),
        ..Default::default()
    };
    let response = handler.list_tables(tables_request).await.unwrap();
    let tables = response.tables.as_brief().expect("brief mode").to_vec();
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
        database: Some("app".into()),
    };
    handler.write_query(create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_child (id SERIAL PRIMARY KEY, parent_id INT REFERENCES drop_test_parent(id))"
            .into(),
        database: Some("app".into()),
    };
    handler.write_query(create_child).await.unwrap();

    // Attempt to drop parent without cascade — should fail
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "drop_test_parent".into(),
        cascade: false,
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
async fn test_drop_table_cascade() {
    let handler = handler(false);

    // Create parent and child tables with FK
    let create_parent = QueryRequest {
        query: "CREATE TABLE drop_test_cascade_parent (id SERIAL PRIMARY KEY)".into(),
        database: Some("app".into()),
    };
    handler.write_query(create_parent).await.unwrap();

    let create_child = QueryRequest {
        query: "CREATE TABLE drop_test_cascade_child (id SERIAL PRIMARY KEY, parent_id INT REFERENCES drop_test_cascade_parent(id))".into(),
        database: Some("app".into()),
    };
    handler.write_query(create_child).await.unwrap();

    // Drop parent with cascade — should succeed
    let drop_request = DropTableRequest {
        database: Some("app".into()),
        table: "drop_test_cascade_parent".into(),
        cascade: true,
    };
    let response = handler.drop_table(drop_request).await.unwrap();
    assert!(response.message.contains("dropped successfully"));

    // Clean up child table (still exists, just lost FK constraint)
    let cleanup = QueryRequest {
        query: "DROP TABLE IF EXISTS drop_test_cascade_child".into(),
        database: Some("app".into()),
    };
    handler.write_query(cleanup).await.unwrap();
}

#[tokio::test]
async fn test_drop_table_nonexistent() {
    let handler = handler(false);
    let drop_request = DropTableRequest {
        database: Some("app".into()),
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
        database: Some("app".into()),
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
        database: Some("app".into()),
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
        database: Some("app".into()),
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
async fn test_explain_query_plain_write_allowed() {
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
    let names = response.tables.as_brief().expect("brief mode");
    assert!(
        names.iter().any(|t| t == "users"),
        "expected default-database tables, got {names:?}"
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
    let names = response.tables.as_brief().expect("brief mode");
    assert!(
        names.iter().any(|t| t == "users"),
        "expected default-database tables, got {names:?}"
    );
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
async fn test_drop_table_cross_database() {
    let handler = handler(false);

    // Create a table in the analytics database
    let create = QueryRequest {
        query: "CREATE TABLE drop_cross_test (id SERIAL PRIMARY KEY)".into(),
        database: Some("analytics".into()),
    };
    handler.write_query(create).await.unwrap();

    // Drop it from the analytics database
    let drop_request = DropTableRequest {
        database: Some("analytics".into()),
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
        query: "SELECT * FROM users WHERE id IN (SELECT user_id FROM posts WHERE published = true)".into(),
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
        query: "CREATE TABLE drop_default_pg (id SERIAL PRIMARY KEY)".into(),
        database: Some("app".into()),
    };
    handler.write_query(create).await.expect("seed table");

    let drop_request = DropTableRequest {
        database: Some(String::new()),
        table: "drop_default_pg".into(),
        cascade: false,
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
        cascade: false,
    };

    let response = handler.drop_table(drop_request).await;
    assert!(response.is_err(), "drop_table should be blocked in read-only mode");
}

#[tokio::test]
async fn test_read_query_control_char_database_name_rejected() {
    let handler = handler(true);
    let request = ReadQueryRequest {
        query: "SELECT 1".into(),
        database: Some("test\x01db".into()),
        cursor: None,
    };
    let result = handler.read_query(request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

#[tokio::test]
async fn test_list_tables_control_char_database_rejected() {
    let handler = handler(true);
    let request = ListTablesRequest {
        database: Some("test\x00db".into()),
        ..Default::default()
    };
    let result = handler.list_tables(request).await;
    assert!(result.is_err(), "control char in database name should be rejected");
}

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

const PG_DB: &str = "app";

async fn collect_all_paged(handler: &PostgresHandler) -> Vec<String> {
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let request = ListTablesRequest {
            database: Some(PG_DB.into()),
            cursor,
            ..Default::default()
        };
        let response = handler.list_tables(request).await.expect("list page");
        all.extend(response.tables.as_brief().expect("brief mode").iter().cloned());
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
            database: Some(PG_DB.into()),
            ..Default::default()
        })
        .await
        .expect("single page");

    let single_page_names = single_page.tables.as_brief().expect("brief mode").to_vec();
    assert_eq!(
        collected, single_page_names,
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
            database: Some(PG_DB.into()),
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
            database: Some(PG_DB.into()),
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
            database: Some(PG_DB.into()),
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
        database: Some(PG_DB.into()),
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
            database: Some(PG_DB.into()),
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
            database: Some(PG_DB.into()),
            ..Default::default()
        })
        .await
        .expect("first page");
    assert_eq!(first.tables.len(), 1, "page_size=1 must return one table per page");
    assert!(first.next_cursor.is_some(), "page 1 must emit nextCursor");
}

async fn collect_all_paged_databases(handler: &PostgresHandler) -> Vec<String> {
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

async fn collect_all_paged_read_query(handler: &PostgresHandler, query: &str) -> Vec<Value> {
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
async fn test_read_query_non_select_show_server_version_single_page() {
    // SHOW is classified as NonSelect; cursor must be ignored.
    use dbmcp_server::pagination::Cursor;
    let handler = handler_with_page_size(2);

    let without_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW server_version".into(),
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("SHOW server_version should succeed");

    let with_cursor = handler
        .read_query(ReadQueryRequest {
            query: "SHOW server_version".into(),
            database: Some("app".into()),
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
            database: Some("app".into()),
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
            database: Some("app".into()),
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

#[tokio::test]
async fn test_list_views_returns_seeded_views() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_views");

    assert!(
        response.views.contains(&"active_users".to_string()),
        "expected seeded active_users view, got {:?}",
        response.views
    );
    assert!(
        response.views.contains(&"published_posts".to_string()),
        "expected seeded published_posts view, got {:?}",
        response.views
    );
}

#[tokio::test]
async fn test_list_views_excludes_base_tables() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_views");

    for table in ["users", "posts", "tags", "post_tags", "temporal"] {
        assert!(
            !response.views.contains(&table.to_string()),
            "base table `{table}` must not appear in listViews, got {:?}",
            response.views
        );
    }
}

#[tokio::test]
async fn test_list_views_empty_for_view_less_database() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("analytics".into()),
            cursor: None,
        })
        .await
        .expect("list_views");

    assert!(
        response.views.is_empty(),
        "analytics has no views, got {:?}",
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
        };
        let response = handler_paged.list_views(request).await.expect("paged list_views");
        all.extend(response.views);
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    let single = handler_full
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("single-page list_views");

    assert_eq!(all, single.views, "paginated traversal should equal single page");
}

#[tokio::test]
async fn test_list_views_works_in_read_only_mode() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_views in read-only mode");

    assert!(!response.views.is_empty(), "read-only mode must still allow listViews");
}

#[tokio::test]
async fn test_list_triggers_returns_seeded_triggers() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
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
async fn test_list_triggers_excludes_internal_triggers() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let names = response.triggers.as_brief().expect("brief mode");
    // RI_ConstraintTrigger_* are the internal triggers backing FK constraints.
    for trg in names {
        assert!(
            !trg.starts_with("RI_ConstraintTrigger"),
            "internal FK trigger {trg} leaked into listTriggers output: {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_triggers_empty_for_trigger_less_database() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("analytics".into()),
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let names = response.triggers.as_brief().expect("brief mode");
    assert!(names.is_empty(), "analytics has no user triggers, got {names:?}");
}

#[tokio::test]
async fn test_list_triggers_works_in_read_only_mode() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
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
    assert_eq!(names, &["orders_audit_trigger".to_string()], "got {names:?}");
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
    // Mirrors the `listTables` contract: ILIKE wildcards (`%`, `_`) are exposed
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
    for payload in ["'", ";", "--", "\\", "%", "_"] {
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
    let paged = handler_with_page_size(1);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                cursor,
                search: Some("before".into()),
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
            search: Some("before".into()),
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
async fn test_list_triggers_brief_without_parameters_returns_bare_strings() {
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
        triggers.as_array().unwrap().iter().all(serde_json::Value::is_string),
        "expected every brief-mode entry to be a JSON string, got {triggers:?}"
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_returns_full_metadata_for_orders_audit() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("orders_audit_trigger".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");

    let map = response.triggers.as_detailed().expect("detailed mode");
    let entry = map.get("orders_audit_trigger").expect("orders_audit_trigger entry");
    assert_eq!(entry["schema"], serde_json::json!("public"));
    assert_eq!(entry["table"], serde_json::json!("orders"));
    assert_eq!(entry["status"], serde_json::json!("ENABLED"));
    assert_eq!(entry["timing"], serde_json::json!("AFTER"));
    assert_eq!(entry["events"], serde_json::json!(["INSERT", "UPDATE"]));
    assert_eq!(entry["activationLevel"], serde_json::json!("ROW"));
    assert_eq!(entry["functionName"], serde_json::json!("orders_audit_fn"));
    assert!(
        entry["definition"]
            .as_str()
            .expect("definition string")
            .starts_with("CREATE TRIGGER orders_audit_trigger"),
        "definition shape: {:?}",
        entry["definition"]
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_disabled_status() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("block_inventory_delete".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");
    let map = response.triggers.as_detailed().expect("detailed mode");
    let entry = map.get("block_inventory_delete").expect("entry");
    assert_eq!(entry["status"], serde_json::json!("DISABLED"));
    assert_eq!(entry["timing"], serde_json::json!("BEFORE"));
    assert_eq!(entry["events"], serde_json::json!(["DELETE"]));
    assert_eq!(entry["activationLevel"], serde_json::json!("STATEMENT"));
}

#[tokio::test]
async fn test_list_triggers_detailed_partitioned_parent() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            search: Some("logs_redact_before_insert".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");
    let map = response.triggers.as_detailed().expect("detailed mode");
    let entry = map.get("logs_redact_before_insert").expect("entry");
    assert_eq!(entry["table"], serde_json::json!("logs"));
    assert_eq!(entry["timing"], serde_json::json!("BEFORE"));
    assert_eq!(entry["events"], serde_json::json!(["INSERT"]));
    assert_eq!(entry["activationLevel"], serde_json::json!("ROW"));
}

#[tokio::test]
async fn test_list_triggers_detailed_with_search_only_includes_filtered() {
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

    let map = response.triggers.as_detailed().expect("detailed");
    assert!(map.contains_key("orders_audit_trigger"));
    assert!(!map.contains_key("block_inventory_delete"));
    assert!(!map.contains_key("logs_redact_before_insert"));
}

#[tokio::test]
async fn test_list_triggers_detailed_paginates() {
    let paged = handler_with_page_size(1);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_triggers(ListTriggersRequest {
                database: Some("app".into()),
                cursor,
                detailed: true,
                ..Default::default()
            })
            .await
            .expect("list_triggers paginated");

        let map = response.triggers.as_detailed().expect("detailed");
        assert!(map.len() <= 1, "page exceeded page_size=1: {} entries", map.len());
        all.extend(map.keys().cloned());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }

    let single = handler(true)
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("single-page brief");
    assert_eq!(
        all,
        single.triggers.as_brief().expect("brief").to_vec(),
        "paginated detailed traversal must walk every trigger in brief order"
    );
}

#[tokio::test]
async fn test_list_triggers_detailed_internal_triggers_excluded() {
    let handler = handler(true);
    let response = handler
        .list_triggers(ListTriggersRequest {
            database: Some("app".into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_triggers");
    let map = response.triggers.as_detailed().expect("detailed");
    for name in map.keys() {
        assert!(
            !name.starts_with("RI_ConstraintTrigger"),
            "internal FK trigger leaked: {name}"
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
        })
        .await
        .expect("list_functions");

    assert!(
        response.functions.contains(&"calc_total".to_string()),
        "expected seeded calc_total function, got {:?}",
        response.functions
    );
    assert!(
        response.functions.contains(&"double_it".to_string()),
        "expected seeded double_it function, got {:?}",
        response.functions
    );
}

#[tokio::test]
async fn test_list_functions_excludes_procedures() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_functions");

    for proc_name in ["archive_user", "touch_post"] {
        assert!(
            !response.functions.contains(&proc_name.to_string()),
            "procedure `{proc_name}` leaked into listFunctions output: {:?}",
            response.functions
        );
    }
}

#[tokio::test]
async fn test_list_procedures_returns_seeded_procedures() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_procedures");

    assert!(
        response.procedures.contains(&"archive_user".to_string()),
        "expected seeded archive_user procedure, got {:?}",
        response.procedures
    );
    assert!(
        response.procedures.contains(&"touch_post".to_string()),
        "expected seeded touch_post procedure, got {:?}",
        response.procedures
    );
}

#[tokio::test]
async fn test_list_procedures_excludes_functions() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_procedures");

    for func_name in ["calc_total", "double_it"] {
        assert!(
            !response.procedures.contains(&func_name.to_string()),
            "function `{func_name}` leaked into listProcedures output: {:?}",
            response.procedures
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_returns_seeded_matviews() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_materialized_views");

    assert!(
        response.materialized_views.contains(&"mv_recent_orders".to_string()),
        "expected seeded mv_recent_orders matview, got {:?}",
        response.materialized_views
    );
    assert!(
        response.materialized_views.contains(&"mv_user_cohort".to_string()),
        "expected seeded mv_user_cohort matview, got {:?}",
        response.materialized_views
    );
}

#[tokio::test]
async fn test_list_views_excludes_materialized_views() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_views");

    for matview in ["mv_recent_orders", "mv_user_cohort"] {
        assert!(
            !response.views.contains(&matview.to_string()),
            "materialized view `{matview}` leaked into listViews output: {:?}",
            response.views
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_excludes_regular_views() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            cursor: None,
        })
        .await
        .expect("list_materialized_views");

    for view in ["active_users", "published_posts"] {
        assert!(
            !response.materialized_views.contains(&view.to_string()),
            "regular view `{view}` leaked into listMaterializedViews output: {:?}",
            response.materialized_views
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_empty_for_empty_database() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("analytics".into()),
            cursor: None,
        })
        .await
        .expect("list_materialized_views");

    assert!(
        response.materialized_views.is_empty(),
        "analytics has no matviews, got {:?}",
        response.materialized_views
    );
}

/// Returns a cloned Vec<String> of matched table names for a given search term.
async fn search_names(handler: &PostgresHandler, search: &str) -> Vec<String> {
    let request = ListTablesRequest {
        database: Some(PG_DB.into()),
        search: Some(search.into()),
        ..Default::default()
    };
    let response = handler.list_tables(request).await.expect("listTables ok");
    response.tables.as_brief().expect("brief mode").to_vec()
}

#[tokio::test]
async fn test_list_tables_search_filter_returns_only_matches() {
    let handler = handler(true);
    let names = search_names(&handler, "order").await;
    assert_eq!(
        names,
        vec!["erp_orders", "order_items", "orders"],
        "search 'order' must return only matching tables, alphabetically"
    );
}

#[tokio::test]
async fn test_list_tables_search_is_case_insensitive() {
    let handler = handler(true);
    assert_eq!(
        search_names(&handler, "order").await,
        search_names(&handler, "ORDER").await,
        "ILIKE must be case-insensitive"
    );
}

#[tokio::test]
async fn test_list_tables_search_no_match_returns_empty() {
    let handler = handler(true);
    let names = search_names(&handler, "nonexistent_concept_xyz").await;
    assert!(names.is_empty(), "no match must return empty, got {names:?}");
}

#[tokio::test]
async fn test_list_tables_search_supports_like_wildcards() {
    // `_` is the LIKE single-char wildcard — `_rder` must match any name
    // with one char + `rder` (e.g. `orders`, `order_items`, `erp_orders`).
    let handler = handler(true);
    let names = search_names(&handler, "_rder").await;
    assert_eq!(names, vec!["erp_orders", "order_items", "orders"]);
}

#[tokio::test]
async fn test_list_tables_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let with_empty = search_names(&handler, "").await;
    let without = handler
        .list_tables(ListTablesRequest {
            database: Some(PG_DB.into()),
            ..Default::default()
        })
        .await
        .expect("listTables ok")
        .tables
        .as_brief()
        .expect("brief")
        .to_vec();
    assert_eq!(with_empty, without, "empty search must behave like no filter");
}

#[tokio::test]
async fn test_list_tables_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\", "%", "_"] {
        let result = handler
            .list_tables(ListTablesRequest {
                database: Some(PG_DB.into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await;
        assert!(
            result.is_ok(),
            "adversarial search payload {payload:?} must not raise a SQL error: {result:?}"
        );
    }
}

#[tokio::test]
async fn test_list_tables_search_paginates_filtered_results() {
    let handler = handler_with_page_size(2);
    // Collect every page with the filter held constant.
    let mut collected: Vec<String> = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = handler
            .list_tables(ListTablesRequest {
                database: Some(PG_DB.into()),
                cursor,
                search: Some("order".into()),
                detailed: false,
            })
            .await
            .expect("page");
        collected.extend(response.tables.as_brief().expect("brief mode").iter().cloned());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(
        collected,
        vec!["erp_orders", "order_items", "orders"],
        "filter must hold across pages"
    );
}

/// Returns the keyed detailed-mode map (table name → metadata) for a search term.
async fn detailed_entries(handler: &PostgresHandler, search: &str) -> IndexMap<String, Value> {
    let request = ListTablesRequest {
        database: Some(PG_DB.into()),
        search: Some(search.into()),
        detailed: true,
        ..Default::default()
    };
    let response = handler.list_tables(request).await.expect("listTables detailed ok");
    response.tables.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_tables_detailed_returns_full_metadata_for_orders() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "orders").await;
    let orders = entries.get("orders").expect("orders entry present");
    assert_eq!(orders["kind"], "TABLE");
    assert_eq!(orders["schema"], "public");

    // Columns — check a few by name.
    let columns = orders["columns"].as_array().expect("columns array");
    let col_names: Vec<_> = columns
        .iter()
        .filter_map(|c| c["name"].as_str().map(String::from))
        .collect();
    for expected in ["id", "customer_id", "total", "status", "created_at"] {
        assert!(col_names.contains(&expected.into()), "missing column: {expected}");
    }
    let status = columns.iter().find(|c| c["name"] == "status").expect("status column");
    assert_eq!(status["nullable"], false);

    // Constraints — PRIMARY KEY, FOREIGN KEY, CHECK at minimum.
    let constraints = orders["constraints"].as_array().expect("constraints array");
    let types: Vec<_> = constraints
        .iter()
        .filter_map(|c| c["type"].as_str().map(String::from))
        .collect();
    assert!(types.contains(&"PRIMARY KEY".into()), "no PRIMARY KEY: {types:?}");
    assert!(types.contains(&"FOREIGN KEY".into()), "no FOREIGN KEY: {types:?}");
    assert!(types.contains(&"CHECK".into()), "no CHECK: {types:?}");
    let fk = constraints.iter().find(|c| c["type"] == "FOREIGN KEY").expect("FK row");
    assert_eq!(fk["referencedTable"], "customers");
    assert_eq!(fk["referencedColumns"].as_array().expect("refcols").len(), 1);

    // Indexes — primary + secondary.
    let indexes = orders["indexes"].as_array().expect("indexes array");
    let names: Vec<_> = indexes
        .iter()
        .filter_map(|i| i["name"].as_str().map(String::from))
        .collect();
    assert!(names.iter().any(|n| n == "orders_pkey"), "no primary index: {names:?}");
    assert!(
        names.iter().any(|n| n == "orders_customer_created_idx"),
        "no secondary index: {names:?}"
    );

    // Triggers.
    let triggers = orders["triggers"].as_array().expect("triggers array");
    let trigger_names: Vec<_> = triggers
        .iter()
        .filter_map(|t| t["name"].as_str().map(String::from))
        .collect();
    assert!(
        trigger_names.iter().any(|n| n == "orders_audit_trigger"),
        "no trigger: {trigger_names:?}"
    );
}

#[tokio::test]
async fn test_list_tables_detailed_distinguishes_partitioned_tables() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "logs").await;
    let logs = entries.get("logs").expect("logs entry present");
    assert_eq!(logs["kind"], "PARTITIONED_TABLE");
}

#[tokio::test]
async fn test_list_tables_detailed_empty_arrays_when_no_metadata() {
    let handler = handler(true);
    // `erp_orders` has a PK and an external_ref column but no trigger, no non-PK constraints,
    // and no secondary indexes — exercises the empty-array `COALESCE` paths in the CTE.
    let entries = detailed_entries(&handler, "erp_orders").await;
    let entry = entries.get("erp_orders").expect("erp_orders entry present");
    assert!(entry["triggers"].is_array(), "triggers must be array");
    assert!(entry["constraints"].is_array(), "constraints must be array");
    assert!(entry["indexes"].is_array(), "indexes must be array");
    assert!(entry["columns"].is_array(), "columns must be array");
    assert_eq!(
        entry["triggers"].as_array().expect("array").len(),
        0,
        "erp_orders has no triggers"
    );
}

#[tokio::test]
async fn test_list_tables_brief_without_parameters_returns_bare_strings() {
    let handler = handler(true);
    let response = handler
        .list_tables(ListTablesRequest {
            database: Some(PG_DB.into()),
            ..Default::default()
        })
        .await
        .expect("list");
    let value = serde_json::to_value(&response.tables).expect("serialize");
    assert!(
        value
            .as_array()
            .expect("array")
            .first()
            .expect("at least one")
            .is_string(),
        "brief-mode entries must be strings on the wire, got {value:?}"
    );
}

#[tokio::test]
async fn test_list_tables_detailed_with_search_only_fetches_filtered_subset() {
    let handler = handler(true);
    // "order" matches three tables: erp_orders, order_items, orders.
    let entries = detailed_entries(&handler, "order").await;
    let names: Vec<_> = entries.keys().cloned().collect();
    assert_eq!(
        names,
        vec!["erp_orders", "order_items", "orders"],
        "detailed search must return only filtered subset"
    );
}

#[tokio::test]
async fn test_list_tables_detailed_values_have_no_redundant_name_field() {
    let handler = handler(true);
    let entries = detailed_entries(&handler, "order").await;
    assert!(!entries.is_empty(), "fixture must yield at least one match for 'order'");
    for (key, value) in &entries {
        assert!(value.is_object(), "value for {key} must be a JSON object: {value}");
        assert!(
            value.get("name").is_none(),
            "FR-002 violated: value for {key} still carries 'name': {value}"
        );
    }
}

#[tokio::test]
async fn test_list_tables_detailed_paginates() {
    let handler = handler_with_page_size(1);
    let mut collected_names: Vec<String> = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = handler
            .list_tables(ListTablesRequest {
                database: Some(PG_DB.into()),
                cursor,
                search: Some("order".into()),
                detailed: true,
            })
            .await
            .expect("page");
        let entries = response.tables.as_detailed().expect("detailed");
        assert!(entries.len() <= 1, "page_size=1 must cap to 1 per page");
        collected_names.extend(entries.keys().cloned());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    assert_eq!(
        collected_names,
        vec!["erp_orders", "order_items", "orders"],
        "detailed pagination must yield filtered sequence"
    );
}

#[tokio::test]
async fn test_list_tables_detailed_key_order_matches_brief_order() {
    let handler = handler(true);
    let brief = handler
        .list_tables(ListTablesRequest {
            database: Some(PG_DB.into()),
            search: Some("order".into()),
            detailed: false,
            ..Default::default()
        })
        .await
        .expect("listTables brief ok");
    let detailed = detailed_entries(&handler, "order").await;
    let brief_names: Vec<String> = brief.tables.as_brief().expect("brief mode").to_vec();
    let detailed_keys: Vec<String> = detailed.keys().cloned().collect();
    assert_eq!(
        brief_names, detailed_keys,
        "detailed key order must match brief string order — FR-003"
    );
}
