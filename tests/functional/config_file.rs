//! Shared helpers for spawning `dbmcp stdio --config <file>`.
//!
//! The child starts with every inherited `DB_*` env var removed, so only
//! the config file and the env vars a test passes explicitly apply.

#![allow(dead_code, reason = "each backend test binary uses a different subset")]

use rmcp::ServiceExt;
use rmcp::model::CallToolRequestParams;
use rmcp::service::RunningService;
use rmcp::transport::TokioChildProcess;
use serde_json::{Value, json};

/// MCP client connected to a spawned `dbmcp`, plus its config directory.
pub struct ConfigClient {
    /// Running MCP client.
    pub client: RunningService<rmcp::RoleClient, ()>,
    /// Keeps the config file alive until the client is dropped.
    _dir: tempfile::TempDir,
}

/// Writes `yaml` to `dbmcp.yaml` and connects to `dbmcp stdio --config` it.
///
/// `files` are written beside the config as `(name, contents)`. `env` is
/// applied after every inherited `DB_*` var is removed.
pub async fn connect(yaml: &str, files: &[(&str, &str)], env: &[(&str, &str)]) -> ConfigClient {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("dbmcp.yaml");
    std::fs::write(&path, yaml).expect("write config");
    for (name, contents) in files {
        std::fs::write(dir.path().join(name), contents).expect("write file");
    }

    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_dbmcp"));
    command.arg("stdio").arg("--config").arg(&path);
    for (key, _) in std::env::vars_os() {
        if key.to_string_lossy().starts_with("DB_") {
            command.env_remove(key);
        }
    }
    command.envs(env.iter().copied());

    let transport = TokioChildProcess::new(command).expect("spawn dbmcp");
    let client = ().serve(transport).await.expect("mcp handshake");
    ConfigClient { client, _dir: dir }
}

impl ConfigClient {
    /// Calls `readQuery` and returns its structured result.
    pub async fn read_query(&self, query: &str) -> Value {
        let arguments = json!({ "query": query }).as_object().cloned().expect("object");
        let result = self
            .client
            .call_tool(CallToolRequestParams::new("readQuery").with_arguments(arguments))
            .await
            .expect("tools/call");
        assert_ne!(result.is_error, Some(true), "tool error: {result:?}");
        result.structured_content.expect("structured content")
    }

    /// Returns whether the server exposes `writeQuery`.
    pub async fn exposes_write_query(&self) -> bool {
        let tools = self.client.list_all_tools().await.expect("tools/list");
        tools.iter().any(|tool| tool.name == "writeQuery")
    }

    /// Shuts the client down.
    pub async fn shutdown(self) {
        self.client.cancel().await.expect("shutdown");
    }
}

/// Returns a config file for `backend` at `host:port` with `read_only: true`.
fn network_yaml(backend: &str, host: &str, port: u16) -> String {
    format!("database:\n  backend: {backend}\n  host: {host}\n  port: {port}\n  name: app\n  read_only: true\n")
}

/// Asserts the seeded `users` row is readable through `client`.
async fn assert_reads_seed(client: &ConfigClient) {
    let result = client.read_query("SELECT name FROM users WHERE id = 1").await;
    assert_eq!(result["rows"], json!([{ "name": "Alice Johnson" }]), "{result}");
}

/// Connects to a network `backend` configured only through the config file.
pub async fn assert_file_supplies_connection_settings(backend: &str, host: &str, port: u16) {
    let client = connect(&network_yaml(backend, host, port), &[], &[]).await;
    assert_reads_seed(&client).await;
    assert!(!client.exposes_write_query().await, "file read_only must apply");
    client.shutdown().await;
}

/// Connects with a wrong host in the file that `DB_HOST`/`DB_PORT` override.
pub async fn assert_env_overrides_file(backend: &str, host: &str, port: u16) {
    let port = port.to_string();
    let env = [("DB_HOST", host), ("DB_PORT", port.as_str()), ("DB_READ_ONLY", "false")];
    let client = connect(&network_yaml(backend, "invalid.invalid", 1), &[], &env).await;
    assert_reads_seed(&client).await;
    assert!(
        client.exposes_write_query().await,
        "env DB_READ_ONLY must beat the file"
    );
    client.shutdown().await;
}
