//! MCP transport selector.

/// Supported MCP transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Transport {
    /// JSON-RPC over stdin/stdout.
    Stdio,
    /// Streamable HTTP.
    Http,
}
