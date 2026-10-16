//! Functional tests for the SSH tunnel.
//!
//! Spawns the `dbmcp` binary over stdio and queries a database that is only
//! reachable through the sshd container. The child inherits `DB_BACKEND`,
//! `DB_HOST`, `DB_PORT`, and every `DB_SSH_*` variable from the environment.
//! Key tests drop `DB_SSH_PASSWORD` and authenticate with the fixture keys
//! in `crates/ssh/testdata`, authorised via `tests/ssh/authorized_keys`.
//!
//! ```bash
//! ./tests/run.sh --filter mysql      # MySQL/MariaDB via sshd
//! ./tests/run.sh --filter postgres   # PostgreSQL via sshd
//! ```

use std::process::Stdio;

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

/// Passphrase of the encrypted fixture key.
const PASSPHRASE: &str = "secret";

/// Returns the path of a fixture key in `crates/ssh/testdata`.
fn key(name: &str) -> String {
    format!("{}/crates/ssh/testdata/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Builds `dbmcp stdio` with the inherited database and SSH environment.
fn command() -> tokio::process::Command {
    assert!(
        std::env::var_os("DB_SSH_HOST").is_some(),
        "DB_SSH_HOST must be set (run via ./tests/run.sh)"
    );
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dbmcp"));
    command.arg("stdio").env("DB_NAME", "app");
    command
}

/// Builds `dbmcp stdio` authenticating only with the key `name`.
fn key_command(name: &str, passphrase: Option<&str>) -> tokio::process::Command {
    let mut command = command();
    command.env_remove("DB_SSH_PASSWORD").env("DB_SSH_KEY", key(name));
    match passphrase {
        Some(passphrase) => command.env("DB_SSH_KEY_PASSPHRASE", passphrase),
        None => command.env_remove("DB_SSH_KEY_PASSPHRASE"),
    };
    command
}

/// Completes the MCP handshake with a spawned `dbmcp`.
async fn connect(command: tokio::process::Command) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let transport = TokioChildProcess::new(command).expect("spawn dbmcp");
    ().serve(transport).await.expect("mcp handshake")
}

/// Starts `dbmcp stdio` with the inherited database and SSH environment.
async fn client() -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    connect(command()).await
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

/// Runs one query through a tunnel authenticated with key `name`.
async fn assert_key_auth(name: &str, passphrase: Option<&str>) {
    let client = connect(key_command(name, passphrase)).await;

    let result = read_query(&client, "SELECT name FROM users WHERE id = 1").await;
    assert_eq!(result["rows"], json!([{ "name": "Alice Johnson" }]), "{name}: {result}");

    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn read_query_succeeds_with_ed25519_key() {
    assert_key_auth("openssh_ed25519", None).await;
}

#[tokio::test]
async fn read_query_succeeds_with_encrypted_key() {
    assert_key_auth("openssh_ed25519_enc", Some(PASSPHRASE)).await;
}

/// OpenSSH 8.8+ rejects SHA-1 `ssh-rsa`, so success proves `rsa-sha2-*`.
#[tokio::test]
async fn read_query_succeeds_with_rsa_key() {
    assert_key_auth("openssh_rsa", None).await;
}

#[tokio::test]
async fn wrong_key_passphrase_fails_startup() {
    let path = key("openssh_ed25519_enc");
    let output = key_command("openssh_ed25519_enc", Some("wrong"))
        .stdin(Stdio::null())
        .output()
        .await
        .expect("run dbmcp");

    assert!(!output.status.success(), "startup must fail: {output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(&format!(
            "server initialisation failed: could not decrypt SSH key {path} (wrong passphrase or unsupported format)"
        )),
        "{stderr}"
    );
}
