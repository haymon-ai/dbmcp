//! The config files: each YAML file read, interpolated, and structure-checked.
//!
//! [`ConfigFiles::read`] picks the files (`--config` list > `DBMCP_CONFIG`
//! list in the setting view of the env layers) and handles them one at a
//! time in order: read, parse into an untyped tree, `${}`-interpolate against
//! the variable view of the env layers, and check against the config schema
//! alone. A read, parse, or interpolation failure aborts the load. The first
//! file with structure errors is the last file read: its errors are kept
//! for [`settings::check`](crate::settings::check) to report beside every
//! value error, so one load reports every problem at once.
//!
//! [`ConfigFiles::candidates`] then yields a setting's value from each file,
//! the last file first.

use std::path::{Path, PathBuf};

use crate::env::{CONFIG_VAR, Raw};
use crate::interpolate;
use crate::layers::EnvLayers;
use crate::loader::{LoadErrorKind, file_list, read_file};
use crate::node::{self, Node, SpannedNode};
use crate::options::LoadOptions;
use crate::schema;
use crate::settings::InputError;
use crate::source::Source;

/// The interpolated config files in list order, plus the stopping file's structure errors.
#[derive(Debug, Default)]
pub(crate) struct ConfigFiles {
    files: Vec<(PathBuf, SpannedNode)>,
    structure: Vec<InputError>,
}

impl ConfigFiles {
    /// Reads, parses, interpolates, and structure-checks the named config files.
    ///
    /// `${}` warnings go to `warnings`, prefixed with the file path.
    ///
    /// # Errors
    ///
    /// Returns the first list, read, parse, or interpolation failure.
    pub(crate) fn read(
        options: &LoadOptions,
        layers: &EnvLayers<'_>,
        warnings: &mut Vec<String>,
    ) -> Result<Self, LoadErrorKind> {
        let list = layers
            .candidates(CONFIG_VAR)
            .find(|(_, _, eligible)| *eligible)
            .map(|(list, source, _)| (list, source.to_string()));
        let Some((paths, origin)) = file_list(&options.config, "--config", list)? else {
            return Ok(Self::default());
        };
        let lookup = |name: &str| layers.var(name);
        let mut files = Vec::new();
        for raw in paths {
            let path = options.working_dir.join(raw);
            let text = read_file(&path, "config file", &origin)?;
            let mut tree = node::parse(&text).map_err(|error| {
                let location = error
                    .location()
                    .filter(|l| l.line() > 0)
                    .map(|l| format!("{}:{}: ", l.line(), l.column()))
                    .unwrap_or_default();
                let message = error.to_string();
                let message = message.lines().next().unwrap_or_default();
                LoadErrorKind::Parse {
                    errors: vec![format!("{}:{location}{message}", path.display())],
                    path: path.clone(),
                }
            })?;
            let in_file = |messages: &[String]| -> Vec<String> {
                messages.iter().map(|m| format!("{}:{m}", path.display())).collect()
            };
            let outcome = interpolate::interpolate(&mut tree, &lookup);
            warnings.extend(in_file(&outcome.warnings));
            if !outcome.errors.is_empty() {
                return Err(LoadErrorKind::Interpolate {
                    errors: in_file(&outcome.errors),
                    path,
                });
            }
            let structure = schema::validate(&tree, &path);
            files.push((path, tree));
            if !structure.is_empty() {
                return Ok(Self { files, structure });
            }
        }
        Ok(Self {
            files,
            structure: Vec::new(),
        })
    }

    /// Returns the structure errors of the file that stopped the read, if any.
    pub(crate) fn structure(&self) -> &[InputError] {
        &self.structure
    }

    /// Returns the node at the dotted `path` in each file setting it, last first.
    ///
    /// Each value comes with its file's directory, which relative paths
    /// resolve against. A null or unset value is yielded too; its setting's
    /// check treats it as unset, so an earlier file shows through.
    pub(crate) fn candidates(&self, path: &str) -> impl Iterator<Item = (Raw<'_>, Source)> {
        self.files.iter().rev().filter_map(move |(file, root)| {
            let node = leaf(root, path)?;
            let source = Source::File {
                path: file.clone(),
                line: node.referenced.line(),
                column: node.referenced.column(),
            };
            let dir = file.parent().unwrap_or(Path::new(""));
            Some((Raw::Node { node: &node.value, dir }, source))
        })
    }

    /// Returns the list position of the first config file read from `path`.
    pub(crate) fn position(&self, path: &Path) -> Option<usize> {
        self.files.iter().position(|(file, _)| file == path)
    }
}

/// Returns the node at the dotted `path`, if every key on the way is a mapping entry.
fn leaf<'a>(root: &'a SpannedNode, path: &str) -> Option<&'a SpannedNode> {
    path.split('.').try_fold(root, |node, key| match &node.value {
        Node::Map(entries) => entries.get(key).map(|(_, value)| value),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::*;

    /// A scratch working directory holding config files.
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

        fn config(mut self, name: &str, text: &str) -> Self {
            let path = self.path(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(path, text).expect("write");
            self.options.config.push(name.into());
            self
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn read(&self) -> ConfigFiles {
            let layers = EnvLayers::read(&self.options, &mut Vec::new()).expect("no env files");
            ConfigFiles::read(&self.options, &layers, &mut Vec::new()).expect("readable config files")
        }
    }

    /// Returns each candidate's source and directory.
    fn sources(files: &ConfigFiles, path: &str) -> Vec<(Source, PathBuf)> {
        files
            .candidates(path)
            .map(|(raw, source)| {
                let Raw::Node { dir, .. } = raw else {
                    panic!("config candidates are nodes");
                };
                (source, dir.to_path_buf())
            })
            .collect()
    }

    #[test]
    fn candidates_come_last_file_first_with_source_and_directory() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  host: a\n")
            .config("sub/b.yaml", "\ndatabase:\n    host: b\n");
        let files = fixture.read();
        assert_eq!(
            sources(&files, "database.host"),
            [
                (
                    Source::File {
                        path: fixture.path("sub/b.yaml"),
                        line: 3,
                        column: 11,
                    },
                    fixture.path("sub"),
                ),
                (
                    Source::File {
                        path: fixture.path("a.yaml"),
                        line: 2,
                        column: 9,
                    },
                    fixture.path(""),
                ),
            ]
        );
        assert_eq!(files.position(&fixture.path("sub/b.yaml")), Some(1));
        assert!(files.structure().is_empty());
    }

    #[test]
    fn files_without_the_key_yield_nothing_and_null_is_yielded() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  host: a\n")
            .config("b.yaml", "database:\n  host:\n")
            .config("c.yaml", "log_level: warn\n");
        let candidates: Vec<_> = fixture
            .read()
            .candidates("database.host")
            .map(|(raw, _)| match raw {
                Raw::Node { node, .. } => node.clone(),
                Raw::Text(_) => panic!("config candidates are nodes"),
            })
            .collect();
        assert_eq!(candidates, [Node::Null, Node::Str("a".into())]);
    }

    #[test]
    fn reading_stops_at_the_first_file_with_structure_errors_and_keeps_them() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  host: a\n")
            .config("b.yaml", "database:\n  hots: b\n")
            .config("c.yaml", "database:\n  host: c\n");
        let files = fixture.read();
        assert_eq!(files.candidates("database.host").count(), 1, "c.yaml is not read");
        let structure: Vec<_> = files.structure().iter().map(ToString::to_string).collect();
        assert_eq!(structure.len(), 1, "{structure:?}");
        assert!(structure[0].contains("database.hots"), "{structure:?}");
    }
}
