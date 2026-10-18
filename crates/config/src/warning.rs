//! Non-fatal problems found while loading the configuration.

use std::path::PathBuf;

/// Problem that does not stop loading but should be logged.
///
/// Never carries a variable's value or raw file contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigWarning {
    /// The default `.env` beside the config file exists but could not be read.
    SkippedEnvFile {
        /// Absolute path of the env file.
        path: PathBuf,
        /// Why reading failed.
        reason: String,
    },

    /// A referenced variable is unset and was replaced with `""`.
    UnsetVariable {
        /// Variable name.
        name: String,
        /// Where it was referenced, e.g. `/abs/.env:3`.
        location: String,
    },
}

impl std::fmt::Display for ConfigWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SkippedEnvFile { path, reason } => {
                write!(f, "skipping env file {}: {reason}", path.display())
            }
            Self::UnsetVariable { name, location } => {
                write!(
                    f,
                    "variable {name} is not set; substituting an empty string ({location})"
                )
            }
        }
    }
}
