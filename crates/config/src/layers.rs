//! The env layers: the process env above the env files, read in two views.
//!
//! [`EnvLayers::read`] picks the env files (`--env` list > process
//! `DBMCP_ENV_FILE` list) and reads them in order, stopping at the first
//! file that cannot be read or parsed. Each file's `${}` and bare `KEY` lines
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
use std::path::Path;

use crate::dotenv::{self, EnvFile};
use crate::env::{self, ENV_FILE_VAR};
use crate::loader::{LoadErrorKind, file_list, non_empty, read_file};
use crate::options::LoadOptions;
use crate::source::Source;

/// The process env stacked above the env files, in list order.
#[derive(Debug)]
pub(crate) struct EnvLayers<'a> {
    process: &'a HashMap<String, String>,
    files: Vec<EnvFile>,
}

impl<'a> EnvLayers<'a> {
    /// Reads the env files named by `--env` or process `DBMCP_ENV_FILE`, in order.
    ///
    /// The warnings of the files read so far go to `warnings`, even on failure.
    ///
    /// # Errors
    ///
    /// Returns the first list, read, or parse failure.
    pub(crate) fn read(options: &'a LoadOptions, warnings: &mut Vec<String>) -> Result<Self, LoadErrorKind> {
        let list = non_empty(&options.environment, ENV_FILE_VAR).map(|list| (list, ENV_FILE_VAR.to_owned()));
        let (paths, origin) = file_list(&options.env_files, "--env", list)?;
        let mut layers = Self {
            process: &options.environment,
            files: Vec::new(),
        };
        for raw in paths {
            let path = options
                .working_dir
                .join(env::expand_tilde(&raw, options.home.as_deref()));
            let text = read_file(&path).map_err(|source| LoadErrorKind::Read {
                what: "env file",
                path: path.clone(),
                origin: origin.clone(),
                source,
            })?;
            let process = |name: &str| layers.process.get(name).cloned();
            let earlier = |name: &str| layers.latest(name).map(|(_, value, _)| value.to_owned());
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
    pub(crate) fn var(&self, name: &str) -> Option<String> {
        self.process
            .get(name)
            .cloned()
            .or_else(|| self.latest(name).map(|(_, value, _)| value.to_owned()))
    }

    /// Returns every non-empty value of `name` in the setting view, highest first.
    ///
    /// Yields the process value, then each env file's value from the last
    /// file to the first. `eligible` marks the values that may win: the
    /// process value, and the last env file's value unless that file sets
    /// `name` empty. The others lose but must still be checked.
    pub(crate) fn candidates(&self, name: &'static str) -> impl Iterator<Item = (&str, Source, bool)> {
        let process = non_empty(self.process, name).map(|text| (text, Source::Env(name), true));
        let last = self.files.iter().rposition(|file| file.get(name).is_some());
        let files = self.files.iter().enumerate().rev().filter_map(move |(i, file)| {
            let (text, line) = file.get_with_line(name).filter(|(text, _)| !text.is_empty())?;
            let source = Source::EnvFile {
                name,
                path: file.path().to_path_buf(),
                line,
            };
            Some((text, source, Some(i) == last))
        });
        process.into_iter().chain(files)
    }

    /// Returns the list position of the first env file read from `path`.
    pub(crate) fn position(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|file| file.path() == path)
    }

    /// Returns the last env file that sets `name`, with its value and line.
    fn latest(&self, name: &str) -> Option<(&EnvFile, &str, usize)> {
        self.files
            .iter()
            .rev()
            .find_map(|file| file.get_with_line(name).map(|(value, line)| (file, value, line)))
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;

    /// A scratch working directory holding env files.
    struct Fixture {
        dir: TempDir,
        options: LoadOptions,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let options = LoadOptions {
                working_dir: dir.path().to_path_buf(),
                ..LoadOptions::default()
            };
            Self { dir, options }
        }

        fn env(mut self, name: &str, value: &str) -> Self {
            self.options.environment.insert(name.into(), value.into());
            self
        }

        fn env_file(mut self, name: &str, text: &str) -> Self {
            fs::write(self.path(name), text).expect("write");
            self.options.env_files.push(name.into());
            self
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn layers(&self) -> EnvLayers<'_> {
            EnvLayers::read(&self.options, &mut Vec::new()).expect("valid env files")
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
                ("process", Source::Env("DBMCP_X"), true),
                (
                    "b",
                    Source::EnvFile {
                        name: "DBMCP_X",
                        path: fixture.path("b.env"),
                        line: 2,
                    },
                    true,
                ),
                (
                    "a",
                    Source::EnvFile {
                        name: "DBMCP_X",
                        path: fixture.path("a.env"),
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
            .map(|(text, _, eligible)| (text, eligible))
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
        let error = EnvLayers::read(&fixture.options, &mut warnings).expect_err("broken b.env");
        assert!(matches!(error, LoadErrorKind::EnvFile { .. }), "{error}");
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
