//! Checks every setting value from every source and picks each winner.
//!
//! [`check`] runs before any setting is resolved. It takes the config files'
//! structure errors from the config schema, then checks every set value
//! with its [`Kind`]: each leaf of every config file, each non-empty entry of
//! every env file, and each non-empty process env var, even values that lose
//! to a higher source, are overwritten by a later file, or sit in a section
//! that is not active. Path values come out resolved: YAML paths against
//! their own config file's directory. Every failure is an [`InputError`];
//! when there are none, [`Settings`] holds each setting's winning value and
//! its [`Source`]: the env layers' setting view (see
//! [`EnvLayers::candidates`]) > the last config file setting it.
//!
//! [`Kind`]: crate::env::Kind
//! [`EnvLayers::candidates`]: crate::layers::EnvLayers::candidates

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::env::{Raw, SETTINGS, Setting, Value};
use crate::layers::EnvLayers;
use crate::node::{Node, SpannedNode};
use crate::source::Source;

/// One invalid input: a malformed config file or a value that fails its setting's checks.
///
/// Renders value-free as `<source>: <path>: <reason>`.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
#[error("{from}: {path}: {reason}")]
pub(crate) struct InputError {
    /// Where the value or key is.
    pub(crate) from: Source,
    /// Dotted YAML key path, or `<root>`.
    pub(crate) path: String,
    /// Value-free reason, e.g. `must be at least 1`.
    pub(crate) reason: String,
}

impl InputError {
    /// Orders errors by config file, then env file, then process env.
    ///
    /// `rank` gives a file's position in its list.
    fn order(&self, rank: impl Fn(&Path) -> usize) -> (u8, usize, u64, u64, &str) {
        match &self.from {
            Source::File { path, line, column } => (0, rank(path), *line, *column, &self.path),
            Source::EnvFile { path, line, .. } => (1, rank(path), *line as u64, 0, &self.path),
            Source::Env(name) => (2, 0, 0, 0, name),
        }
    }
}

/// An interpolated config file and its absolute path.
pub(crate) type ConfigFile = (PathBuf, SpannedNode);

/// Each set setting's winning value and its source.
#[derive(Debug, Default)]
pub(crate) struct Settings {
    values: HashMap<&'static str, Value>,
    sources: HashMap<&'static str, Source>,
}

impl Settings {
    /// Returns the winning value of `setting`, if any source sets it.
    pub(crate) fn get(&self, setting: Setting) -> Option<&Value> {
        self.values.get(setting.path)
    }

    /// Returns where the winning value of `setting` came from.
    pub(crate) fn source(&self, setting: Setting) -> Option<&Source> {
        self.sources.get(setting.path)
    }

    /// Returns the source of every set setting, by path.
    pub(crate) fn sources(&self) -> &HashMap<&'static str, Source> {
        &self.sources
    }
}

/// Checks every source's values and returns each setting's winner.
///
/// `files` are the interpolated config files and their paths, in list
/// order; `structure` holds the config files' schema errors; `home` expands
/// `~` in paths.
///
/// # Errors
///
/// Returns `structure` and every value that fails its setting's checks,
/// from any source, sorted by [`InputError::order`] and without repeats.
pub(crate) fn check(
    files: &[ConfigFile],
    structure: Vec<InputError>,
    layers: &EnvLayers<'_>,
    home: Option<&Path>,
) -> Result<Settings, Vec<InputError>> {
    let mut errors = structure;
    let mut settings = Settings::default();
    for &setting in SETTINGS {
        let Setting { path, env, kind } = setting;
        // Each candidate is checked; only an eligible one may win.
        let mut candidates: Vec<_> = layers
            .candidates(env)
            .map(|(text, source, eligible)| (Raw::Text(text), source, eligible))
            .collect();
        // A later config file wins where it sets a value; unset ones let earlier files through.
        for (file, root) in files.iter().rev() {
            if let Some(node) = leaf(root, path) {
                let source = Source::File {
                    path: file.clone(),
                    line: node.referenced.line(),
                    column: node.referenced.column(),
                };
                let dir = file.parent().unwrap_or(Path::new(""));
                candidates.push((Raw::Node { node: &node.value, dir }, source, true));
            }
        }
        for (raw, source, eligible) in candidates {
            match kind.check(raw, home) {
                Ok(Some(value)) if eligible && !settings.values.contains_key(path) => {
                    settings.values.insert(path, value);
                    settings.sources.insert(path, source);
                }
                Ok(_) => {}
                Err(reason) => errors.push(InputError {
                    from: source,
                    path: path.to_owned(),
                    reason,
                }),
            }
        }
    }
    if errors.is_empty() {
        return Ok(settings);
    }
    let rank = |path: &Path| {
        files
            .iter()
            .position(|(file, _)| file == path)
            .or_else(|| layers.position(path))
            .unwrap_or(0)
    };
    errors.sort_by(|a, b| a.order(rank).cmp(&b.order(rank)));
    errors.dedup();
    Err(errors)
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
    use crate::env;
    use crate::interpolate;
    use crate::node;
    use crate::options::LoadOptions;
    use crate::schema;

    /// Inputs for [`check`]; the env file is written to a scratch directory.
    struct Inputs {
        dir: TempDir,
        yaml: Option<&'static str>,
        env_file: Option<&'static str>,
        environment: HashMap<String, String>,
    }

    impl Default for Inputs {
        fn default() -> Self {
            Self {
                dir: tempfile::tempdir().expect("tempdir"),
                yaml: None,
                env_file: None,
                environment: HashMap::new(),
            }
        }
    }

    impl Inputs {
        fn yaml(mut self, yaml: &'static str) -> Self {
            self.yaml = Some(yaml);
            self
        }

        fn env_file(mut self, text: &'static str) -> Self {
            self.env_file = Some(text);
            self
        }

        fn env(mut self, name: &str, value: &str) -> Self {
            self.environment.insert(name.into(), value.into());
            self
        }

        /// Returns the env file's absolute path.
        fn env_path(&self) -> PathBuf {
            self.dir.path().join(".env")
        }

        fn check(&self) -> Result<Settings, Vec<InputError>> {
            let root = self.yaml.map(|yaml| node::parse(yaml).expect("valid yaml"));
            let mut options = LoadOptions {
                environment: self.environment.clone(),
                working_dir: self.dir.path().to_path_buf(),
                ..LoadOptions::default()
            };
            if let Some(text) = self.env_file {
                fs::write(self.env_path(), text).expect("write env file");
                options.env_files.push(".env".into());
            }
            let layers = EnvLayers::read(&options, &mut Vec::new()).expect("valid env file");
            let files: Vec<_> = root
                .map(|root| (PathBuf::from("/x/dbmcp.yaml"), root))
                .into_iter()
                .collect();
            let structure = files
                .iter()
                .flat_map(|(path, root)| schema::validate(root, path))
                .collect();
            check(&files, structure, &layers, None)
        }

        #[track_caller]
        fn errors(&self) -> Vec<String> {
            self.check()
                .expect_err("invalid")
                .iter()
                .map(ToString::to_string)
                .collect()
        }
    }

    fn file(line: u64, column: u64) -> Source {
        Source::File {
            path: PathBuf::from("/x/dbmcp.yaml"),
            line,
            column,
        }
    }

    #[test]
    fn winner_is_process_env_then_env_file_then_yaml() {
        let settings = Inputs::default()
            .yaml("database:\n  host: yaml\n  user: yaml\n  port: 1111\n")
            .env_file("DBMCP_DB_HOST=file\nDBMCP_DB_USER=file\nDBMCP_DB_PORT=\n")
            .env("DBMCP_DB_HOST", "process")
            .env("DBMCP_DB_USER", "")
            .check()
            .expect("valid");
        assert_eq!(settings.get(env::DB_HOST), Some(&Value::Text("process".into())));
        assert_eq!(settings.source(env::DB_HOST), Some(&Source::Env("DBMCP_DB_HOST")));
        assert_eq!(settings.get(env::DB_USER), Some(&Value::Text("file".into())));
        assert!(matches!(
            settings.source(env::DB_USER),
            Some(Source::EnvFile { line: 2, .. })
        ));
        assert_eq!(settings.get(env::DB_PORT), Some(&Value::Int(1111)));
        assert_eq!(settings.source(env::DB_PORT), Some(&file(4, 9)));
        assert_eq!(settings.get(env::DB_NAME), None);
    }

    #[test]
    fn unset_yaml_values_let_lower_sources_through() {
        let settings = Inputs::default()
            .yaml("database:\n  port: \"\"\n  charset: \"\"\nssh:\n")
            .check()
            .expect("valid");
        assert_eq!(settings.get(env::DB_PORT), None, "empty text at an integer is unset");
        assert_eq!(settings.get(env::DB_CHARSET), Some(&Value::Text(String::new())));
        assert!(settings.sources().keys().all(|path| *path == "database.charset"));
    }

    #[test]
    fn losing_values_are_still_checked() {
        let inputs = Inputs::default()
            .yaml("database:\n  port: abc\n")
            .env_file("DBMCP_DB_PORT=70000\n")
            .env("DBMCP_DB_PORT", "5432");
        let env_file = inputs.env_path().display().to_string();
        assert_eq!(
            inputs.errors(),
            [
                "/x/dbmcp.yaml:2:9: database.port: expected non-negative integer, found string".to_owned(),
                format!("DBMCP_DB_PORT in {env_file}:1: database.port: must be at most 65535"),
            ]
        );
    }

    #[test]
    fn values_in_inactive_sections_are_checked() {
        let errors = Inputs::default()
            .yaml("http:\n  port: nope\n")
            .env("DBMCP_SSH_PORT", "0")
            .errors();
        assert_eq!(
            errors,
            [
                "/x/dbmcp.yaml:2:9: http.port: expected non-negative integer, found string",
                "DBMCP_SSH_PORT: ssh.port: must be at least 1",
            ]
        );
    }

    #[test]
    fn errors_are_sorted_file_then_env_file_then_env() {
        let inputs = Inputs::default()
            .yaml("ssh:\n  port: 0\ndatabase:\n  pasword: x\n  backend: oracle\n")
            .env_file("# c\nDBMCP_DB_SSL=on\nDBMCP_DB_PAGE_SIZE=0\n")
            .env("DBMCP_TRANSPORT", "sse")
            .env("DBMCP_DB_READ_ONLY", "on");
        let env_file = inputs.env_path().display().to_string();
        assert_eq!(
            inputs.errors(),
            [
                "/x/dbmcp.yaml:2:9: ssh.port: must be at least 1".to_owned(),
                "/x/dbmcp.yaml:4:3: database.pasword: unknown field; expected one of backend, host, port, user, \
                 password, name, charset, ssl, ssl_ca, ssl_cert, ssl_key, ssl_verify_cert, read_only, \
                 max_pool_size, connection_timeout, query_timeout, page_size"
                    .to_owned(),
                "/x/dbmcp.yaml:5:12: database.backend: expected one of mysql, mariadb, postgres, sqlite".to_owned(),
                format!(
                    "DBMCP_DB_SSL in {env_file}:2: database.ssl: expected boolean (true/false/1/0/t/f), found string"
                ),
                format!("DBMCP_DB_PAGE_SIZE in {env_file}:3: database.page_size: must be at least 1"),
                "DBMCP_DB_READ_ONLY: database.read_only: expected boolean (true/false/1/0/t/f), found string"
                    .to_owned(),
                "DBMCP_TRANSPORT: transport: expected one of stdio, http".to_owned(),
            ]
        );
    }

    #[test]
    fn errors_never_echo_values() {
        let errors = Inputs::default()
            .yaml("database:\n  password: 12345\n  backend: s3cret\n")
            .env("DBMCP_DB_PORT", "s3cret")
            .errors();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(!errors.concat().contains("s3cret") && !errors.concat().contains("12345"));
    }

    #[test]
    fn values_under_a_malformed_section_are_skipped() {
        let errors = Inputs::default().yaml("pii: []\n").errors();
        assert_eq!(errors, ["/x/dbmcp.yaml:1:6: pii: expected mapping, found sequence"]);
    }

    #[test]
    fn error_source_is_where_the_value_is() {
        let error = Inputs::default()
            .yaml("database:\n  ssl: 1\n")
            .check()
            .expect_err("invalid");
        assert_eq!(
            error,
            [InputError {
                from: file(2, 8),
                path: "database.ssl".into(),
                reason: "expected boolean (true/false/1/0/t/f), found integer".into(),
            }]
        );
    }

    /// Returns the names `${NAME:?…}` / `${NAME?…}` require in `yaml`.
    fn required_vars(yaml: &str) -> Vec<String> {
        yaml.split("${")
            .skip(1)
            .filter_map(|rest| {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let after = &rest[name.len()..];
                (after.starts_with(":?") || after.starts_with('?')).then_some(name)
            })
            .collect()
    }

    #[test]
    fn docs_yaml_examples_are_valid() {
        let docs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/content/docs");
        let mut checked = 0;
        for entry in std::fs::read_dir(&docs).expect("docs directory") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|ext| ext != "mdx") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("readable doc");
            let mut lines = text.lines().enumerate();
            while let Some((start, line)) = lines.next() {
                if line.trim() != "```yaml" {
                    continue;
                }
                let block: Vec<&str> = lines
                    .by_ref()
                    .map(|(_, l)| l)
                    .take_while(|l| l.trim() != "```")
                    .collect();
                let yaml = block.join("\n");
                let required = required_vars(&yaml);
                let lookup = |name: &str| required.iter().any(|r| r == name).then(|| "placeholder".to_owned());
                let mut root = node::parse(&yaml).unwrap_or_else(|e| panic!("{}:{}: {e}", path.display(), start + 1));
                let outcome = interpolate::interpolate(&mut root, &lookup);
                assert!(
                    outcome.errors.is_empty(),
                    "{}:{}: {:?}",
                    path.display(),
                    start + 1,
                    outcome.errors
                );
                let structure = schema::validate(&root, &path);
                let options = LoadOptions::default();
                let layers = EnvLayers::read(&options, &mut Vec::new()).expect("no env files");
                if let Err(errors) = check(&[(path.clone(), root)], structure, &layers, None) {
                    panic!("{}:{}: {errors:?}", path.display(), start + 1);
                }
                checked += 1;
            }
        }
        assert!(checked >= 4, "found only {checked} yaml examples");
    }
}
