//! Log severity selector shared by the CLI and the config file.

/// Log severity levels for the MCP server.
///
/// The CLI maps each variant onto the matching `tracing::Level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum LogLevel {
    /// Only errors.
    Error,
    /// Warnings and above.
    Warn,
    /// Informational and above (default).
    Info,
    /// Debug and above.
    Debug,
    /// All trace output.
    Trace,
}

impl LogLevel {
    /// Default log level.
    pub const DEFAULT: Self = Self::Info;
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Error => write!(f, "error"),
            Self::Warn => write!(f, "warn"),
            Self::Info => write!(f, "info"),
            Self::Debug => write!(f, "debug"),
            Self::Trace => write!(f, "trace"),
        }
    }
}
