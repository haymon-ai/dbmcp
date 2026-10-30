//! MCP transport selector.

/// Supported MCP transports.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Transport {
    /// JSON-RPC over stdin/stdout (default).
    #[default]
    Stdio,
    /// Streamable HTTP.
    Http,
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stdio => write!(f, "stdio"),
            Self::Http => write!(f, "http"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_stdio() {
        assert_eq!(Transport::default(), Transport::Stdio);
    }

    #[test]
    fn display_lowercase() {
        assert_eq!(Transport::Stdio.to_string(), "stdio");
        assert_eq!(Transport::Http.to_string(), "http");
    }
}
