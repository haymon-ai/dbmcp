//! Reads every source, checks every setting value, and picks each winner.
//!
//! [`read`] is the loader's one entry point. It picks the env files
//! (`--env-file` list > process `DBMCP_ENV_FILE` list, see [`pick`]) and
//! reads them into the env layers (see [`layers`]), then picks the config
//! files (`--config-file` list > `DBMCP_CONFIG_FILE` list in the env layers'
//! setting view) and reads them (see [`files`]). Each reader stops at the first broken file.
//!
//! [`check`] then takes the config files' structure errors from the config
//! schema and checks every set value with its [`Kind`]: each leaf of every
//! config file, each non-empty entry of every env file, and each non-empty
//! process env var, even values that lose to a higher source, are
//! overwritten by a later file, or sit in a section that is not active.
//! Path values come out resolved: YAML paths against their own config
//! file's directory. Every failure is an [`InputError`]; when there are
//! none, [`Settings`] holds each setting's winning value and its
//! [`Source`]: the env layers' setting view (see [`EnvLayers::candidates`])
//! > the last config file setting it.
//!
//! [`Kind`]: crate::env::Kind

mod files;
mod layers;
mod pick;

use std::collections::HashMap;
use std::path::Path;

use crate::env::{CONFIG_FILE_VAR, ENV_FILE_VAR, Raw, SETTINGS, Setting, Value};
use crate::loader::LoadErrorKind;
use crate::options::LoadOptions;
use crate::source::Source;
use files::ConfigFiles;
use layers::EnvLayers;
use pick::FileKind;

/// One invalid input: a malformed config file or a value that fails its setting's checks.
///
/// Renders value-free as `<source>: <path>: <reason>`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
    /// Files rank by their position in `files` or `layers`.
    fn order(&self, files: &ConfigFiles, layers: &EnvLayers<'_>) -> (u8, usize, u64, u64, &str) {
        match &self.from {
            Source::File { path, line, column } => (0, files.position(path).unwrap_or(0), *line, *column, &self.path),
            Source::EnvFile { path, line, .. } => (1, layers.position(path).unwrap_or(0), *line as u64, 0, &self.path),
            Source::Env(name) => (2, 0, 0, 0, name),
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

/// Reads every source named by `options` and returns each setting's winner.
///
/// Warnings of the files read go to `warnings`, even on failure.
///
/// # Errors
///
/// Returns the first file list, read, parse, or interpolation failure,
/// else [`LoadErrorKind::Settings`] with every [`InputError`].
pub(crate) fn read(options: &LoadOptions, warnings: &mut Vec<String>) -> Result<Settings, LoadErrorKind> {
    let env_list = layers::non_empty(&options.environment, ENV_FILE_VAR).map(|list| (list, ENV_FILE_VAR.to_owned()));
    let home = options.home.as_deref();
    let env_files = pick::files(FileKind::Env, &options.env_files, env_list, &options.working_dir, home)?;
    let layers = EnvLayers::read(&options.environment, env_files, warnings)?;
    let config_list = layers
        .candidates(CONFIG_FILE_VAR)
        .find(|(_, _, eligible)| *eligible)
        .map(|(list, source, _)| (list, source.to_string()));
    let config_files = pick::files(
        FileKind::Config,
        &options.config_files,
        config_list,
        &options.working_dir,
        home,
    )?;
    let files = ConfigFiles::read(config_files, &|name| layers.var(name), warnings)?;
    check(&files, &layers, home).map_err(LoadErrorKind::Settings)
}

/// Checks every source's values and returns each setting's winner.
///
/// `home` expands `~` in paths.
///
/// # Errors
///
/// Returns the config files' structure errors and every value that fails its setting's checks,
/// from any source, sorted by [`InputError::order`].
fn check(files: &ConfigFiles, layers: &EnvLayers<'_>, home: Option<&Path>) -> Result<Settings, Vec<InputError>> {
    let mut errors = files.structure().to_vec();
    let mut settings = Settings::default();
    for &setting in SETTINGS {
        let Setting { path, env, kind } = setting;
        // Each candidate is checked; only an eligible one may win.
        let candidates = layers
            .candidates(env)
            .map(|(text, source, eligible)| (Raw::Text(text), source, eligible))
            // A later config file wins where it sets a value; unset ones let earlier files through.
            .chain(files.candidates(path).map(|(raw, source)| (raw, source, true)));
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
    errors.sort_by(|a, b| a.order(files, layers).cmp(&b.order(files, layers)));
    Err(errors)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use tempfile::TempDir;

    use super::*;
    use crate::env;

    /// Inputs for [`check`]; the files are written to a scratch directory.
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

        /// Returns the config file's absolute path.
        fn yaml_path(&self) -> String {
            self.dir.path().join("dbmcp.yaml").display().to_string()
        }

        /// Returns a config file source at `line` and `column`.
        fn file(&self, line: u64, column: u64) -> Source {
            Source::File {
                path: self.dir.path().join("dbmcp.yaml"),
                line,
                column,
            }
        }

        fn check(&self) -> Result<Settings, Vec<InputError>> {
            let mut options = LoadOptions {
                environment: self.environment.clone(),
                working_dir: self.dir.path().to_path_buf(),
                ..LoadOptions::default()
            };
            if let Some(text) = self.env_file {
                fs::write(self.env_path(), text).expect("write env file");
                options.env_files.push(".env".into());
            }
            if let Some(yaml) = self.yaml {
                fs::write(self.yaml_path(), yaml).expect("write config file");
                options.config_files.push("dbmcp.yaml".into());
            }
            match read(&options, &mut Vec::new()) {
                Ok(settings) => Ok(settings),
                Err(LoadErrorKind::Settings(errors)) => Err(errors),
                Err(error) => panic!("readable inputs: {error}"),
            }
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

    #[test]
    fn winner_is_process_env_then_env_file_then_yaml() {
        let inputs = Inputs::default()
            .yaml("database:\n  host: yaml\n  user: yaml\n  port: 1111\n")
            .env_file("DBMCP_DB_HOST=file\nDBMCP_DB_USER=file\nDBMCP_DB_PORT=\n")
            .env("DBMCP_DB_HOST", "process")
            .env("DBMCP_DB_USER", "");
        let settings = inputs.check().expect("valid");
        assert_eq!(settings.get(env::DB_HOST), Some(&Value::Text("process".into())));
        assert_eq!(settings.source(env::DB_HOST), Some(&Source::Env("DBMCP_DB_HOST")));
        assert_eq!(settings.get(env::DB_USER), Some(&Value::Text("file".into())));
        assert!(matches!(
            settings.source(env::DB_USER),
            Some(Source::EnvFile { line: 2, .. })
        ));
        assert_eq!(settings.get(env::DB_PORT), Some(&Value::Int(1111)));
        assert_eq!(settings.source(env::DB_PORT), Some(&inputs.file(4, 9)));
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
        let yaml = inputs.yaml_path();
        assert_eq!(
            inputs.errors(),
            [
                format!("{yaml}:2:9: database.port: expected non-negative integer, found string"),
                format!("DBMCP_DB_PORT in {env_file}:1: database.port: must be at most 65535"),
            ]
        );
    }

    #[test]
    fn values_in_inactive_sections_are_checked() {
        let inputs = Inputs::default()
            .yaml("http:\n  port: nope\n")
            .env("DBMCP_SSH_PORT", "0");
        let yaml = inputs.yaml_path();
        assert_eq!(
            inputs.errors(),
            [
                format!("{yaml}:2:9: http.port: expected non-negative integer, found string"),
                "DBMCP_SSH_PORT: ssh.port: must be at least 1".to_owned(),
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
        let yaml = inputs.yaml_path();
        assert_eq!(
            inputs.errors(),
            [
                format!("{yaml}:2:9: ssh.port: must be at least 1"),
                format!(
                    "{yaml}:4:3: database.pasword: unknown field; expected one of backend, host, port, user, \
                     password, name, charset, ssl, ssl_ca, ssl_cert, ssl_key, ssl_verify_cert, read_only, \
                     max_pool_size, connection_timeout, query_timeout, page_size"
                ),
                format!("{yaml}:5:12: database.backend: expected one of mysql, mariadb, postgres, sqlite"),
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
        let inputs = Inputs::default().yaml("pii: []\n");
        let yaml = inputs.yaml_path();
        assert_eq!(
            inputs.errors(),
            [format!("{yaml}:1:6: pii: expected mapping, found sequence")]
        );
    }

    #[test]
    fn error_source_is_where_the_value_is() {
        let inputs = Inputs::default().yaml("database:\n  ssl: 1\n");
        let error = inputs.check().expect_err("invalid");
        assert_eq!(
            error,
            [InputError {
                from: inputs.file(2, 8),
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
        let dir = tempfile::tempdir().expect("tempdir");
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
                let at = format!("{}:{}", path.display(), start + 1);
                fs::write(dir.path().join("example.yaml"), &yaml).expect("write example");
                let options = LoadOptions {
                    config_files: vec!["example.yaml".into()],
                    environment: required_vars(&yaml)
                        .into_iter()
                        .map(|name| (name, "placeholder".to_owned()))
                        .collect(),
                    working_dir: dir.path().to_path_buf(),
                    ..LoadOptions::default()
                };
                if let Err(error) = read(&options, &mut Vec::new()) {
                    panic!("{at}: {error}");
                }
                checked += 1;
            }
        }
        assert!(checked >= 4, "found only {checked} yaml examples");
    }
}
