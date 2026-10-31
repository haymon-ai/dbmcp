//! Checks every setting value from every source and picks each winner.
//!
//! [`check`] runs before any setting is resolved. It validates the config
//! file's structure against the config schema, then checks every set value
//! with its [`Kind`]: each YAML leaf, each non-empty env-file entry, and each
//! non-empty process env var, even values that lose to a higher source or sit
//! in a section that is not active. Every failure is an [`InputError`]; when
//! there are none, [`Settings`] holds each setting's winning value
//! (non-empty process env > non-empty env file > YAML) and its [`Source`].
//!
//! [`Kind`]: crate::env::Kind

use std::collections::HashMap;
use std::path::Path;

use crate::dotenv::EnvFile;
use crate::env::{Raw, SETTINGS, Setting, Value};
use crate::node::{Node, SpannedNode};
use crate::schema;
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
    /// Orders errors by config file location, then env file line, then env name.
    fn order(&self) -> (u8, u64, u64, &str) {
        match &self.from {
            Source::File { line, column, .. } => (0, *line, *column, &self.path),
            Source::EnvFile { line, .. } => (1, *line as u64, 0, &self.path),
            Source::Env(name) => (2, 0, 0, name),
        }
    }
}

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
/// `file` is the interpolated config file and its path.
///
/// # Errors
///
/// Returns every structure error in the file and every value that fails its
/// setting's checks, from any source, sorted by [`InputError::order`].
pub(crate) fn check(
    file: Option<(&Path, &SpannedNode)>,
    env_file: Option<&EnvFile>,
    environment: &HashMap<String, String>,
) -> Result<Settings, Vec<InputError>> {
    let mut errors = file
        .map(|(path, root)| schema::validate(root, path))
        .unwrap_or_default();
    let mut settings = Settings::default();
    for &setting in SETTINGS {
        let Setting { path, env, kind } = setting;
        let mut candidates = Vec::new();
        if let Some(text) = environment.get(env).filter(|text| !text.is_empty()) {
            candidates.push((Raw::Text(text), Source::Env(env)));
        }
        if let Some(file) = env_file
            && let Some((text, line)) = file.get_with_line(env).filter(|(text, _)| !text.is_empty())
        {
            let source = Source::EnvFile {
                name: env,
                path: file.path().to_path_buf(),
                line,
            };
            candidates.push((Raw::Text(text), source));
        }
        if let Some((file, root)) = file
            && let Some(node) = leaf(root, path)
        {
            let source = Source::File {
                path: file.to_path_buf(),
                line: node.referenced.line(),
                column: node.referenced.column(),
            };
            candidates.push((Raw::Node(&node.value), source));
        }
        for (raw, source) in candidates {
            match kind.check(raw) {
                Ok(Some(value)) if !settings.values.contains_key(path) => {
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
    errors.sort_by(|a, b| a.order().cmp(&b.order()));
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
    use std::path::PathBuf;

    use super::*;
    use crate::dotenv;
    use crate::env;
    use crate::interpolate;
    use crate::node;

    /// In-memory inputs for [`check`].
    #[derive(Default)]
    struct Inputs {
        yaml: Option<&'static str>,
        env_file: Option<&'static str>,
        environment: HashMap<String, String>,
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

        fn check(&self) -> Result<Settings, Vec<InputError>> {
            let root = self.yaml.map(|yaml| node::parse(yaml).expect("valid yaml"));
            let env_file = self.env_file.map(|text| {
                dotenv::parse(Path::new("/x/.env"), text, &|_| None)
                    .expect("valid env file")
                    .file
            });
            check(
                root.as_ref().map(|root| (Path::new("/x/dbmcp.yaml"), root)),
                env_file.as_ref(),
                &self.environment,
            )
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
        let errors = Inputs::default()
            .yaml("database:\n  port: abc\n")
            .env_file("DBMCP_DB_PORT=70000\n")
            .env("DBMCP_DB_PORT", "5432")
            .errors();
        assert_eq!(
            errors,
            [
                "/x/dbmcp.yaml:2:9: database.port: expected non-negative integer, found string",
                "DBMCP_DB_PORT in /x/.env:1: database.port: must be at most 65535",
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
        let errors = Inputs::default()
            .yaml("ssh:\n  port: 0\ndatabase:\n  pasword: x\n  backend: oracle\n")
            .env_file("# c\nDBMCP_DB_SSL=on\nDBMCP_DB_PAGE_SIZE=0\n")
            .env("DBMCP_TRANSPORT", "sse")
            .env("DBMCP_DB_READ_ONLY", "on")
            .errors();
        assert_eq!(
            errors,
            [
                "/x/dbmcp.yaml:2:9: ssh.port: must be at least 1",
                "/x/dbmcp.yaml:4:3: database.pasword: unknown field; expected one of backend, host, port, user, \
                 password, name, charset, ssl, ssl_ca, ssl_cert, ssl_key, ssl_verify_cert, read_only, \
                 max_pool_size, connection_timeout, query_timeout, page_size",
                "/x/dbmcp.yaml:5:12: database.backend: expected one of mysql, mariadb, postgres, sqlite",
                "DBMCP_DB_SSL in /x/.env:2: database.ssl: expected boolean (true/false/1/0/t/f), found string",
                "DBMCP_DB_PAGE_SIZE in /x/.env:3: database.page_size: must be at least 1",
                "DBMCP_DB_READ_ONLY: database.read_only: expected boolean (true/false/1/0/t/f), found string",
                "DBMCP_TRANSPORT: transport: expected one of stdio, http",
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
                if let Err(errors) = check(Some((&path, &root)), None, &HashMap::new()) {
                    panic!("{}:{}: {errors:?}", path.display(), start + 1);
                }
                checked += 1;
            }
        }
        assert!(checked >= 4, "found only {checked} yaml examples");
    }
}
