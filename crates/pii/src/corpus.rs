//! Test/bench-only fixture loader. Gated behind the `test-support` feature.

use std::fs;
use std::path::PathBuf;

/// Two-bucket recognizer fixture loaded from `corpus/{name}.toml`.
#[derive(Debug, serde::Deserialize)]
pub struct Corpus {
    /// Examples that MUST surface the recognizer's entity type.
    #[serde(default)]
    pub positives: Vec<String>,
    /// Examples that MUST NOT surface the recognizer's entity type.
    #[serde(default)]
    pub negatives: Vec<String>,
}

impl Corpus {
    /// Load `crates/pii/corpus/{stem}.toml`.
    ///
    /// `stem` is the bare entity stem (`"email"`, `"credit_card"`, …) — the
    /// `.toml` extension is appended.
    ///
    /// # Panics
    ///
    /// Panics on read or parse failure; fixtures are checked into the repo
    /// and the parser is only invoked from tests and benches.
    #[must_use]
    pub fn load(stem: &str) -> Self {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("corpus")
            .join(stem)
            .with_extension("toml");
        let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read corpus {}: {e}", path.display()));
        toml::from_str(&raw).unwrap_or_else(|e| panic!("parse corpus {}: {e}", path.display()))
    }
}
