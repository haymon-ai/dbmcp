//! Loader inputs: the explicit file paths, the process environment, and the home directory.
//!
//! The CLI fills it from its two flags and the process, and the loader
//! never reads `std::env` itself.

use std::collections::HashMap;
use std::path::PathBuf;

/// Inputs to [`load`](crate::load).
#[derive(Debug, Clone, Default)]
pub struct LoadOptions {
    /// `--config-file` values in order; non-empty beats `DBMCP_CONFIG_FILE`.
    pub config_files: Vec<PathBuf>,
    /// `--env-file` values in order; non-empty beats `DBMCP_ENV_FILE`.
    pub env_files: Vec<PathBuf>,
    /// Process environment, the highest-ranked settings source.
    pub environment: HashMap<String, String>,
    /// Directory relative `--config-file` / `--env-file` paths resolve against.
    pub working_dir: PathBuf,
    /// Home directory for `~` expansion and the default `known_hosts`.
    pub home: Option<PathBuf>,
}

/// Returns the user's home directory, treating an empty path as absent.
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    std::env::home_dir().filter(|home| !home.as_os_str().is_empty())
}
