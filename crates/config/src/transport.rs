//! MCP transport selector.

/// Supported MCP transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// JSON-RPC over stdin/stdout.
    Stdio,
    /// Streamable HTTP.
    Http,
}

impl Transport {
    /// Accepted setting values, in the order error messages list them.
    pub(crate) const CHOICES: &[(&str, Self)] = &[("stdio", Self::Stdio), ("http", Self::Http)];
}
