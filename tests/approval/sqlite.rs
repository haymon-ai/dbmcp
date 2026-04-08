//! `SQLite` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`.

mod common;

use database_mcp_config::{DatabaseBackend, DatabaseConfig};
use database_mcp_sqlite::SqliteAdapter;

/// Creates a `SQLite` adapter from the `DB_PATH` environment variable.
fn adapter(read_only: bool) -> SqliteAdapter {
    let config = DatabaseConfig {
        backend: DatabaseBackend::Sqlite,
        name: Some(std::env::var("DB_PATH").expect("DB_PATH must be set")),
        read_only,
        ..DatabaseConfig::default()
    };
    SqliteAdapter::new(&config)
}

#[tokio::test]
async fn test_server_info() {
    common::run_with_client(adapter(false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_list_tools() {
    common::run_with_client(adapter(false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools", tools);
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_only() {
    common::run_with_client(adapter(true), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only", tools);
    })
    .await;
}
