//! Functional tests for the SSH tunnel.
//!
//! Spawns the `dbmcp` binary over stdio and queries a database that is only
//! reachable through the sshd container. The child inherits `DB_BACKEND`,
//! `DB_HOST`, `DB_PORT`, and every `DB_SSH_*` variable from the environment.
//!
//! ```bash
//! ./tests/run.sh --filter mysql      # MySQL/MariaDB via sshd
//! ./tests/run.sh --filter postgres   # PostgreSQL via sshd
//! ```

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

/// Starts `dbmcp stdio` with the inherited database and SSH environment.
async fn client() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    assert!(
        std::env::var_os("DB_SSH_HOST").is_some(),
        "DB_SSH_HOST must be set (run via ./tests/run.sh)"
    );
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dbmcp"));
    command.arg("stdio").env("DB_NAME", "app");
    let transport = TokioChildProcess::new(command).expect("spawn dbmcp");
    ().serve(transport).await.expect("mcp handshake")
}

/// Calls `readQuery` and returns the structured result.
async fn read_query(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>, query: &str) -> Value {
    let arguments = json!({ "query": query }).as_object().cloned().expect("object");
    let result = client
        .call_tool(CallToolRequestParams::new("readQuery").with_arguments(arguments))
        .await
        .expect("tools/call");
    assert_ne!(result.is_error, Some(true), "tool error: {result:?}");
    result.structured_content.expect("structured content")
}

#[tokio::test]
async fn read_query_succeeds_through_ssh_tunnel() {
    let client = client().await;

    let result = read_query(&client, "SELECT name FROM users WHERE id = 1").await;
    assert_eq!(result["rows"], json!([{ "name": "Alice Johnson" }]), "{result}");

    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn sequential_queries_reuse_ssh_tunnel() {
    let client = client().await;

    for _ in 0..3 {
        let result = read_query(&client, "SELECT COUNT(*) AS n FROM post_tags").await;
        let rows = result["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 1, "{result}");
    }

    client.cancel().await.expect("shutdown");
}
