//! `PostgreSQL` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`.

mod common;

use dbmcp_config::{DatabaseBackend, DatabaseConfig};
use dbmcp_postgres::PostgresHandler;
use dbmcp_server::Server;

/// Creates a `PostgreSQL`-backed [`Server`] from `DB_HOST` and `DB_PORT` environment variables.
fn server(read_only: bool) -> Server {
    let config = DatabaseConfig {
        backend: DatabaseBackend::Postgres,
        host: std::env::var("DB_HOST").expect("DB_HOST must be set"),
        port: std::env::var("DB_PORT")
            .expect("DB_PORT must be set")
            .parse()
            .expect("DB_PORT must be a valid port number"),
        user: "postgres".into(),
        name: Some("app".into()),
        read_only,
        ..DatabaseConfig::default()
    };
    PostgresHandler::new(&config).into()
}

#[tokio::test]
async fn test_server_info() {
    common::run_with_client(server(false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_list_tools() {
    common::run_with_client(server(false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools", tools);
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_only() {
    common::run_with_client(server(true), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only", tools);
    })
    .await;
}
