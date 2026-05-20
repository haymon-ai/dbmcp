//! `MySQL` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`.

mod common;

use dbmcp_config::{Config, DatabaseBackend, DatabaseConfig, PiiConfig};
use dbmcp_mysql::MysqlHandler;
use dbmcp_server::Server;

/// Creates a `MySQL`-backed [`Server`] from `DB_HOST` and `DB_PORT` environment variables.
///
/// `pinned == true` sets the config `name` to `"app"`; `false` leaves it `None`.
fn server(read_only: bool, pinned: bool) -> Server {
    let config = Config {
        database: DatabaseConfig {
            backend: DatabaseBackend::Mysql,
            host: std::env::var("DB_HOST").expect("DB_HOST must be set"),
            port: std::env::var("DB_PORT")
                .expect("DB_PORT must be set")
                .parse()
                .expect("DB_PORT must be a valid port number"),
            name: pinned.then(|| "app".into()),
            read_only,
            ..DatabaseConfig::default()
        },
        http: None,
        pii: PiiConfig::default(),
    };
    MysqlHandler::new(&config).into()
}

#[tokio::test]
async fn test_server_info_read_write_pinned() {
    common::run_with_client(server(false, true), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_write_pinned", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_server_info_read_write_unpinned() {
    common::run_with_client(server(false, false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_write_unpinned", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_server_info_read_only_pinned() {
    common::run_with_client(server(true, true), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_only_pinned", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_server_info_read_only_unpinned() {
    common::run_with_client(server(true, false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_only_unpinned", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_write_pinned() {
    common::run_with_client(server(false, true), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_write_pinned", tools);
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_write_unpinned() {
    common::run_with_client(server(false, false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_write_unpinned", tools);
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_only_pinned() {
    common::run_with_client(server(true, true), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only_pinned", tools);
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_only_unpinned() {
    common::run_with_client(server(true, false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_only_unpinned", tools);
    })
    .await;
}
