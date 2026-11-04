//! The env layers: the process env above the env files, read in two views.
//!
//! [`EnvLayers::read`] parses the env files [`read`](super::read) picked,
//! in order, stopping at the first file that cannot be read or parsed. Each file's `${}` and bare `KEY` lines
//! see the process env, then the file's own earlier lines, then the files
//! read before it.
//!
//! The two views deliberately differ on empty values:
//!
//! - The variable view ([`EnvLayers::var`]) reads a name as a shell would:
//!   process env first, then the last env file setting it. An empty value is
//!   still a value, which is what makes `${VAR-default}` differ from
//!   `${VAR:-default}`.
//! - The setting view ([`EnvLayers::candidates`]) reads a `DBMCP_*` name: an
//!   empty value is unset, so a non-empty process value wins, else the last
//!   env file setting the name decides it, and an empty value there clears it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::dotenv::{self, EnvFile};
use crate::loader::LoadErrorKind;
use crate::source::Source;

/// The process env stacked above the env files, in list order.
#[derive(Debug)]
pub(super) struct EnvLayers<'a> {
    process: &'a HashMap<String, String>,
    files: Vec<EnvFile>,
}

impl<'a> EnvLayers<'a> {
    /// Parses the env `files`, in order, above the `process` env.
    ///
    /// Each item is a picked file's path and text, or the error reading it.
    /// The warnings of the files parsed so far go to `warnings`, even on failure.
    ///
    /// # Errors
    ///
    /// Returns the first read or parse failure.
    pub(super) fn read(
        process: &'a HashMap<String, String>,
        files: impl IntoIterator<Item = Result<(PathBuf, String), LoadErrorKind>>,
        warnings: &mut Vec<String>,
    ) -> Result<Self, LoadErrorKind> {
        let mut layers = Self {
            process,
            files: Vec::new(),
        };
        for file in files {
            let (path, text) = file?;
            let process = |name: &str| layers.process.get(name).cloned();
            let earlier = |name: &str| layers.latest(name).map(str::to_owned);
            let parsed = dotenv::parse(&path, &text, &process, &earlier)
                .map_err(|errors| LoadErrorKind::EnvFile { path, errors })?;
            warnings.extend(parsed.warnings);
            layers.files.push(parsed.file);
        }
        Ok(layers)
    }

    /// Returns `name` in the variable view: process env, then the last env file.
    ///
    /// An empty value is returned as such, not skipped.
    pub(super) fn var(&self, name: &str) -> Option<String> {
        self.process
            .get(name)
            .cloned()
            .or_else(|| self.latest(name).map(str::to_owned))
    }

    /// Returns every non-empty value of `name` in the setting view, highest first.
    ///
    /// Yields the process value, then each env file's value from the last
    /// file to the first, with that file's directory, which relative paths
    /// resolve against. `eligible` marks the values that may win: the
    /// process value, and the last env file's value unless that file sets
    /// `name` empty. The others lose but must still be checked.
    pub(super) fn candidates(&self, name: &'static str) -> impl Iterator<Item = (&str, Option<&Path>, Source, bool)> {
        let process = non_empty(self.process, name).map(|text| (text, None, Source::Env(name), true));
        let last = self.files.iter().rposition(|file| file.get(name).is_some());
        let files = self.files.iter().enumerate().rev().filter_map(move |(i, file)| {
            let (text, line) = file.get_with_line(name).filter(|(text, _)| !text.is_empty())?;
            let source = Source::EnvFile {
                name,
                path: file.path().to_path_buf(),
                line,
            };
            let dir = file.path().parent().unwrap_or(Path::new(""));
            Some((text, Some(dir), source, Some(i) == last))
        });
        process.into_iter().chain(files)
    }

    /// Returns the list position of the first env file read from `path`.
    pub(super) fn position(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|file| file.path() == path)
    }

    /// Returns the value of `name` in the last env file that sets it.
    fn latest(&self, name: &str) -> Option<&str> {
        self.files.iter().rev().find_map(|file| file.get(name))
    }
}

/// Returns the value of `name` in `environment`, unless unset or empty.
pub(super) fn non_empty<'a>(environment: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    environment.get(name).map(String::as_str).filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Returns the path of the picked file `name`.
    fn path(name: &str) -> PathBuf {
        Path::new("dir").join(name)
    }

    /// The process env and the picked env files' paths and texts.
    #[derive(Default)]
    struct Fixture {
        process: HashMap<String, String>,
        files: Vec<(PathBuf, String)>,
    }

    impl Fixture {
        fn new() -> Self {
            Self::default()
        }

        fn env(mut self, name: &str, value: &str) -> Self {
            self.process.insert(name.into(), value.into());
            self
        }

        fn env_file(mut self, name: &str, text: &str) -> Self {
            self.files.push((path(name), text.into()));
            self
        }

        fn picked(&self) -> impl Iterator<Item = Result<(PathBuf, String), LoadErrorKind>> {
            self.files.clone().into_iter().map(Ok)
        }

        fn layers(&self) -> EnvLayers<'_> {
            EnvLayers::read(&self.process, self.picked(), &mut Vec::new()).expect("valid env files")
        }
    }

    #[test]
    fn var_reads_process_env_then_the_last_env_file() {
        let fixture = Fixture::new()
            .env("P", "process")
            .env("E", "")
            .env_file("a.env", "P=a\nE=a\nA=a\nB=a\n")
            .env_file("b.env", "B=b\nC=\n");
        let layers = fixture.layers();
        assert_eq!(layers.var("P").as_deref(), Some("process"));
        assert_eq!(
            layers.var("E").as_deref(),
            Some(""),
            "an empty process value is a value"
        );
        assert_eq!(layers.var("A").as_deref(), Some("a"));
        assert_eq!(layers.var("B").as_deref(), Some("b"));
        assert_eq!(
            layers.var("C").as_deref(),
            Some(""),
            "an empty env file value is a value"
        );
        assert_eq!(layers.var("NONE"), None);
    }

    #[test]
    fn candidates_list_non_empty_values_highest_first() {
        let fixture = Fixture::new()
            .env("DBMCP_X", "process")
            .env_file("a.env", "DBMCP_X=a\n")
            .env_file("b.env", "\nDBMCP_X=b\n");
        let layers = fixture.layers();
        let candidates: Vec<_> = layers.candidates("DBMCP_X").collect();
        assert_eq!(
            candidates,
            [
                ("process", None, Source::Env("DBMCP_X"), true),
                (
                    "b",
                    Some(Path::new("dir")),
                    Source::EnvFile {
                        name: "DBMCP_X",
                        path: path("b.env"),
                        line: 2,
                    },
                    true,
                ),
                (
                    "a",
                    Some(Path::new("dir")),
                    Source::EnvFile {
                        name: "DBMCP_X",
                        path: path("a.env"),
                        line: 1,
                    },
                    false,
                ),
            ]
        );
    }

    #[test]
    fn empty_values_are_unset_and_a_later_empty_env_file_value_clears() {
        let fixture = Fixture::new()
            .env("DBMCP_X", "")
            .env_file("a.env", "DBMCP_X=a\n")
            .env_file("b.env", "DBMCP_X=\n");
        let layers = fixture.layers();
        let candidates: Vec<_> = layers
            .candidates("DBMCP_X")
            .map(|(text, _, _, eligible)| (text, eligible))
            .collect();
        assert_eq!(candidates, [("a", false)], "checked, but cannot win");
    }

    #[test]
    fn later_env_files_see_earlier_ones_after_process_env() {
        let fixture = Fixture::new()
            .env("P", "process")
            .env_file("a.env", "P=a\nA=a\n")
            .env_file("b.env", "X=${P},${A}\nA\n");
        let layers = fixture.layers();
        assert_eq!(layers.var("X").as_deref(), Some("process,a"));
        assert_eq!(
            layers.var("A").as_deref(),
            Some("a"),
            "a bare key inherits from earlier files"
        );
    }

    #[test]
    fn warnings_of_files_read_before_a_failure_are_kept() {
        let fixture = Fixture::new()
            .env_file("a.env", "A=${NOPE}\n")
            .env_file("b.env", "B C=1\n");
        let mut warnings = Vec::new();
        let error = EnvLayers::read(&fixture.process, fixture.picked(), &mut warnings).expect_err("broken b.env");
        assert!(matches!(error, LoadErrorKind::EnvFile { .. }), "{error}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
