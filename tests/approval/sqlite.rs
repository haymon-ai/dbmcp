//! `SQLite` approval tests.
//!
//! Captures MCP tool schemas and server info as golden files using `insta`,
//! and checks tool-call results end to end through a real rmcp client.

mod common;

use dbmcp_config::{Config, DatabaseBackend, DatabaseConfig, HttpConfig, PiiConfig};
use dbmcp_server::Server;
use dbmcp_sqlite::SqliteHandler;
use rmcp::RoleClient;
use rmcp::model::{CacheScope, CallToolRequestParams, CallToolResponse, CallToolResult, JsonObject, ProtocolVersion};
use rmcp::service::{ClientLifecycleMode, Peer, ServiceError};
use serde_json::json;

/// Creates a `SQLite`-backed [`Server`] from the `DB_PATH` environment variable.
fn server(read_only: bool) -> Server {
    server_with_pii(read_only, PiiConfig::default())
}

/// Creates a `SQLite`-backed [`Server`] with the given PII configuration.
fn server_with_pii(read_only: bool, pii: PiiConfig) -> Server {
    let config = Config {
        database: DatabaseConfig {
            backend: DatabaseBackend::Sqlite,
            name: Some(std::env::var("DB_PATH").expect("DB_PATH must be set")),
            read_only,
            ..DatabaseConfig::default()
        },
        http: HttpConfig::default(),
        pii,
        ssh: None,
    };
    SqliteHandler::new(&config).expect("handler builds").into()
}

/// Calls tool `name` through the client and returns its completed result.
async fn call_tool(
    peer: &Peer<RoleClient>,
    name: &'static str,
    arguments: Option<JsonObject>,
) -> Result<CallToolResult, ServiceError> {
    let mut params = CallToolRequestParams::new(name);
    if let Some(arguments) = arguments {
        params = params.with_arguments(arguments);
    }
    match peer.call_tool_once(params).await? {
        CallToolResponse::Complete(result) => Ok(result),
        other => panic!("expected a completed tool result, got {other:?}"),
    }
}

/// Unwraps a `json!` object literal into tool arguments.
fn object(value: serde_json::Value) -> JsonObject {
    match value {
        serde_json::Value::Object(map) => map,
        other => panic!("tool arguments must be a JSON object, got {other}"),
    }
}

/// Returns the text of the first content block of `result`.
fn text(result: &CallToolResult) -> &str {
    &result.content[0].as_text().expect("text content").text
}

#[tokio::test]
async fn test_server_info_read_write() {
    common::run_with_client(server(false), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_write", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_server_info_read_only() {
    common::run_with_client(server(true), |peer| async move {
        let info = peer.peer_info().expect("missing peer_info");
        insta::assert_json_snapshot!("server_info_read_only", info, {
            ".serverInfo.version" => "[version]"
        });
    })
    .await;
}

#[tokio::test]
async fn test_list_tools_read_write() {
    common::run_with_client(server(false), |peer| async move {
        let tools = peer.list_all_tools().await.expect("list_all_tools failed");
        insta::assert_json_snapshot!("list_tools_read_write", tools);
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

#[tokio::test]
async fn test_list_tools_cache_hints_are_private() {
    let lifecycle = ClientLifecycleMode::Discover {
        preferred_versions: vec![ProtocolVersion::V_2026_07_28],
    };
    common::run_with_client_lifecycle(server(true), lifecycle, |peer| async move {
        let result = peer.list_tools(None).await.expect("list_tools succeeds");
        assert_eq!(result.ttl_ms, Some(0));
        assert_eq!(result.cache_scope, Some(CacheScope::Private));
    })
    .await;
}

#[tokio::test]
async fn test_call_tool_error_is_returned_in_band() {
    common::run_with_client(server(true), |peer| async move {
        let result = call_tool(
            &peer,
            "readQuery",
            Some(object(json!({"query": "SELECT * FROM missing_table"}))),
        )
        .await
        .expect("tool error must not be a protocol error");
        assert_eq!(result.is_error, Some(true));
        assert!(text(&result).contains("no such table"), "got: {}", text(&result));
    })
    .await;
}

#[tokio::test]
async fn test_call_tool_error_is_redacted_when_pii_enabled() {
    let pii = PiiConfig {
        enabled: true,
        ..PiiConfig::default()
    };
    common::run_with_client(server_with_pii(true, pii), |peer| async move {
        let result = call_tool(
            &peer,
            "readQuery",
            Some(object(json!({"query": "SELECT * FROM \"jane@doe.com\""}))),
        )
        .await
        .expect("tool error must not be a protocol error");
        assert_eq!(result.is_error, Some(true));
        assert!(!text(&result).contains("jane@doe.com"), "got: {}", text(&result));
        assert!(text(&result).contains("<EMAIL_ADDRESS>"), "got: {}", text(&result));
    })
    .await;
}

#[tokio::test]
async fn test_call_tool_without_arguments() {
    common::run_with_client(server(true), |peer| async move {
        let result = call_tool(&peer, "listViews", None).await.expect("listViews succeeds");
        assert_ne!(result.is_error, Some(true), "got: {}", text(&result));
    })
    .await;
}

#[tokio::test]
async fn test_call_unknown_tool_is_protocol_error() {
    common::run_with_client(server(true), |peer| async move {
        let result = call_tool(&peer, "noSuchTool", Some(object(json!({})))).await;
        assert!(result.is_err(), "unknown tool must be a protocol error, got {result:?}");
    })
    .await;
}
