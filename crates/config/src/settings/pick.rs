//! The file picks: which env or config files to read, and their text.
//!
//! [`files`] takes the flag values, else the comma list, resolves each
//! path, and reads the files one at a time as the caller asks, so a
//! reader that stops at a broken file never reads the files after it.
//! Relative paths resolve against the working directory; only env file
//! paths expand a leading `~`.

use std::path::{Path, PathBuf};

use indexmap::IndexSet;

use crate::env;
use crate::loader::LoadErrorKind;

/// The kind of file a pick names.
#[derive(Debug, Clone, Copy)]
pub(super) enum FileKind {
    /// Env files: `--env-file` or `DBMCP_ENV_FILE`; `~` expands.
    Env,
    /// YAML config files: `--config-file` or `DBMCP_CONFIG_FILE`; `~` stays literal.
    Config,
}

impl FileKind {
    /// Returns the noun used in read errors.
    fn noun(self) -> &'static str {
        match self {
            Self::Env => "env file",
            Self::Config => "config file",
        }
    }

    /// Returns the flag naming files of this kind.
    fn flag(self) -> &'static str {
        match self {
            Self::Env => "--env-file",
            Self::Config => "--config-file",
        }
    }
}

/// Picks the `kind` files named by `flags`, else by the comma `list`.
///
/// A flag given at least once replaces the list; `list` pairs the comma
/// list with where it came from, and a blank list names no files. Yields
/// each file's resolved path and text in order, reading each file only
/// when asked. A file listed twice is read once, at its last place, which
/// keeps its precedence. No list yields nothing.
///
/// # Errors
///
/// Returns [`LoadErrorKind::List`] for an empty flag value or list entry,
/// before any file is read. Each yielded item is [`LoadErrorKind::Read`]
/// when its path is a directory or cannot be read.
pub(super) fn files(
    kind: FileKind,
    flags: &[PathBuf],
    list: Option<(&str, String)>,
    working_dir: &Path,
    home: Option<&Path>,
) -> Result<impl Iterator<Item = Result<(PathBuf, String), LoadErrorKind>> + use<>, LoadErrorKind> {
    let (raw, origin): (Vec<&Path>, String) = if !flags.is_empty() {
        if flags.iter().any(|path| path.as_os_str().is_empty()) {
            return Err(LoadErrorKind::List {
                origin: kind.flag().to_owned(),
                problem: "empty path",
            });
        }
        (flags.iter().map(PathBuf::as_path).collect(), kind.flag().to_owned())
    } else if let Some((list, origin)) = list.filter(|(list, _)| !list.trim().is_empty()) {
        (split_list(list, &origin)?, origin)
    } else {
        (Vec::new(), String::new())
    };
    let paths: IndexSet<PathBuf> = raw
        .into_iter()
        .rev()
        .map(|raw| match kind {
            FileKind::Env => working_dir.join(env::expand_tilde(raw, home)),
            FileKind::Config => working_dir.join(raw),
        })
        .collect();
    Ok(paths.into_iter().rev().map(move |path| {
        let text = read_file(&path, kind.noun(), &origin)?;
        Ok((path, text))
    }))
}

/// Splits a comma list of paths, trimming each entry.
///
/// # Errors
///
/// Returns [`LoadErrorKind::List`] naming `origin` when an entry is empty.
fn split_list<'a>(list: &'a str, origin: &str) -> Result<Vec<&'a Path>, LoadErrorKind> {
    let items: Vec<&str> = list.split(',').map(str::trim).collect();
    if items.iter().any(|item| item.is_empty()) {
        return Err(LoadErrorKind::List {
            origin: origin.to_owned(),
            problem: "empty entry in file list",
        });
    }
    Ok(items.into_iter().map(Path::new).collect())
}

/// Reads the `what` file at `path`, named by `origin`; rejects directories, allows pipes.
///
/// # Errors
///
/// Returns [`LoadErrorKind::Read`] when `path` is a directory or cannot be read.
fn read_file(path: &Path, what: &'static str, origin: &str) -> Result<String, LoadErrorKind> {
    let read = || {
        if std::fs::metadata(path)?.is_dir() {
            return Err(std::io::Error::other("is a directory"));
        }
        std::fs::read_to_string(path)
    };
    read().map_err(|source| LoadErrorKind::Read {
        what,
        path: path.to_path_buf(),
        origin: origin.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Returns the picked paths, or the list error as text.
    fn paths(kind: FileKind, flags: &[&str], list: Option<&str>) -> Result<Vec<PathBuf>, String> {
        let flags: Vec<PathBuf> = flags.iter().map(PathBuf::from).collect();
        let list = list.map(|list| (list, "LIST".to_owned()));
        let files =
            files(kind, &flags, list, Path::new("work"), Some(Path::new("home"))).map_err(|error| error.to_string())?;
        // Nothing exists on disk, so each item is a read error naming its path.
        Ok(files
            .map(|item| match item.expect_err("no such file") {
                LoadErrorKind::Read { path, .. } => path,
                other => panic!("read error expected: {other}"),
            })
            .collect())
    }

    #[test]
    fn flags_replace_the_list_and_neither_picks_nothing() {
        assert_eq!(
            paths(FileKind::Config, &["a.yaml"], Some("b.yaml")),
            Ok(vec![Path::new("work").join("a.yaml")])
        );
        assert_eq!(
            paths(FileKind::Config, &[], Some(" b.yaml , c.yaml")),
            Ok(vec![Path::new("work").join("b.yaml"), Path::new("work").join("c.yaml")])
        );
        assert_eq!(paths(FileKind::Config, &[], None), Ok(Vec::new()));
        assert_eq!(paths(FileKind::Config, &[], Some(" ")), Ok(Vec::new()));
    }

    #[test]
    fn a_file_listed_twice_is_picked_once_at_its_last_place() {
        assert_eq!(
            paths(FileKind::Config, &["a.yaml", "b.yaml", "./a.yaml"], None),
            Ok(vec![
                Path::new("work").join("b.yaml"),
                Path::new("work").join("./a.yaml")
            ])
        );
    }

    #[test]
    fn empty_flag_values_and_list_entries_are_list_errors() {
        assert_eq!(
            paths(FileKind::Env, &["a.env", ""], None),
            Err("--env-file: empty path".to_owned())
        );
        assert_eq!(
            paths(FileKind::Config, &[], Some("a.yaml,,b.yaml")),
            Err("LIST: empty entry in file list".to_owned())
        );
    }

    #[test]
    fn only_env_file_paths_expand_tilde() {
        let tilde = Path::new("~").join("x");
        let tilde = tilde.to_str().expect("utf-8");
        assert_eq!(
            paths(FileKind::Env, &[tilde], None),
            Ok(vec![Path::new("work").join("home").join("x")])
        );
        assert_eq!(
            paths(FileKind::Config, &[tilde], None),
            Ok(vec![Path::new("work").join(tilde)])
        );
    }

    #[test]
    fn files_are_read_lazily_and_read_errors_name_their_origin() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.env"), "A=1\n").expect("write");
        fs::create_dir(dir.path().join("sub")).expect("mkdir");
        let list = Some(("a.env,missing.env,sub", "DBMCP_ENV_FILE".to_owned()));
        let mut files = files(FileKind::Env, &[], list, dir.path(), None).expect("valid list");
        let (path, text) = files.next().expect("first").expect("readable");
        assert_eq!((path, text.as_str()), (dir.path().join("a.env"), "A=1\n"));
        let missing = files.next().expect("second").expect_err("missing");
        assert!(
            missing.to_string().starts_with(&format!(
                "failed to read env file {} (from DBMCP_ENV_FILE): ",
                dir.path().join("missing.env").display()
            )),
            "{missing}"
        );
        let directory = files.next().expect("third").expect_err("directory");
        assert!(directory.to_string().ends_with("is a directory"), "{directory}");
        assert!(files.next().is_none());
    }
}
