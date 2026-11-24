//! Log severity selector.

/// Log severity levels for the MCP server.
///
/// Parsed from the `log_level` setting; the CLI maps it to a tracing level.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    /// Only errors.
    Error,
    /// Warnings and above.
    Warn,
    /// Informational and above (default).
    #[default]
    Info,
    /// Debug and above.
    Debug,
    /// All trace output.
    Trace,
}

impl LogLevel {
    /// Returns the setting value.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
            Self::Trace => "trace",
        }
    }
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
