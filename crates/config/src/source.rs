//! Where a setting's winning value came from.
//!
//! The loader records one [`Source`] per set value and appends it to error
//! messages.

use std::fmt;
use std::path::PathBuf;

/// The input that supplied a setting's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A process environment variable.
    Env(&'static str),
    /// A key in the env file, with its 1-based line.
    EnvFile {
        /// Env var name.
        name: &'static str,
        /// Absolute env file path.
        path: PathBuf,
        /// 1-based line of the key.
        line: usize,
    },
    /// A value in the YAML config file, with its 1-based line and column.
    File {
        /// Absolute config file path.
        path: PathBuf,
        /// 1-based line.
        line: u64,
        /// 1-based column.
        column: u64,
    },
}

impl fmt::Display for Source {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Env(name) => f.write_str(name),
            Self::EnvFile { name, path, line } => write!(f, "{name} in {}:{line}", path.display()),
            Self::File { path, line, column } => write!(f, "{}:{line}:{column}", path.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_names_the_input() {
        assert_eq!(Source::Env("DBMCP_DB_PORT").to_string(), "DBMCP_DB_PORT");
        let env_file = Source::EnvFile {
            name: "DBMCP_DB_PORT",
            path: "/etc/dbmcp.env".into(),
            line: 3,
        };
        assert_eq!(env_file.to_string(), "DBMCP_DB_PORT in /etc/dbmcp.env:3");
        let file = Source::File {
            path: "/etc/dbmcp.yaml".into(),
            line: 12,
            column: 5,
        };
        assert_eq!(file.to_string(), "/etc/dbmcp.yaml:12:5");
    }
}
