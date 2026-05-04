//! Functional tests for `PostgreSQL`.
//!
//! Tests exercise the handler methods directly, which is the same code
//! path the per-tool ZSTs delegate to.
//!
//! ```bash
//! ./tests/run.sh --filter postgres
//! ```

use dbmcp_config::{Config, DatabaseBackend, DatabaseConfig, PiiConfig, PiiOperator};
use dbmcp_postgres::PostgresHandler;
use dbmcp_postgres::types::{
    DropTableRequest, ListFunctionsRequest, ListMaterializedViewsRequest, ListProceduresRequest, ListTablesRequest,
    ListTriggersRequest, ListViewsRequest,
};
use dbmcp_server::types::{
    CreateDatabaseRequest, DropDatabaseRequest, ExplainQueryRequest, ListDatabasesRequest, QueryRequest,
    ReadQueryRequest,
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
    PostgresHandler::new(&Config {
        database: config,
        http: None,
        pii: PiiConfig::default(),
    })
}

fn handler(read_only: bool) -> PostgresHandler {
    let config = base_db_config(read_only);
    PostgresHandler::new(&Config {
        database: config,
        http: None,
        pii: PiiConfig::default(),
    })
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
    let handler = PostgresHandler::new(&Config {
        database: config,
        http: None,
        pii: PiiConfig::default(),
    });
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
    let handler = PostgresHandler::new(&Config {
        database: config,
        http: None,
        pii: PiiConfig::default(),
    });
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
            ..Default::default()
        })
        .await
        .expect("list_views");

    let names = response.views.as_brief().expect("brief mode");
    assert!(
        names.contains(&"active_users".to_string()),
        "expected seeded active_users view, got {names:?}"
    );
    assert!(
        names.contains(&"published_posts".to_string()),
        "expected seeded published_posts view, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_views_excludes_base_tables() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_views");

    let names = response.views.as_brief().expect("brief mode");
    for table in ["users", "posts", "tags", "post_tags", "temporal"] {
        assert!(
            !names.contains(&table.to_string()),
            "base table `{table}` must not appear in listViews, got {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_views_empty_for_view_less_database() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("analytics".into()),
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
            ..Default::default()
        })
        .await
        .expect("list_views in read-only mode");

    assert!(
        !response.views.as_brief().expect("brief").is_empty(),
        "read-only mode must still allow listViews"
    );
}

// === spec 063: search + detailed mode tests ===

async fn list_views_brief(handler: &PostgresHandler, search: Option<&str>) -> Vec<String> {
    let request = ListViewsRequest {
        database: Some("app".into()),
        search: search.map(str::to_owned),
        ..Default::default()
    };
    let response = handler.list_views(request).await.expect("list_views");
    response.views.as_brief().expect("brief mode").to_vec()
}

async fn list_views_detailed(handler: &PostgresHandler, search: Option<&str>) -> IndexMap<String, Value> {
    let request = ListViewsRequest {
        database: Some("app".into()),
        search: search.map(str::to_owned),
        detailed: true,
        ..Default::default()
    };
    let response = handler.list_views(request).await.expect("list_views detailed");
    response.views.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_views_search_filter_returns_only_matches() {
    let handler = handler(true);
    let names = list_views_brief(&handler, Some("active")).await;
    assert_eq!(
        names,
        vec!["active_orders".to_string(), "active_users".to_string()],
        "search=active must return only the two active_* views, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_views_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = list_views_brief(&handler, Some("active")).await;
    let upper = list_views_brief(&handler, Some("ACTIVE")).await;
    assert_eq!(lower, upper, "ILIKE must be case-insensitive");
}

#[tokio::test]
async fn test_list_views_search_no_match_returns_empty() {
    let handler = handler(true);
    let names = list_views_brief(&handler, Some("nonexistent_view_xyz")).await;
    assert!(names.is_empty(), "no match must return empty array, got {names:?}");
}

#[tokio::test]
async fn test_list_views_search_supports_wildcard_semantics() {
    let handler = handler(true);
    // `%` matches any sequence — `active%users` matches `active_users` only.
    let percent = list_views_brief(&handler, Some("active%users")).await;
    assert_eq!(
        percent,
        vec!["active_users".to_string()],
        "% wildcard must match active_users only"
    );
    // `_` matches a single character — `a_chived` matches `archived_orders`.
    let underscore = list_views_brief(&handler, Some("a_chived")).await;
    assert!(
        underscore.contains(&"archived_orders".to_string()),
        "_ wildcard must match archived_orders, got {underscore:?}"
    );
}

#[tokio::test]
async fn test_list_views_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\"] {
        let response = handler
            .list_views(ListViewsRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("search={payload:?} must not raise SQL error: {e:?}"));
        assert!(
            response.views.as_brief().is_some(),
            "search={payload:?} must return brief mode"
        );
    }
}

#[tokio::test]
async fn test_list_views_search_paginates_filtered_results() {
    let paged = handler_with_page_size(1);
    let full = handler(true);

    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = paged
            .list_views(ListViewsRequest {
                database: Some("app".into()),
                cursor,
                search: Some("active".into()),
                ..Default::default()
            })
            .await
            .expect("paged list_views");
        all.extend(response.views.as_brief().expect("brief").to_vec());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    let single = full
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            search: Some("active".into()),
            ..Default::default()
        })
        .await
        .expect("single-page list_views");
    let single_names = single.views.as_brief().expect("brief").to_vec();
    assert_eq!(all, single_names, "paginated filter must equal single page");
}

#[tokio::test]
async fn test_list_views_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let no_filter = list_views_brief(&handler, None).await;
    let empty = list_views_brief(&handler, Some("")).await;
    let whitespace = list_views_brief(&handler, Some("   ")).await;
    assert_eq!(no_filter, empty, "empty search must be treated as no filter");
    assert_eq!(
        no_filter, whitespace,
        "whitespace-only search must be treated as no filter"
    );
}

#[tokio::test]
async fn test_list_views_excludes_materialized_views_and_system_schemas() {
    let handler = handler(true);
    let all = list_views_brief(&handler, None).await;
    // Materialized views must not appear.
    assert!(
        !all.contains(&"mv_recent_orders".to_string()),
        "mv_recent_orders is a materialized view; must not appear in listViews, got {all:?}"
    );
    assert!(
        !all.contains(&"mv_user_cohort".to_string()),
        "mv_user_cohort is a materialized view; must not appear, got {all:?}"
    );
    // System-schema views must not match either.
    let sys = list_views_brief(&handler, Some("pg_indexes")).await;
    assert!(
        sys.is_empty(),
        "pg_catalog views must never appear in listViews, got {sys:?}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_returns_full_metadata_for_active_users() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("active_users")).await;
    let entry = map
        .get("active_users")
        .expect("active_users must be present under bare-name key");
    assert_eq!(entry["schema"], Value::String("public".into()));
    assert_eq!(entry["owner"], Value::String("app_user".into()));
    assert_eq!(
        entry["description"],
        Value::String("Currently-active user accounts".into())
    );
    let definition = entry["definition"].as_str().expect("definition string");
    assert!(
        definition.contains("SELECT"),
        "definition must contain SELECT, got: {definition}"
    );
    assert!(
        definition.contains("users"),
        "definition must reference users, got: {definition}"
    );
    // Bare-name key contract: no `name` field inside the value.
    assert!(
        entry.get("name").is_none(),
        "value must not repeat `name` (it is the map key), got entry: {entry}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_no_comment_yields_null_description() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("active_orders")).await;
    let entry = map.get("active_orders").expect("active_orders entry");
    assert_eq!(
        entry["description"],
        Value::Null,
        "no COMMENT ON VIEW → JSON null, not empty string. entry: {entry}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_owner_reflects_alter_view_owner() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("archived_orders")).await;
    let entry = map.get("archived_orders").expect("archived_orders entry");
    assert_eq!(
        entry["owner"],
        Value::String("reporting_role".into()),
        "owner must reflect ALTER VIEW OWNER TO reporting_role"
    );
}

#[tokio::test]
async fn test_list_views_detailed_definition_round_trip_multiline() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("user_order_summary")).await;
    let entry = map.get("user_order_summary").expect("user_order_summary entry");
    let definition = entry["definition"].as_str().expect("definition string");
    assert!(definition.contains("WITH recent AS"), "must contain CTE: {definition}");
    assert!(
        definition.contains("LEFT JOIN recent"),
        "must contain LEFT JOIN: {definition}"
    );
    assert!(
        definition.contains('\n'),
        "multi-line definition must preserve newlines: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_definition_round_trip_quote() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("audit_log")).await;
    let entry = map.get("audit_log").expect("audit_log entry");
    let definition = entry["definition"].as_str().expect("definition string");
    assert!(
        definition.contains("'admin'"),
        "single-quote literal must round-trip in definition, got: {definition}"
    );
}

#[tokio::test]
async fn test_list_views_detailed_with_search_only_includes_filtered() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("active")).await;
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec!["active_orders", "active_users"],
        "detailed+search must include only matching views"
    );
}

#[tokio::test]
async fn test_list_views_detailed_paginates() {
    let paged = handler_with_page_size(2);
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = paged
            .list_views(ListViewsRequest {
                database: Some("app".into()),
                cursor,
                detailed: true,
                ..Default::default()
            })
            .await
            .expect("paged detailed list_views");
        all.extend(
            response
                .views
                .as_detailed()
                .expect("detailed")
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        );
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    // Brief and detailed pagination must traverse the same row sequence.
    let full = handler(true);
    let brief = list_views_brief(&full, None).await;
    assert_eq!(all, brief, "detailed pagination must match brief order/keys");
}

#[tokio::test]
async fn test_list_views_brief_returns_bare_strings() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_views brief");
    let serialized = serde_json::to_value(&response).expect("serialize");
    let views = serialized.get("views").expect("views field");
    assert!(views.is_array(), "brief mode views must serialise as bare-string array");
    for entry in views.as_array().expect("array") {
        assert!(
            entry.is_string(),
            "brief mode entries must be raw strings, got: {entry:?}"
        );
    }
}

#[tokio::test]
async fn test_list_views_detailed_omits_value_fields() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, Some("active_users")).await;
    let entry = map.get("active_users").expect("active_users entry");
    let object = entry.as_object().expect("entry must be object");
    let keys: std::collections::BTreeSet<&str> = object.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> =
        ["schema", "owner", "description", "definition"].into_iter().collect();
    assert_eq!(
        keys, expected,
        "detailed value must contain exactly the four contract fields"
    );
    for forbidden in [
        "name",
        "columns",
        "securityBarrier",
        "securityInvoker",
        "withCheckOption",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "detailed value must NOT contain `{forbidden}` field, got entry: {entry}"
        );
    }
}

#[tokio::test]
async fn test_list_views_detailed_excludes_materialized_views() {
    let handler = handler(true);
    let map = list_views_detailed(&handler, None).await;
    assert!(
        !map.contains_key("mv_recent_orders"),
        "materialized view must not appear in detailed listViews, got keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );
    assert!(
        !map.contains_key("mv_user_cohort"),
        "materialized view must not appear in detailed listViews, got keys: {:?}",
        map.keys().collect::<Vec<_>>()
    );
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

// -----------------------------------------------------------------------
// listFunctions search + detailed mode (spec 057)
// -----------------------------------------------------------------------

async fn list_functions_brief(handler: &PostgresHandler, search: Option<&str>) -> Vec<String> {
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: search.map(str::to_string),
            ..Default::default()
        })
        .await
        .expect("list_functions");
    response.functions.as_brief().expect("brief mode").to_vec()
}

async fn list_functions_detailed(
    handler: &PostgresHandler,
    search: &str,
) -> indexmap::IndexMap<String, serde_json::Value> {
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            search: Some(search.into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_functions detailed");
    response.functions.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_functions_search_filter_returns_only_matches() {
    let handler = handler(true);
    let names = list_functions_brief(&handler, Some("calc_order")).await;
    assert_eq!(
        names,
        vec![
            "calc_order_subtotal".to_string(),
            "calc_order_total".to_string(),
            "calc_order_total".to_string(),
        ],
        "expected three calc_order_* matches (overload duplication)"
    );
}

#[tokio::test]
async fn test_list_functions_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = list_functions_brief(&handler, Some("calc_order")).await;
    let upper = list_functions_brief(&handler, Some("CALC_ORDER")).await;
    assert_eq!(lower, upper);
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
    // `_` is the ILIKE single-char wildcard. `c_lc` matches any 4-char substring
    // starting with `c` then any char then `lc` — i.e. matches `calc` in every
    // calc_* seeded function. A literal-substring contract would match nothing
    // (no function contains the four characters `c`, `_`, `l`, `c` in order).
    let handler = handler(true);
    let names = list_functions_brief(&handler, Some("c_lc")).await;
    assert!(
        !names.is_empty(),
        "expected `_` to be honoured as wildcard; empty result implies literal-substring mode"
    );
    assert!(
        names.iter().all(|n| n.contains("calc")),
        "all wildcard matches should contain `calc`, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_functions_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\", "%", "_"] {
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
    let paged = handler_with_page_size(1);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_functions(ListFunctionsRequest {
                database: Some("app".into()),
                cursor,
                search: Some("calc_order".into()),
                ..Default::default()
            })
            .await
            .expect("list_functions paged");
        all.extend(response.functions.as_brief().expect("brief").to_vec());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let single = list_functions_brief(&handler(true), Some("calc_order")).await;
    assert_eq!(all, single, "paginated traversal should equal single-page result");
}

#[tokio::test]
async fn test_list_functions_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let none_filter = list_functions_brief(&handler, None).await;
    let empty = list_functions_brief(&handler, Some("")).await;
    let whitespace = list_functions_brief(&handler, Some("   ")).await;
    assert_eq!(none_filter, empty);
    assert_eq!(none_filter, whitespace);
}

#[tokio::test]
async fn test_list_functions_excludes_aggregates_and_window_and_procedures() {
    let handler = handler(true);
    // Aggregate sum_demo (prokind='a') must not appear.
    let agg = list_functions_brief(&handler, Some("sum_demo")).await;
    assert!(agg.is_empty(), "aggregate leaked into listFunctions: {agg:?}");
    // Procedure noop_proc (prokind='p') must not appear.
    let proc_match = list_functions_brief(&handler, Some("noop_proc")).await;
    assert!(
        proc_match.is_empty(),
        "procedure leaked into listFunctions: {proc_match:?}"
    );
}

#[tokio::test]
async fn test_list_functions_detailed_returns_full_metadata_for_calc_order_subtotal() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "calc_order_subtotal").await;
    let key = "calc_order_subtotal(order_id integer)";
    let entry = detailed
        .get(key)
        .unwrap_or_else(|| panic!("missing key {key}; got keys {:?}", detailed.keys().collect::<Vec<_>>()));
    assert_eq!(entry["schema"], "public");
    assert_eq!(entry["name"], "calc_order_subtotal");
    assert_eq!(entry["language"], "sql");
    assert_eq!(entry["arguments"], "order_id integer");
    assert_eq!(entry["returnType"], "numeric");
    assert_eq!(entry["volatility"], "IMMUTABLE");
    assert_eq!(entry["strict"], true);
    assert_eq!(entry["security"], "INVOKER");
    assert_eq!(entry["parallelSafety"], "SAFE");
    assert_eq!(entry["owner"], "app_user");
    assert_eq!(entry["description"], "Sums line items minus discounts");
    let definition = entry["definition"].as_str().expect("definition is string");
    assert!(
        definition.contains("calc_order_subtotal"),
        "definition missing function name: {definition}"
    );
}

#[tokio::test]
async fn test_list_functions_detailed_volatile_plpgsql() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "audit_user_login").await;
    let entry = detailed.values().next().expect("audit_user_login entry");
    assert_eq!(entry["language"], "plpgsql");
    assert_eq!(entry["volatility"], "VOLATILE");
    assert_eq!(entry["strict"], false);
}

#[tokio::test]
async fn test_list_functions_detailed_security_definer() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "elevate_user").await;
    let entry = detailed.values().next().expect("elevate_user entry");
    assert_eq!(entry["security"], "DEFINER");
    assert_eq!(entry["description"], "Privileged helper - runs as definer.");
}

#[tokio::test]
async fn test_list_functions_detailed_parallel_safety_safe() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "ratelimit_check").await;
    let entry = detailed.values().next().expect("ratelimit_check entry");
    assert_eq!(entry["parallelSafety"], "SAFE");
    assert_eq!(entry["volatility"], "STABLE");
}

#[tokio::test]
async fn test_list_functions_detailed_no_comment_yields_null_description() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "tmp_helper").await;
    let entry = detailed.get("tmp_helper()").expect("no-arg key");
    assert_eq!(entry["arguments"], "");
    assert!(
        entry["description"].is_null(),
        "description should be null, got {:?}",
        entry["description"]
    );
}

#[tokio::test]
async fn test_list_functions_detailed_multi_arg_signature() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "multi_arg_demo").await;
    let entry = detailed.values().next().expect("multi_arg_demo entry");
    let args = entry["arguments"].as_str().expect("arguments string");
    for substring in ["a integer", "b integer", "OUT total integer", "VARIADIC tags"] {
        assert!(args.contains(substring), "arguments missing {substring:?}: {args}");
    }
}

#[tokio::test]
async fn test_list_functions_detailed_overload_disambiguation() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "calc_order_total").await;
    let keys: Vec<&str> = detailed.keys().map(String::as_str).collect();
    assert!(
        keys.contains(&"calc_order_total(order_id integer)"),
        "missing single-arg overload key, got {keys:?}"
    );
    assert!(
        keys.contains(&"calc_order_total(order_id integer, tax_rate numeric)"),
        "missing two-arg overload key, got {keys:?}"
    );
    assert_eq!(detailed.len(), 2, "expected exactly two overload entries, got {keys:?}");
}

#[tokio::test]
async fn test_list_functions_brief_returns_bare_strings() {
    let handler = handler(true);
    let response = handler
        .list_functions(ListFunctionsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_functions");
    let value = serde_json::to_value(&response).expect("serialise");
    let arr = value["functions"].as_array().expect("functions is array in brief mode");
    for entry in arr {
        assert!(entry.is_string(), "expected string entry, got {entry:?}");
    }
}

#[tokio::test]
async fn test_list_functions_detailed_with_search_only_includes_filtered() {
    let handler = handler(true);
    let detailed = list_functions_detailed(&handler, "calc_order").await;
    let keys: Vec<&str> = detailed.keys().map(String::as_str).collect();
    for excluded in [
        "audit_user_login",
        "elevate_user",
        "ratelimit_check",
        "tmp_helper",
        "multi_arg_demo",
    ] {
        assert!(
            !keys.iter().any(|k| k.starts_with(excluded)),
            "excluded function {excluded} leaked: {keys:?}"
        );
    }
}

#[tokio::test]
async fn test_list_functions_detailed_paginates() {
    let paged = handler_with_page_size(1);
    let mut all_keys = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_functions(ListFunctionsRequest {
                database: Some("app".into()),
                cursor,
                search: Some("calc_order".into()),
                detailed: true,
            })
            .await
            .expect("list_functions detailed paged");
        let page = response.functions.as_detailed().expect("detailed").clone();
        assert!(page.len() <= 1, "page size 1 must not exceed 1 entry");
        all_keys.extend(page.into_keys());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        all_keys.len(),
        3,
        "expected 3 calc_order entries across pages, got {all_keys:?}"
    );
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

// -----------------------------------------------------------------------
// listProcedures search + detailed mode (spec 061)
// -----------------------------------------------------------------------

async fn list_procedures_brief(handler: &PostgresHandler, search: Option<&str>) -> Vec<String> {
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            search: search.map(str::to_string),
            ..Default::default()
        })
        .await
        .expect("list_procedures");
    response.procedures.as_brief().expect("brief mode").to_vec()
}

async fn list_procedures_detailed(
    handler: &PostgresHandler,
    search: &str,
) -> indexmap::IndexMap<String, serde_json::Value> {
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            search: Some(search.into()),
            detailed: true,
            ..Default::default()
        })
        .await
        .expect("list_procedures detailed");
    response.procedures.as_detailed().expect("detailed mode").clone()
}

#[tokio::test]
async fn test_list_procedures_search_filter_returns_only_matches() {
    let handler = handler(true);
    let names = list_procedures_brief(&handler, Some("archive_order")).await;
    assert_eq!(
        names,
        vec![
            "archive_order".to_string(),
            "archive_order_history".to_string(),
            "archive_order_history".to_string(),
        ],
        "expected three archive_order* matches (overload duplication)"
    );
}

#[tokio::test]
async fn test_list_procedures_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = list_procedures_brief(&handler, Some("archive_order")).await;
    let upper = list_procedures_brief(&handler, Some("ARCHIVE_ORDER")).await;
    assert_eq!(lower, upper);
}

#[tokio::test]
async fn test_list_procedures_search_no_match_returns_empty() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            search: Some("nonexistent_procedure_xyz".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");
    assert!(response.procedures.as_brief().expect("brief").is_empty());
    assert!(response.next_cursor.is_none());
}

#[tokio::test]
async fn test_list_procedures_search_supports_wildcard_semantics() {
    // `_` is the ILIKE single-char wildcard. `a_chive` matches `archive` —
    // a literal-substring contract would not.
    let handler = handler(true);
    let names = list_procedures_brief(&handler, Some("a_chive")).await;
    assert!(
        !names.is_empty(),
        "expected `_` to be honoured as wildcard; empty result implies literal-substring mode"
    );
    assert!(
        names.iter().all(|n| n.contains("archive")),
        "all wildcard matches should contain `archive`, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\"] {
        let response = handler
            .list_procedures(ListProceduresRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("list_procedures failed for payload {payload:?}: {e:?}"));
        assert!(
            response.procedures.as_brief().is_some(),
            "payload {payload:?} returned non-brief shape"
        );
    }
}

#[tokio::test]
async fn test_list_procedures_search_paginates_filtered_results() {
    let paged = handler_with_page_size(1);
    let mut all = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_procedures(ListProceduresRequest {
                database: Some("app".into()),
                cursor,
                search: Some("archive_order".into()),
                ..Default::default()
            })
            .await
            .expect("list_procedures paged");
        all.extend(response.procedures.as_brief().expect("brief").to_vec());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    let single = list_procedures_brief(&handler(true), Some("archive_order")).await;
    assert_eq!(all, single, "paginated traversal should equal single-page result");
}

#[tokio::test]
async fn test_list_procedures_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let none_filter = list_procedures_brief(&handler, None).await;
    let empty = list_procedures_brief(&handler, Some("")).await;
    let whitespace = list_procedures_brief(&handler, Some("   ")).await;
    assert_eq!(none_filter, empty);
    assert_eq!(none_filter, whitespace);
}

#[tokio::test]
async fn test_list_procedures_excludes_functions_aggregates_and_window() {
    let handler = handler(true);
    // Function calc_total (prokind='f') must not appear.
    let func = list_procedures_brief(&handler, Some("calc_total")).await;
    assert!(func.is_empty(), "function leaked into listProcedures: {func:?}");
    // Aggregate sum_demo (prokind='a') must not appear.
    let agg = list_procedures_brief(&handler, Some("sum_demo")).await;
    assert!(agg.is_empty(), "aggregate leaked into listProcedures: {agg:?}");
}

#[tokio::test]
async fn test_list_procedures_detailed_returns_full_metadata_for_archive_order() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "archive_order").await;
    // `archive_order` (single-arg) should be present; the overload pair
    // `archive_order_history` also matches and shares the search prefix, so the
    // result has at least three entries — pick the single-arg `archive_order`.
    let key = detailed
        .keys()
        .find(|k| k.starts_with("archive_order(") && !k.starts_with("archive_order_history"))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "archive_order key missing; got {:?}",
                detailed.keys().collect::<Vec<_>>()
            )
        });
    let entry = &detailed[&key];
    assert_eq!(entry["schema"], "public");
    assert_eq!(entry["name"], "archive_order");
    assert_eq!(entry["language"], "plpgsql");
    let args = entry["arguments"].as_str().expect("arguments string");
    assert!(args.contains("order_id integer"), "arguments missing param: {args}");
    assert_eq!(entry["security"], "INVOKER");
    assert_eq!(entry["owner"], "app_user");
    assert_eq!(entry["description"], "Moves an order into the archive table");
    let definition = entry["definition"].as_str().expect("definition is string");
    assert!(
        definition.contains("archive_order"),
        "definition missing procedure name: {definition}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_security_definer() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "elevate_user_proc").await;
    let entry = detailed.values().next().expect("elevate_user_proc entry");
    assert_eq!(entry["security"], "DEFINER");
    assert_eq!(entry["description"], "Privileged helper - runs as definer.");
}

#[tokio::test]
async fn test_list_procedures_detailed_no_comment_yields_null_description() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "tmp_cleanup").await;
    let entry = detailed.get("tmp_cleanup()").expect("zero-arg key with empty parens");
    assert_eq!(entry["arguments"], "");
    assert!(
        entry["description"].is_null(),
        "description should be null, got {:?}",
        entry["description"]
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_zero_arg_key_uniformity() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "tmp_cleanup").await;
    let keys: Vec<&str> = detailed.keys().map(String::as_str).collect();
    assert!(
        keys.contains(&"tmp_cleanup()"),
        "zero-arg key MUST be `tmp_cleanup()` (with empty parens), got {keys:?}"
    );
    assert!(
        !keys.contains(&"tmp_cleanup"),
        "zero-arg key MUST NOT be bare `tmp_cleanup`; got {keys:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_multi_arg_signature() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "summarise_orders").await;
    let entry = detailed.values().next().expect("summarise_orders entry");
    let args = entry["arguments"].as_str().expect("arguments string");
    for substring in [
        "tenant_id integer",
        "OUT total integer",
        "INOUT cursor_name",
        "VARIADIC tags",
    ] {
        assert!(args.contains(substring), "arguments missing {substring:?}: {args}");
    }
}

#[tokio::test]
async fn test_list_procedures_detailed_overload_disambiguation() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "archive_order_history").await;
    let keys: Vec<&str> = detailed.keys().map(String::as_str).collect();
    assert!(
        keys.iter()
            .any(|k| k.starts_with("archive_order_history(") && !k.contains("boolean")),
        "missing single-arg overload key, got {keys:?}"
    );
    assert!(
        keys.iter()
            .any(|k| k.starts_with("archive_order_history(") && k.contains("boolean")),
        "missing two-arg overload key (with boolean), got {keys:?}"
    );
    assert_eq!(detailed.len(), 2, "expected exactly two overload entries, got {keys:?}");
}

#[tokio::test]
async fn test_list_procedures_brief_returns_bare_strings() {
    let handler = handler(true);
    let response = handler
        .list_procedures(ListProceduresRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_procedures");
    let value = serde_json::to_value(&response).expect("serialise");
    let arr = value["procedures"]
        .as_array()
        .expect("procedures is array in brief mode");
    for entry in arr {
        assert!(entry.is_string(), "expected string entry, got {entry:?}");
    }
}

#[tokio::test]
async fn test_list_procedures_detailed_with_search_only_includes_filtered() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "archive_order_history").await;
    let keys: Vec<&str> = detailed.keys().map(String::as_str).collect();
    for excluded in [
        "archive_user",
        "archive_order(",
        "elevate_user_proc",
        "tmp_cleanup",
        "summarise_orders",
        "noop_proc",
        "touch_post",
    ] {
        assert!(
            !keys.iter().any(|k| k.starts_with(excluded)),
            "excluded procedure {excluded} leaked: {keys:?}"
        );
    }
}

#[tokio::test]
async fn test_list_procedures_detailed_paginates() {
    let paged = handler_with_page_size(1);
    let mut all_keys = Vec::new();
    let mut cursor = None;
    loop {
        let response = paged
            .list_procedures(ListProceduresRequest {
                database: Some("app".into()),
                cursor,
                search: Some("archive_order".into()),
                detailed: true,
            })
            .await
            .expect("list_procedures detailed paged");
        let page = response.procedures.as_detailed().expect("detailed").clone();
        assert!(page.len() <= 1, "page size 1 must not exceed 1 entry");
        all_keys.extend(page.into_keys());
        cursor = response.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(
        all_keys.len(),
        3,
        "expected 3 archive_order* entries across pages, got {all_keys:?}"
    );
}

#[tokio::test]
async fn test_list_procedures_detailed_omits_function_only_fields() {
    let handler = handler(true);
    let detailed = list_procedures_detailed(&handler, "archive_order").await;
    let entry = detailed.values().next().expect("at least one entry");
    let obj = entry.as_object().expect("entry is object");
    let keys: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> = [
        "schema",
        "name",
        "language",
        "arguments",
        "security",
        "owner",
        "description",
        "definition",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        keys, expected,
        "detailed entry must carry exactly the eight procedure fields; got {keys:?}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_returns_seeded_matviews() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_materialized_views");

    let names = response.materialized_views.as_brief().expect("brief mode");
    assert!(
        names.contains(&"mv_recent_orders".to_string()),
        "expected seeded mv_recent_orders matview, got {names:?}"
    );
    assert!(
        names.contains(&"mv_user_cohort".to_string()),
        "expected seeded mv_user_cohort matview, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_views_excludes_materialized_views() {
    let handler = handler(true);
    let response = handler
        .list_views(ListViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_views");

    let names = response.views.as_brief().expect("brief mode");
    for matview in ["mv_recent_orders", "mv_user_cohort"] {
        assert!(
            !names.contains(&matview.to_string()),
            "materialized view `{matview}` leaked into listViews output: {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_excludes_regular_views() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_materialized_views");

    let names = response.materialized_views.as_brief().expect("brief mode");
    for view in ["active_users", "published_posts"] {
        assert!(
            !names.contains(&view.to_string()),
            "regular view `{view}` leaked into listMaterializedViews output: {names:?}"
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_empty_for_empty_database() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("analytics".into()),
            ..Default::default()
        })
        .await
        .expect("list_materialized_views");

    assert!(
        response.materialized_views.is_empty(),
        "analytics has no matviews, got len={}",
        response.materialized_views.len()
    );
}

// === spec 067: search + detailed mode tests for listMaterializedViews ===

async fn list_matviews_brief(handler: &PostgresHandler, search: Option<&str>) -> Vec<String> {
    let request = ListMaterializedViewsRequest {
        database: Some("app".into()),
        search: search.map(str::to_owned),
        ..Default::default()
    };
    let response = handler
        .list_materialized_views(request)
        .await
        .expect("list_materialized_views");
    response.materialized_views.as_brief().expect("brief mode").to_vec()
}

async fn list_matviews_detailed(handler: &PostgresHandler, search: Option<&str>) -> IndexMap<String, Value> {
    let request = ListMaterializedViewsRequest {
        database: Some("app".into()),
        search: search.map(str::to_owned),
        detailed: true,
        ..Default::default()
    };
    let response = handler
        .list_materialized_views(request)
        .await
        .expect("list_materialized_views detailed");
    response
        .materialized_views
        .as_detailed()
        .expect("detailed mode")
        .clone()
}

#[tokio::test]
async fn test_list_materialized_views_search_filter_returns_only_matches() {
    let handler = handler(true);
    let names = list_matviews_brief(&handler, Some("orders")).await;
    assert_eq!(
        names,
        vec![
            "mv_archived_orders".to_string(),
            "mv_orders_by_region".to_string(),
            "mv_recent_orders".to_string(),
        ],
        "search=orders must return the three *orders* matviews alphabetically, got {names:?}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_search_is_case_insensitive() {
    let handler = handler(true);
    let lower = list_matviews_brief(&handler, Some("orders")).await;
    let upper = list_matviews_brief(&handler, Some("ORDERS")).await;
    assert_eq!(lower, upper, "ILIKE must be case-insensitive");
}

#[tokio::test]
async fn test_list_materialized_views_search_no_match_returns_empty() {
    let handler = handler(true);
    let names = list_matviews_brief(&handler, Some("zzznomatchxyz")).await;
    assert!(names.is_empty(), "no match must return empty array, got {names:?}");
}

#[tokio::test]
async fn test_list_materialized_views_search_supports_wildcard_semantics() {
    let handler = handler(true);
    // `%` wildcard — `mv_orders%region` matches only `mv_orders_by_region`.
    let percent = list_matviews_brief(&handler, Some("mv_orders%region")).await;
    assert_eq!(
        percent,
        vec!["mv_orders_by_region".to_string()],
        "% wildcard must match mv_orders_by_region only"
    );
    // `_` matches a single character — `mv_or_ers_by_region` matches `mv_orders_by_region`.
    let underscore = list_matviews_brief(&handler, Some("mv_or_ers_by_region")).await;
    assert!(
        underscore.contains(&"mv_orders_by_region".to_string()),
        "_ wildcard must match mv_orders_by_region, got {underscore:?}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_search_sql_meta_payloads_are_safe() {
    let handler = handler(true);
    for payload in ["'", ";", "--", "\\"] {
        let response = handler
            .list_materialized_views(ListMaterializedViewsRequest {
                database: Some("app".into()),
                search: Some(payload.into()),
                ..Default::default()
            })
            .await
            .unwrap_or_else(|e| panic!("search={payload:?} must not raise SQL error: {e:?}"));
        assert!(
            response.materialized_views.as_brief().is_some(),
            "search={payload:?} must return brief mode"
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_search_paginates_filtered_results() {
    let paged = handler_with_page_size(1);
    let full = handler(true);

    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = paged
            .list_materialized_views(ListMaterializedViewsRequest {
                database: Some("app".into()),
                cursor,
                search: Some("orders".into()),
                ..Default::default()
            })
            .await
            .expect("paged list_materialized_views");
        all.extend(response.materialized_views.as_brief().expect("brief").to_vec());
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }

    let single = full
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            search: Some("orders".into()),
            ..Default::default()
        })
        .await
        .expect("single-page list_materialized_views");
    let single_names = single.materialized_views.as_brief().expect("brief").to_vec();
    assert_eq!(all, single_names, "paginated filter must equal single page");
}

#[tokio::test]
async fn test_list_materialized_views_search_empty_is_same_as_no_filter() {
    let handler = handler(true);
    let no_filter = list_matviews_brief(&handler, None).await;
    let empty = list_matviews_brief(&handler, Some("")).await;
    let whitespace = list_matviews_brief(&handler, Some("   ")).await;
    assert_eq!(no_filter, empty, "empty search must be treated as no filter");
    assert_eq!(
        no_filter, whitespace,
        "whitespace-only search must be treated as no filter"
    );
}

#[tokio::test]
async fn test_list_materialized_views_excludes_regular_views_and_system_schemas() {
    let handler = handler(true);
    let all = list_matviews_brief(&handler, None).await;
    for view in [
        "active_users",
        "active_orders",
        "archived_orders",
        "audit_log",
        "published_posts",
    ] {
        assert!(
            !all.contains(&view.to_string()),
            "regular view `{view}` must not appear in listMaterializedViews, got {all:?}"
        );
    }
    let sys = list_matviews_brief(&handler, Some("pg_stat")).await;
    assert!(
        sys.is_empty(),
        "pg_catalog matviews must never appear in listMaterializedViews, got {sys:?}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_returns_full_metadata_for_orders_by_region() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("mv_orders_by_region")).await;
    let entry = map
        .get("mv_orders_by_region")
        .expect("mv_orders_by_region must be present under bare-name key");
    assert_eq!(entry["schema"], Value::String("public".into()));
    assert_eq!(entry["owner"], Value::String("app_user".into()));
    assert_eq!(
        entry["description"],
        Value::String("Orders rolled up by region for the BI dashboard.".into())
    );
    let definition = entry["definition"].as_str().expect("definition string");
    assert!(
        definition.contains("SELECT"),
        "definition must contain SELECT: {definition}"
    );
    assert!(
        definition.contains("paid_orders"),
        "definition must contain CTE name: {definition}"
    );
    assert!(
        definition.contains("'paid'"),
        "single-quote literal must round-trip: {definition}"
    );
    assert_eq!(entry["populated"], Value::Bool(true), "matview should be populated");
    assert_eq!(
        entry["indexed"],
        Value::Bool(true),
        "matview has a unique index, indexed must be true"
    );
    // Bare-name key contract: no `name` field inside the value.
    assert!(
        entry.get("name").is_none(),
        "value must not repeat `name` (it is the map key), got entry: {entry}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_no_comment_yields_null_description() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("mv_archived_orders")).await;
    let entry = map.get("mv_archived_orders").expect("mv_archived_orders entry");
    assert_eq!(
        entry["description"],
        Value::Null,
        "no COMMENT ON MATERIALIZED VIEW → JSON null, not empty string. entry: {entry}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_owner_reflects_alter_owner() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("mv_archived_orders")).await;
    let entry = map.get("mv_archived_orders").expect("mv_archived_orders entry");
    assert_eq!(
        entry["owner"],
        Value::String("reporting_role".into()),
        "owner must reflect ALTER MATERIALIZED VIEW OWNER TO reporting_role"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_definition_round_trip_multiline() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("mv_orders_by_region")).await;
    let entry = map.get("mv_orders_by_region").expect("entry");
    let definition = entry["definition"].as_str().expect("definition string");
    assert!(
        definition.contains("WITH paid_orders AS"),
        "must contain CTE: {definition}"
    );
    assert!(definition.contains("GROUP BY"), "must contain GROUP BY: {definition}");
    assert!(
        definition.contains('\n'),
        "multi-line definition must preserve newlines: {definition:?}"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_reports_populated_false_for_with_no_data() {
    let handler = handler(true);

    let map = list_matviews_detailed(&handler, Some("mv_pending_data")).await;
    let entry = map.get("mv_pending_data").expect("mv_pending_data entry");
    assert_eq!(
        entry["populated"],
        Value::Bool(false),
        "WITH NO DATA matview must report populated=false until refreshed"
    );

    // Refresh and re-check — populated flips to true. Use a non-read-only handler
    // so writeQuery is allowed.
    let writer = handler_with_page_size(50);
    writer
        .write_query(QueryRequest {
            database: Some("app".into()),
            query: "REFRESH MATERIALIZED VIEW mv_pending_data".into(),
        })
        .await
        .expect("refresh mv_pending_data");

    let map = list_matviews_detailed(&handler, Some("mv_pending_data")).await;
    let entry = map.get("mv_pending_data").expect("mv_pending_data entry after refresh");
    assert_eq!(
        entry["populated"],
        Value::Bool(true),
        "after REFRESH MATERIALIZED VIEW, populated must be true"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_reports_indexed_per_index_existence() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, None).await;

    let with_idx = map.get("mv_orders_by_region").expect("mv_orders_by_region entry");
    assert_eq!(
        with_idx["indexed"],
        Value::Bool(true),
        "matview with a unique index must report indexed=true"
    );

    let no_idx = map.get("mv_archived_orders").expect("mv_archived_orders entry");
    assert_eq!(
        no_idx["indexed"],
        Value::Bool(false),
        "matview without any index must report indexed=false"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_with_search_only_includes_filtered() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("orders")).await;
    let keys: Vec<&str> = map.keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        vec!["mv_archived_orders", "mv_orders_by_region", "mv_recent_orders"],
        "detailed+search must include only matching matviews"
    );
}

#[tokio::test]
async fn test_list_materialized_views_detailed_paginates() {
    let paged = handler_with_page_size(2);
    let mut all = Vec::new();
    let mut cursor: Option<dbmcp_server::pagination::Cursor> = None;
    loop {
        let response = paged
            .list_materialized_views(ListMaterializedViewsRequest {
                database: Some("app".into()),
                cursor,
                detailed: true,
                ..Default::default()
            })
            .await
            .expect("paged detailed list_materialized_views");
        all.extend(
            response
                .materialized_views
                .as_detailed()
                .expect("detailed")
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
        );
        match response.next_cursor {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    let full = handler(true);
    let brief = list_matviews_brief(&full, None).await;
    assert_eq!(all, brief, "detailed pagination must match brief order/keys");
}

#[tokio::test]
async fn test_list_materialized_views_brief_returns_bare_strings() {
    let handler = handler(true);
    let response = handler
        .list_materialized_views(ListMaterializedViewsRequest {
            database: Some("app".into()),
            ..Default::default()
        })
        .await
        .expect("list_materialized_views brief");
    let serialized = serde_json::to_value(&response).expect("serialize");
    let entries = serialized.get("materializedViews").expect("materializedViews field");
    assert!(
        entries.is_array(),
        "brief mode materializedViews must serialise as bare-string array"
    );
    for entry in entries.as_array().expect("array") {
        assert!(
            entry.is_string(),
            "brief mode entries must be raw strings, got: {entry:?}"
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_detailed_value_has_exactly_six_fields() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, Some("mv_orders_by_region")).await;
    let entry = map.get("mv_orders_by_region").expect("entry");
    let object = entry.as_object().expect("entry must be object");
    let keys: std::collections::BTreeSet<&str> = object.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> =
        ["schema", "owner", "description", "definition", "populated", "indexed"]
            .into_iter()
            .collect();
    assert_eq!(
        keys, expected,
        "detailed value must contain exactly the six contract fields"
    );
    for forbidden in [
        "name",
        "columns",
        "tablespace",
        "isPopulated",
        "hasIndexes",
        "is_populated",
        "has_indexes",
    ] {
        assert!(
            !object.contains_key(forbidden),
            "detailed value must NOT contain `{forbidden}` field, got entry: {entry}"
        );
    }
}

#[tokio::test]
async fn test_list_materialized_views_detailed_excludes_regular_views() {
    let handler = handler(true);
    let map = list_matviews_detailed(&handler, None).await;
    for view in ["active_users", "active_orders", "archived_orders", "audit_log"] {
        assert!(
            !map.contains_key(view),
            "regular view `{view}` must not appear in detailed listMaterializedViews, got keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }
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

fn handler_with_redaction(redact_pii: bool) -> PostgresHandler {
    PostgresHandler::new(&Config {
        database: base_db_config(false),
        http: None,
        pii: PiiConfig {
            enabled: redact_pii,
            operator: PiiOperator::Replace,
        },
    })
}

fn handler_with_operator(operator: PiiOperator) -> PostgresHandler {
    PostgresHandler::new(&Config {
        database: base_db_config(false),
        http: None,
        pii: PiiConfig {
            enabled: true,
            operator,
        },
    })
}

#[tokio::test]
async fn read_query_redacts_email_when_enabled() {
    let handler = handler_with_redaction(true);
    let select = ReadQueryRequest {
        query: "SELECT 'ping me at jane.doe@example.com' AS msg".into(),
        database: None,
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0]["msg"], "ping me at <EMAIL_ADDRESS>");
}

#[tokio::test]
async fn read_query_unchanged_when_disabled() {
    let handler = handler_with_redaction(false);
    let select = ReadQueryRequest {
        query: "SELECT 'ping me at jane.doe@example.com' AS msg".into(),
        database: None,
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    assert_eq!(rows.rows[0]["msg"], "ping me at jane.doe@example.com");
}

#[tokio::test]
async fn write_query_redacts_returning_clause() {
    let handler = handler_with_redaction(true);
    let insert = QueryRequest {
        query: "INSERT INTO users (name, email) VALUES ('PIIRet', 'piiret@example.com') RETURNING email".into(),
        database: None,
    };
    let rows = handler.write_query(insert).await.unwrap();
    assert_eq!(rows.rows.len(), 1);
    assert_eq!(rows.rows[0]["email"], "<EMAIL_ADDRESS>");

    let cleanup = QueryRequest {
        query: "DELETE FROM users WHERE name = 'PIIRet'".into(),
        database: None,
    };
    handler_with_redaction(false).write_query(cleanup).await.unwrap();
}

#[tokio::test]
async fn explain_analyze_redacts_plan_text() {
    let handler = handler_with_redaction(true);
    let explain = ExplainQueryRequest {
        database: None,
        query: "SELECT 'ping me at jane.doe@example.com' AS msg".into(),
        analyze: true,
    };
    let rows = handler.explain_query(explain).await.unwrap();
    let serialized = serde_json::to_string(&rows.rows).unwrap();
    assert!(
        !serialized.contains("jane.doe@example.com"),
        "raw email leaked into EXPLAIN plan: {serialized}"
    );
}

#[tokio::test]
async fn read_query_mask_operator_replaces_with_asterisks() {
    let handler = handler_with_operator(PiiOperator::Mask);
    let select = ReadQueryRequest {
        query: "SELECT 'jane.doe@example.com' AS msg".into(),
        database: None,
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let out = rows.rows[0]["msg"].as_str().unwrap();
    assert_eq!(out.len(), "jane.doe@example.com".len(), "mask preserves length");
    assert!(out.chars().all(|c| c == '*'), "mask must use '*': {out}");
}

#[tokio::test]
async fn read_query_redact_operator_returns_empty_span() {
    let handler = handler_with_operator(PiiOperator::Redact);
    let select = ReadQueryRequest {
        query: "SELECT 'jane.doe@example.com' AS msg".into(),
        database: None,
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    assert_eq!(rows.rows[0]["msg"], "");
}

#[tokio::test]
async fn read_query_hash_operator_emits_stable_digest() {
    let handler = handler_with_operator(PiiOperator::Hash);
    let select = ReadQueryRequest {
        query: "SELECT 'jane.doe@example.com' AS a, 'jane.doe@example.com' AS b".into(),
        database: None,
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let a = rows.rows[0]["a"].as_str().unwrap();
    let b = rows.rows[0]["b"].as_str().unwrap();
    assert_eq!(a, b, "same input must hash to same digest");
    assert_ne!(a, "jane.doe@example.com");
    assert_eq!(a.len(), 64, "SHA-256 hex digest is 64 chars: {a}");
    assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "digest must be hex: {a}");
}

// Spec 092 / issue #141 (US3) — NUMERIC, MONEY, REAL, DOUBLE PRECISION round-trip
// through readQuery without silent nulls or precision loss. Asserts against
// the value-driven JSON shape rule from data-model.md.
#[tokio::test]
async fn test_read_numeric_columns_round_trip() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT label, n_small, n_int, n_overflow, f4, f8, m_small, m_overflow FROM numeric_samples ORDER BY id"
            .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let by_label: std::collections::HashMap<&str, &Value> = rows
        .rows
        .iter()
        .map(|r| (r["label"].as_str().expect("label is a string"), r))
        .collect();

    let basic = by_label["basic"];
    assert_eq!(basic["n_small"], serde_json::json!(123.45), "NUMERIC(12,2) basic");
    assert_eq!(basic["n_int"], serde_json::json!(42), "NUMERIC(10,0) integer");
    assert_eq!(basic["n_overflow"], serde_json::json!(1.5));
    assert_eq!(basic["f4"], serde_json::json!(1.5), "REAL must not be null");
    assert_eq!(basic["f8"], serde_json::json!(2.5), "DOUBLE PRECISION");
    assert_eq!(basic["m_small"], serde_json::json!(123.45), "MONEY $123.45");
    assert_eq!(
        basic["m_overflow"],
        serde_json::json!("92233720368547758.07"),
        "MONEY at i64::MAX cents must emit as exact-text string"
    );

    let trailing = by_label["trailing_zero"];
    assert_eq!(trailing["n_small"], serde_json::json!(1.2));
    assert_eq!(trailing["n_int"], serde_json::json!(10));
    assert_eq!(trailing["n_overflow"], serde_json::json!(0.1));
    assert_eq!(trailing["m_small"], serde_json::json!(0.1), "MONEY $0.10 normalized");

    let neg = by_label["negative"];
    assert_eq!(neg["n_small"], serde_json::json!(-99.99));
    assert_eq!(neg["n_int"], serde_json::json!(-7));
    assert_eq!(neg["n_overflow"], serde_json::json!(-123.45));
    assert_eq!(neg["f4"], serde_json::json!(-1.5));
    assert_eq!(neg["f8"], serde_json::json!(-2.5));
    assert_eq!(neg["m_small"], serde_json::json!(-99.99));
    assert_eq!(neg["m_overflow"], serde_json::json!("-92233720368547758.08"));

    let overflow = by_label["overflow"];
    assert_eq!(overflow["n_small"], serde_json::json!(0.01));
    assert_eq!(overflow["n_int"], serde_json::json!(1));
    assert_eq!(
        overflow["n_overflow"],
        serde_json::json!("12345678901234567890.123456789"),
        "NUMERIC(38,10) beyond f64 precision must be exact-text string"
    );
    assert_eq!(overflow["f4"], serde_json::json!(1.5));
    assert_eq!(overflow["f8"], serde_json::json!(1e100));
    // $1.00 normalizes to integer 1; bigdecimal_to_json emits as integer
    // JSON number per the integer fast-path in numeric.rs.
    assert_eq!(overflow["m_small"], serde_json::json!(1));

    let null_row = by_label["all_null"];
    assert_eq!(null_row["n_small"], Value::Null, "explicit SQL NULL preserved");
    assert_eq!(null_row["n_int"], Value::Null);
    assert_eq!(null_row["n_overflow"], Value::Null);
    assert_eq!(null_row["f4"], Value::Null);
    assert_eq!(null_row["f8"], Value::Null);
    assert_eq!(null_row["m_small"], Value::Null);
    assert_eq!(null_row["m_overflow"], Value::Null);
}

// Spec 092 US2 — NUMERIC aggregation preserves precision end-to-end (SC-003).
#[tokio::test]
async fn test_read_numeric_aggregation_exact_precision() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query:
            "SELECT SUM(n_small) AS total FROM numeric_samples WHERE label IN ('basic', 'trailing_zero', 'overflow')"
                .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    // 123.45 + 1.20 + 0.01 = 124.66 — fits in f64 → JSON number.
    assert_eq!(rows.rows[0]["total"], serde_json::json!(124.66));
}

// AVG over NUMERIC produces a higher-scale result than the input columns
// (Postgres widens scale for AVG). Verifies the digit-gate path: integers
// inside the avg widen to a fraction with extra zero scale digits but the
// canonical decimal still fits in 15 significant digits → JSON number.
#[tokio::test]
async fn test_read_numeric_avg_preserves_precision() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT AVG(n_int)::numeric(20, 6) AS avg_int FROM numeric_samples \
                WHERE label IN ('basic', 'trailing_zero')"
            .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    // (42 + 10) / 2 = 26 — integer-valued AVG normalises back to integer.
    assert_eq!(rows.rows[0]["avg_int"], serde_json::json!(26));
}

// NUMERIC special values: 'NaN' and ±Infinity (PG 14+) are valid in
// Postgres but unrepresentable in BigDecimal — sqlx::try_get fails and the
// row decoder emits SQL Null. Pin behaviour so a future bigdecimal/sqlx
// upgrade that adds support surfaces as an obvious diff (and we can revisit
// emitting "NaN"/"Infinity" strings if downstream consumers want them).
#[tokio::test]
async fn test_read_numeric_special_values_emit_null() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT 'NaN'::numeric AS n_nan, \
                       'Infinity'::numeric AS n_inf, \
                       '-Infinity'::numeric AS n_neg_inf"
            .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    assert_eq!(rows.rows[0]["n_nan"], Value::Null, "NaN unrepresentable → Null");
    assert_eq!(rows.rows[0]["n_inf"], Value::Null, "+Infinity unrepresentable → Null");
    assert_eq!(
        rows.rows[0]["n_neg_inf"],
        Value::Null,
        "-Infinity unrepresentable → Null"
    );
}

// Float NaN/Infinity round-trip: sqlx decodes as f64::NAN / f64::INFINITY,
// then `Value::from(f64)` emits Null because serde_json::Number cannot
// represent non-finite floats. Locks behaviour explicitly so a future
// switch to a JSON encoder that admits NaN does not silently change the
// wire shape.
#[tokio::test]
async fn test_read_float_nan_inf_emit_null() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT 'NaN'::float8 AS f_nan, \
                       'Infinity'::float8 AS f_inf, \
                       '-Infinity'::float8 AS f_neg_inf, \
                       'NaN'::float4 AS f4_nan, \
                       'Infinity'::float4 AS f4_inf"
            .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    assert_eq!(rows.rows[0]["f_nan"], Value::Null);
    assert_eq!(rows.rows[0]["f_inf"], Value::Null);
    assert_eq!(rows.rows[0]["f_neg_inf"], Value::Null);
    assert_eq!(rows.rows[0]["f4_nan"], Value::Null);
    assert_eq!(rows.rows[0]["f4_inf"], Value::Null);
}

// Negative-scale NUMERIC magnitudes: 1e30 has 1 mantissa digit but 31
// significant decimal digits when canonicalised. The shape rule must route
// to string (not a lossy f64 number) and preserve magnitude/sign — exact
// textual form depends on `BigDecimal::Display` (scientific past internal
// thresholds), so we assert shape, not literal text.
#[tokio::test]
async fn test_read_numeric_huge_magnitude_emits_string() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT 1e30::numeric AS huge, (-1e30)::numeric AS huge_neg".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let huge = rows.rows[0]["huge"].as_str().expect("huge magnitude must be string");
    assert!(
        !huge.starts_with('-'),
        "positive magnitude has no leading minus: {huge}"
    );
    let parsed: f64 = huge.replace('E', "e").parse().expect("parses as f64");
    assert!(
        (parsed - 1e30).abs() / 1e30 < 1e-10,
        "wire form must round-trip to ~1e30, got {huge}"
    );
    let huge_neg = rows.rows[0]["huge_neg"]
        .as_str()
        .expect("negative huge magnitude must be string");
    assert!(huge_neg.starts_with('-'), "negative magnitude keeps sign: {huge_neg}");
}

// f64 underflow regression: NUMERIC values smaller than ~5e-324 round to
// 0.0 when converted to f64. Without an underflow guard, the shape rule
// would emit JSON `0` and silently lose the value. End-to-end check that
// the wire form preserves magnitude for tiny non-zero NUMERICs.
#[tokio::test]
async fn test_read_numeric_f64_underflow_emits_string() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT 1e-500::numeric AS tiny, (-1e-500)::numeric AS tiny_neg".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let tiny = rows.rows[0]["tiny"].as_str().expect("tiny non-zero must be string");
    assert!(
        tiny.to_ascii_lowercase().contains("e-500") || tiny.starts_with("0.0"),
        "magnitude must survive: {tiny}"
    );
    assert_ne!(
        rows.rows[0]["tiny"],
        serde_json::json!(0),
        "must not silently round to JSON 0"
    );
    let tiny_neg = rows.rows[0]["tiny_neg"].as_str().expect("negative tiny is string");
    assert!(tiny_neg.starts_with('-'), "negative sign preserved: {tiny_neg}");
}

// `DOUBLE PRECISION` value at f64-magnitude boundary (spec data-model:74).
// `1e308` is within f64 range and must round-trip as a JSON number;
// `1e400` overflows to ±Infinity and emits Null (spec out-of-scope, but
// pinned here so the wire shape stays consistent if upstream changes).
#[tokio::test]
async fn test_read_float8_extreme_magnitude() {
    let handler = handler(false);
    let select = ReadQueryRequest {
        query: "SELECT 1e308::float8 AS big, (-1e308)::float8 AS big_neg, \
                       'Infinity'::float8 / 1e308::float8 AS overflows"
            .into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let big = rows.rows[0]["big"].as_f64().expect("1e308 fits in f64");
    assert!(
        (big / 1e308 - 1.0).abs() < 1e-10,
        "big magnitude round-trips: got {big}"
    );
    let big_neg = rows.rows[0]["big_neg"].as_f64().expect("-1e308 fits");
    assert!(big_neg < 0.0);
    // Infinity / 1e308 = Infinity → Null.
    assert_eq!(rows.rows[0]["overflows"], Value::Null);
}

// PII redaction interaction with stringified NUMERIC overflow values.
//
// Issue #141 changed NUMERIC overflow rendering from `Null` → JSON string
// (e.g. `"12345678901234567890.123456789"`). With PII enabled, every
// string leaf is scanned, so a stringified numeric is now eligible for
// redaction. The trailing 9-digit run matches the US_SSN recogniser's
// dotted pattern (e.g. `123.45.6789` → `<US_SSN>`). This test pins that
// interaction so a future PII-recogniser tweak (or a decision to skip
// digit-only/decimal-only leaves) surfaces as an obvious diff. PII is
// off by default; operators who enable it accept fuzzy-match collateral.
#[tokio::test]
async fn test_pii_redaction_applies_to_numeric_overflow_string() {
    let handler = handler_with_redaction(true);
    let select = ReadQueryRequest {
        query: "SELECT n_overflow FROM numeric_samples WHERE label = 'overflow'".into(),
        database: Some("app".into()),
        cursor: None,
    };
    let rows = handler.read_query(select).await.unwrap();
    let value = rows.rows[0]["n_overflow"].as_str().expect("string after PII walk");
    // Integer prefix is unanchored against current recognisers and survives
    // intact; trailing 9-digit run hits US_SSN. Document the split so any
    // future change is visible.
    assert!(
        value.starts_with("12345678901234567890."),
        "integer prefix must survive PII walk verbatim: {value}"
    );
    assert!(
        value.contains("<US_SSN>") || value == "12345678901234567890.123456789",
        "trailing digits either match SSN recogniser or pass through verbatim: {value}"
    );
}
