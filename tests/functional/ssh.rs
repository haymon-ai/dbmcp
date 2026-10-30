//! Functional tests for the SSH tunnel.
//!
//! Spawns the `dbmcp` binary over stdio and queries a database that is only
//! reachable through the sshd container. The child inherits `DBMCP_DB_BACKEND`,
//! `DBMCP_DB_HOST`, `DBMCP_DB_PORT`, and every `DBMCP_SSH_*` variable from the environment.
//! Key tests drop `DBMCP_SSH_PASSWORD` and authenticate with the fixture keys
//! in `crates/ssh/testdata`, authorised via `tests/ssh/authorized_keys`.
//! Host-key tests point `DBMCP_SSH_KNOWN_HOSTS` at a temporary file; the H4
//! tests seed it with `ssh-keyscan`, which must be on `PATH`.
//!
//! The reconnect test restarts sshd inside the container named by
//! `SSHD_CONTAINER` with `docker exec … pkill sshd`. The DB TLS tests run only when `TLS_TEST_CA` names
//! the database's CA certificate (`mysql:9` generates one on first boot) and
//! are skipped otherwise.
//!
//! ```bash
//! ./tests/run.sh --filter mysql      # MySQL/MariaDB via sshd
//! ./tests/run.sh --filter postgres   # PostgreSQL via sshd
//! ```

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

/// Passphrase of the encrypted fixture key.
const PASSPHRASE: &str = "secret";

/// Overall deadline for queries to recover after an sshd restart.
const RECOVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// Returns the path of a fixture key in `crates/ssh/testdata`.
fn key(name: &str) -> String {
    format!("{}/crates/ssh/testdata/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// Builds `dbmcp` with the inherited database and SSH environment.
fn command() -> tokio::process::Command {
    assert!(
        std::env::var_os("DBMCP_SSH_HOST").is_some(),
        "DBMCP_SSH_HOST must be set (run via ./tests/run.sh)"
    );
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dbmcp"));
    command.env("DBMCP_DB_NAME", "app");
    command
}

/// Builds `dbmcp` authenticating only with the key `name`.
fn key_command(name: &str, passphrase: Option<&str>) -> tokio::process::Command {
    let mut command = command();
    command.env_remove("DBMCP_SSH_PASSWORD").env("DBMCP_SSH_KEY", key(name));
    match passphrase {
        Some(passphrase) => command.env("DBMCP_SSH_KEY_PASSPHRASE", passphrase),
        None => command.env_remove("DBMCP_SSH_KEY_PASSPHRASE"),
    };
    command
}

/// Builds `dbmcp` checking host keys in `known_hosts` under `mode`.
fn verifying_command(known_hosts: &Path, mode: &str) -> tokio::process::Command {
    let mut command = command();
    command
        .env("DBMCP_SSH_KNOWN_HOSTS", known_hosts)
        .env("DBMCP_SSH_STRICT_HOST_KEY_CHECKING", mode)
        .env("DBMCP_DB_CONNECTION_TIMEOUT", "5");
    command
}

/// Returns the SSH server's `DBMCP_SSH_HOST` and `DBMCP_SSH_PORT`.
fn ssh_endpoint() -> (String, String) {
    let host = std::env::var("DBMCP_SSH_HOST").expect("DBMCP_SSH_HOST");
    let port = std::env::var("DBMCP_SSH_PORT").expect("DBMCP_SSH_PORT");
    (host, port)
}

/// Returns the SSH server as recorded in `known_hosts`.
fn known_hosts_name() -> String {
    match ssh_endpoint() {
        (host, port) if port == "22" => host,
        (host, port) => format!("[{host}]:{port}"),
    }
}

/// Completes the MCP handshake with a spawned `dbmcp`.
async fn connect(command: tokio::process::Command) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let transport = TokioChildProcess::new(command).expect("spawn dbmcp");
    ().serve(transport).await.expect("mcp handshake")
}

/// Starts `dbmcp` with the inherited database and SSH environment.
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
    assert_query_succeeds(key_command(name, passphrase)).await;
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

/// Connects like [`connect`], collecting the child's stderr until it exits.
async fn connect_with_logs(
    command: tokio::process::Command,
) -> (
    rmcp::service::RunningService<rmcp::RoleClient, ()>,
    tokio::task::JoinHandle<String>,
) {
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
    (().serve(transport).await.expect("mcp handshake"), logs)
}

/// Runs one query, asserts it fails, and returns the captured stderr.
async fn assert_query_fails(command: tokio::process::Command) -> String {
    let (client, logs) = connect_with_logs(command).await;

    let result = call_read_query(&client, "SELECT 1").await;
    assert_eq!(result.is_error, Some(true), "query must fail: {result:?}");

    client.cancel().await.expect("shutdown");
    logs.await.expect("stderr reader")
}

/// Runs one query, asserts it fails, and returns the tool error text.
async fn query_error_text(command: tokio::process::Command) -> String {
    let client = connect(command).await;

    let text = tool_error_text(&call_read_query(&client, "SELECT 1").await);

    client.cancel().await.expect("shutdown");
    text
}

/// Asserts `result` is a tool error and returns its text.
fn tool_error_text(result: &CallToolResult) -> String {
    assert_eq!(result.is_error, Some(true), "query must fail: {result:?}");
    result.content[0].as_text().expect("text content").text.clone()
}

/// Records the sshd host key of `key_type` in `known_hosts` via `ssh-keyscan`.
async fn keyscan(key_type: &str, known_hosts: &Path) {
    let (host, port) = ssh_endpoint();
    let output = tokio::process::Command::new("ssh-keyscan")
        .args(["-t", key_type, "-p", &port, &host])
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

/// H4: a host recorded only as ECDSA or RSA must not be offered Ed25519 first.
#[tokio::test]
async fn host_recorded_only_as_ecdsa_or_rsa_connects() {
    for key_type in ["ecdsa", "rsa"] {
        let dir = tempfile::tempdir().expect("tempdir");
        let known_hosts = dir.path().join("known_hosts");
        keyscan(key_type, &known_hosts).await;

        assert_query_succeeds(verifying_command(&known_hosts, "yes")).await;
    }
}

#[tokio::test]
async fn tool_error_names_ssh_failure() {
    let mut command = command();
    command
        .env("DBMCP_SSH_PASSWORD", "wrong")
        .env_remove("DBMCP_SSH_KEY")
        .env("DBMCP_DB_CONNECTION_TIMEOUT", "5");
    let client = connect(command).await;

    let text = tool_error_text(&call_read_query(&client, "SELECT 1").await);
    let (host, port) = ssh_endpoint();
    assert!(
        text.ends_with(&format!(
            "(SSH tunnel: SSH authentication failed for dbmcp@{host}:{port})"
        )),
        "{text}"
    );

    // Errors unrelated to reaching the database carry no SSH suffix.
    let text = tool_error_text(&call_read_query(&client, "DELETE FROM users").await);
    assert!(text.starts_with("Query blocked"), "{text}");
    assert!(!text.contains("SSH tunnel"), "{text}");

    client.cancel().await.expect("shutdown");
}

#[tokio::test]
async fn queries_recover_after_sshd_restart() {
    let container = std::env::var("SSHD_CONTAINER").expect("SSHD_CONTAINER must be set (run via ./tests/run.sh)");
    let mut command = command();
    command.env("DBMCP_DB_CONNECTION_TIMEOUT", "5");
    let (client, logs) = connect_with_logs(command).await;
    read_query(&client, "SELECT name FROM users WHERE id = 1").await;

    // Kills the listener and every session process; s6 restarts the listener
    // without re-running container init, unlike `docker restart`.
    let status = tokio::process::Command::new("docker")
        .args(["exec", &container, "pkill", "sshd"])
        .status()
        .await
        .expect("run docker exec");
    assert!(status.success(), "pkill sshd in {container} failed");

    let deadline = Instant::now() + RECOVERY_TIMEOUT;
    loop {
        let result = call_read_query(&client, "SELECT name FROM users WHERE id = 1").await;
        if result.is_error != Some(true) {
            let rows = &result.structured_content.expect("structured content")["rows"];
            assert_eq!(rows, &json!([{ "name": "Alice Johnson" }]));
            break;
        }
        assert!(Instant::now() < deadline, "queries did not recover: {result:?}");
        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    client.cancel().await.expect("shutdown");
    let logs = logs.await.expect("stderr reader");
    assert!(logs.contains("lost: "), "session loss must be observed: {logs}");
    assert!(logs.contains("Reconnecting SSH session"), "{logs}");
}

/// Builds `dbmcp` requiring DB TLS verified against `ca`.
fn tls_command(ca: &str) -> tokio::process::Command {
    let mut command = command();
    command
        .env("DBMCP_DB_SSL", "true")
        .env("DBMCP_DB_SSL_VERIFY_CERT", "true")
        .env("DBMCP_DB_SSL_CA", ca)
        .env("DBMCP_DB_CONNECTION_TIMEOUT", "5");
    command
}

/// Returns `TLS_TEST_CA`, or `None` when the database has no test CA.
fn tls_test_ca() -> Option<String> {
    let ca = std::env::var("TLS_TEST_CA").ok();
    if ca.is_none() {
        eprintln!("skipped: TLS_TEST_CA is not set");
    }
    ca
}

/// `DBMCP_DB_SSL_VERIFY_CERT` ignores the hostname, so `127.0.0.1` verifies.
#[tokio::test]
async fn tls_database_verifies_through_ssh_tunnel() {
    let Some(ca) = tls_test_ca() else { return };
    assert_query_succeeds(tls_command(&ca)).await;
}

#[tokio::test]
async fn tls_database_with_wrong_ca_fails_through_ssh_tunnel() {
    if tls_test_ca().is_none() {
        return;
    }
    let wrong = format!("{}/tests/tls/wrong_ca.pem", env!("CARGO_MANIFEST_DIR"));
    let text = query_error_text(tls_command(&wrong)).await;
    assert!(text.contains("invalid peer certificate"), "{text}");
}
