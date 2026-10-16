//! Functional tests for the SSH tunnel.
//!
//! Spawns the `dbmcp` binary over stdio and queries a database that is only
//! reachable through the sshd container. The child inherits `DB_BACKEND`,
//! `DB_HOST`, `DB_PORT`, and every `DB_SSH_*` variable from the environment.
//! Key tests drop `DB_SSH_PASSWORD` and authenticate with the fixture keys
//! in `crates/ssh/testdata`, authorised via `tests/ssh/authorized_keys`.
//! Host-key tests point `DB_SSH_KNOWN_HOSTS` at a temporary file; the H4
//! tests seed it with `ssh-keyscan`, which must be on `PATH`.
//!
//! ```bash
//! ./tests/run.sh --filter mysql      # MySQL/MariaDB via sshd
//! ./tests/run.sh --filter postgres   # PostgreSQL via sshd
//! ```

use std::path::Path;
use std::process::Stdio;

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
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

/// Builds `dbmcp stdio` checking host keys in `known_hosts` under `mode`.
fn verifying_command(known_hosts: &Path, mode: &str) -> tokio::process::Command {
    let mut command = command();
    command
        .env("DB_SSH_KNOWN_HOSTS", known_hosts)
        .env("DB_SSH_STRICT_HOST_KEY_CHECKING", mode)
        .env("DB_CONNECTION_TIMEOUT", "5");
    command
}

/// Returns the SSH server as recorded in `known_hosts` (`[host]:port`).
fn known_hosts_name() -> String {
    let host = std::env::var("DB_SSH_HOST").expect("DB_SSH_HOST");
    let port = std::env::var("DB_SSH_PORT").expect("DB_SSH_PORT");
    format!("[{host}]:{port}")
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

/// Calls `readQuery` and returns the raw tool result.
async fn call_read_query(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>, query: &str) -> CallToolResult {
    let arguments = json!({ "query": query }).as_object().cloned().expect("object");
    client
        .call_tool(CallToolRequestParams::new("readQuery").with_arguments(arguments))
        .await
        .expect("tools/call")
}

/// Calls `readQuery` and returns the structured result.
async fn read_query(client: &rmcp::service::RunningService<rmcp::RoleClient, ()>, query: &str) -> Value {
    let result = call_read_query(client, query).await;
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

/// Runs one query through a tunnel and asserts it succeeds.
async fn assert_query_succeeds(command: tokio::process::Command) {
    let client = connect(command).await;

    let result = read_query(&client, "SELECT name FROM users WHERE id = 1").await;
    assert_eq!(result["rows"], json!([{ "name": "Alice Johnson" }]), "{result}");

    client.cancel().await.expect("shutdown");
}

/// Runs one query, asserts it fails, and returns the captured stderr.
async fn assert_query_fails(command: tokio::process::Command) -> String {
    let (transport, stderr) = TokioChildProcess::builder(command)
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn dbmcp");
    let mut stderr = stderr.expect("stderr piped");
    let logs = tokio::spawn(async move {
        let mut logs = String::new();
        let _ = tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut logs).await;
        logs
    });
    let client = ().serve(transport).await.expect("mcp handshake");

    let result = call_read_query(&client, "SELECT 1").await;
    assert_eq!(result.is_error, Some(true), "query must fail: {result:?}");

    client.cancel().await.expect("shutdown");
    logs.await.expect("stderr reader")
}

/// Records the sshd host key of `key_type` in `known_hosts` via `ssh-keyscan`.
async fn keyscan(key_type: &str, known_hosts: &Path) {
    let output = tokio::process::Command::new("ssh-keyscan")
        .args(["-t", key_type, "-p"])
        .arg(std::env::var("DB_SSH_PORT").expect("DB_SSH_PORT"))
        .arg(std::env::var("DB_SSH_HOST").expect("DB_SSH_HOST"))
        .output()
        .await
        .expect("run ssh-keyscan");
    assert!(output.status.success(), "ssh-keyscan failed: {output:?}");
    assert!(!output.stdout.is_empty(), "no {key_type} host key: {output:?}");
    std::fs::write(known_hosts, &output.stdout).expect("write known_hosts");
}

#[tokio::test]
async fn unknown_host_key_is_rejected_under_yes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join("known_hosts");

    let logs = assert_query_fails(verifying_command(&known_hosts, "yes")).await;

    assert!(
        logs.contains(&format!("SSH host key for {} (SHA256:", known_hosts_name())),
        "{logs}"
    );
    assert!(logs.contains("ssh-keyscan -p"), "{logs}");
    assert!(!known_hosts.exists(), "yes never writes known_hosts");
}

#[tokio::test]
async fn accept_new_learns_host_key_then_matches() {
    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join(".ssh").join("known_hosts");

    assert_query_succeeds(verifying_command(&known_hosts, "accept-new")).await;

    let recorded = std::fs::read_to_string(&known_hosts).expect("known_hosts written");
    assert_eq!(recorded.lines().count(), 1, "{recorded}");
    assert!(
        recorded.starts_with(&format!("{} ssh-", known_hosts_name())),
        "{recorded}"
    );

    assert_query_succeeds(verifying_command(&known_hosts, "yes")).await;
    assert_query_succeeds(verifying_command(&known_hosts, "accept-new")).await;
    assert_eq!(
        std::fs::read_to_string(&known_hosts).expect("read"),
        recorded,
        "known host is not appended again"
    );
}

#[tokio::test]
async fn changed_host_key_is_rejected() {
    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join("known_hosts");
    let impostor = std::fs::read_to_string(key("openssh_ed25519.pub")).expect("fixture key");
    std::fs::write(&known_hosts, format!("{} {impostor}", known_hosts_name())).expect("seed");

    for mode in ["yes", "accept-new"] {
        let logs = assert_query_fails(verifying_command(&known_hosts, mode)).await;
        assert!(logs.contains("does not match"), "{mode}: {logs}");
    }
}

/// H4: a host recorded only as ECDSA must not be offered Ed25519 first.
#[tokio::test]
async fn host_recorded_only_as_ecdsa_connects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join("known_hosts");
    keyscan("ecdsa", &known_hosts).await;

    assert_query_succeeds(verifying_command(&known_hosts, "yes")).await;
}

/// H4: a host recorded only as RSA must not be offered Ed25519 first.
#[tokio::test]
async fn host_recorded_only_as_rsa_connects() {
    let dir = tempfile::tempdir().expect("tempdir");
    let known_hosts = dir.path().join("known_hosts");
    keyscan("rsa", &known_hosts).await;

    assert_query_succeeds(verifying_command(&known_hosts, "yes")).await;
}
