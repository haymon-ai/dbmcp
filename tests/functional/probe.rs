//! Spawns `dbmcp` on the HTTP transport and probes its endpoints.
//!
//! Shared by the per-backend functional tests. Requests are raw
//! HTTP/1.1 over a TCP stream, so no HTTP client dependency is needed.

use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::process::{Child, Command};

/// Bearer token every spawned server requires on `/mcp`.
const AUTH_TOKEN: &str = "probe-test-token";

/// Returns a loopback port that nothing listens on right now.
pub fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("reserve free port")
        .port()
}

/// Returns a `dbmcp` command for `backend` with the test user and the `app` database.
#[allow(dead_code, reason = "used by the networked backends only")]
pub fn backend_command(backend: &str, default_user: &str) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dbmcp"));
    command
        .env("DBMCP_DB_BACKEND", backend)
        .env(
            "DBMCP_DB_USER",
            std::env::var("DBMCP_DB_USER").unwrap_or_else(|_| default_user.into()),
        )
        .env("DBMCP_DB_NAME", "app");
    command
}

/// Points `command` at a closed loopback port so the database is unreachable.
#[allow(dead_code, reason = "used by the networked backends only")]
pub fn unreachable(mut command: Command) -> Command {
    command
        .env("DBMCP_DB_HOST", "127.0.0.1")
        .env("DBMCP_DB_PORT", free_port().to_string())
        .env("DBMCP_DB_CONNECTION_TIMEOUT", "2");
    command
}

/// Asserts both probes answer `200` without a token while `/mcp` demands one.
pub async fn assert_ready(command: Command) {
    let server = HttpServer::spawn(command).await;
    assert_eq!(server.get("/healthz").await, 200);
    assert_eq!(server.get("/readyz").await, 200);
    assert_eq!(server.get("/mcp").await, 401, "MCP endpoint must stay behind the token");
}

/// Asserts liveness holds while readiness answers `503`.
pub async fn assert_unready(command: Command) {
    let server = HttpServer::spawn(command).await;
    assert_eq!(server.get("/healthz").await, 200);
    assert_eq!(server.get("/readyz").await, 503);
}

/// A running `dbmcp` HTTP server, killed on drop.
pub struct HttpServer {
    _child: Child,
    port: u16,
}

impl HttpServer {
    /// Starts `command` on a free loopback port and waits until it listens.
    pub async fn spawn(mut command: Command) -> Self {
        let port = free_port();
        let child = command
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_HTTP_PORT", port.to_string())
            .env("DBMCP_HTTP_AUTH_TOKEN", AUTH_TOKEN)
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn dbmcp");

        for _ in 0..100 {
            if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
                return Self { _child: child, port };
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("dbmcp did not listen on port {port} within 10s");
    }

    /// Sends `GET path` without credentials and returns the status code.
    pub async fn get(&self, path: &str) -> u16 {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).await.expect("connect");
        let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).await.expect("write request");
        let mut response = String::new();
        stream.read_to_string(&mut response).await.expect("read response");
        response
            .split_whitespace()
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("malformed response: {response}"))
    }
}
