//! `SQLite` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`.

mod common;

use database_mcp_sqlite::SqliteAdapter;
use sqlx::SqlitePool;

#[sqlx::test]
async fn test_server_info(pool: SqlitePool) {
    common::run_with_client(SqliteAdapter::from_pool(pool, false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[sqlx::test]
async fn test_list_tools(pool: SqlitePool) {
    common::run_with_client(SqliteAdapter::from_pool(pool, false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools", tools);
    })
    .await;
}

#[sqlx::test]
async fn test_list_tools_read_only(pool: SqlitePool) {
    common::run_with_client(SqliteAdapter::from_pool(pool, true), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only", tools);
    })
    .await;
}
