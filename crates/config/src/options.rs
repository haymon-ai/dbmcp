//! Loader inputs: the explicit file paths, the process environment, and the home directory.
//!
//! The CLI fills it from its two flags and the process, and the loader
//! never reads `std::env` itself.

use std::collections::HashMap;
use std::path::PathBuf;

/// Inputs to [`load`](crate::load).
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// `--config` value; beats `DBMCP_CONFIG`.
    pub config: Option<PathBuf>,
    /// `--env` value; beats `DBMCP_ENV_FILE`.
    pub env_file: Option<PathBuf>,
    /// Process environment, the highest-ranked settings source.
    pub environment: HashMap<String, String>,
    /// Directory relative `--config` / `--env` paths resolve against.
    pub working_dir: PathBuf,
    /// Home directory for `~` expansion and the default `known_hosts`.
    pub home: Option<PathBuf>,
}

/// Returns the user's home directory, treating an empty path as absent.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| !home.as_os_str().is_empty())
}
