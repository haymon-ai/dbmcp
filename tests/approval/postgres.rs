//! `PostgreSQL` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`.

mod common;

use database_mcp_postgres::PostgresAdapter;
use sqlx::PgPool;

#[sqlx::test]
async fn test_server_info(pool: PgPool) {
    common::run_with_client(PostgresAdapter::from_pool(pool, false).await, |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[sqlx::test]
async fn test_list_tools(pool: PgPool) {
    common::run_with_client(PostgresAdapter::from_pool(pool, false).await, |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools", tools);
    })
    .await;
}

#[sqlx::test]
async fn test_list_tools_read_only(pool: PgPool) {
    common::run_with_client(PostgresAdapter::from_pool(pool, true).await, |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only", tools);
    })
    .await;
}
