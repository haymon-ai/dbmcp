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

/// A running `dbmcp` HTTP server, killed on drop.
pub struct HttpServer {
    _child: Child,
    port: u16,
}

impl HttpServer {
    /// Starts `command` on a free loopback port and waits until it listens.
    pub async fn spawn(mut command: Command) -> Self {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|listener| listener.local_addr())
            .expect("reserve free port")
            .port();
        let child = command
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_HTTP_PORT", port.to_string())
            .env("DBMCP_HTTP_AUTH_TOKEN", AUTH_TOKEN)
            .stdin(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
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
