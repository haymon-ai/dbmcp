//! Loading the YAML config file and layering it under the CLI.
//!
//! [`Sources::read`] reads the optional config file and the dotenv files;
//! the caller then folds the dotenv variables into its CLI layer and calls
//! [`Sources::resolve`], which stacks that layer over the config file and
//! resolves the result into a validated [`Config`](crate::Config). Each
//! stage fails fast with a [`LoadError`]; non-fatal problems are collected
//! as [`ConfigWarning`]s.
//!
//! Without `--env` files, a `.env` beside the config file is loaded when it
//! exists. Without a config file there is no default `.env`.
//!
//! Paths written in the file expand a leading `~` and then resolve
//! relative to the file's directory, so the file means the same thing
//! whatever the current directory is. CLI and env paths are left alone.

use std::path::{Path, PathBuf};

use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::dotenv::Dotenv;
use crate::error::ConfigErrors;
use crate::interpolate::Lookup;
use crate::layer::{ConfigLayer, Resolved, Transport};
use crate::provenance::{Provenance, Source};
use crate::ssh;
use crate::warning::ConfigWarning;
use crate::yaml;

/// Failure of one [`load`] stage.
///
/// `Display` renders a header line followed by one indented bullet per
/// problem.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// The config file could not be read.
    #[error("failed to read config file {}: {reason}", path.display())]
    Read {
        /// Absolute path of the config file.
        path: PathBuf,
        /// Why reading failed.
        reason: String,
    },

    /// The config file is not valid YAML or does not match the schema.
    #[error("failed to parse config file {}:\n  - {message}", path.display())]
    Parse {
        /// Absolute path of the config file.
        path: PathBuf,
        /// Parser message.
        message: String,
    },

    /// A dotenv file could not be read or has malformed entries.
    #[error("failed to load env file {}:{}", path.display(), Bullets(errors))]
    Env {
        /// Absolute path of the env file.
        path: PathBuf,
        /// One message per problem, e.g. `/abs/.env:3: reason`.
        errors: Vec<String>,
    },

    /// A substitution in the config file failed.
    #[error("failed to interpolate config file {}:{}", path.display(), Bullets(errors))]
    Interpolate {
        /// Absolute path of the config file.
        path: PathBuf,
        /// One message per failure, sorted by config path.
        errors: Vec<String>,
    },

    /// The merged configuration violates one or more rules.
    ///
    /// Each error is rendered with its note: where the involved values came
    /// from, or how to set a value nobody set.
    #[error("configuration validation failed:{}", Annotated(errors, notes))]
    Invalid {
        /// The rule violations.
        errors: ConfigErrors,
        /// One note per error (may be empty).
        notes: Vec<String>,
    },
}

impl From<ConfigErrors> for LoadError {
    fn from(errors: ConfigErrors) -> Self {
        let notes = vec![String::new(); errors.len()];
        Self::Invalid { errors, notes }
    }
}

/// Renders each error followed by its note, as [`Bullets`].
struct Annotated<'a>(&'a ConfigErrors, &'a [String]);

impl std::fmt::Display for Annotated<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lines: Vec<String> = self
            .0
            .iter()
            .zip(self.1)
            .map(|(error, note)| format!("{error}{note}"))
            .collect();
        Bullets(&lines).fmt(f)
    }
}

struct Bullets<'a, T>(&'a [T]);

impl<T: std::fmt::Display> std::fmt::Display for Bullets<'_, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for error in self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

/// Raw contents of the config file.
#[derive(Debug)]
struct ConfigFile {
    /// Absolute path.
    path: PathBuf,
    /// File contents.
    text: String,
}

/// Config file, dotenv variables, and process env read by [`Sources::read`].
pub struct Sources<'a> {
    file: Option<ConfigFile>,
    dotenv: Dotenv,
    process_env: Lookup<'a>,
}

impl std::fmt::Debug for Sources<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sources")
            .field("file", &self.file.as_ref().map(|file| &file.path))
            .field("dotenv", &self.dotenv)
            .finish_non_exhaustive()
    }
}

impl<'a> Sources<'a> {
    /// Reads the optional config file and the dotenv files.
    ///
    /// `env_files` are loaded in order, later files overriding earlier ones;
    /// when empty, the `.env` beside `config` is loaded if it exists.
    /// Relative paths resolve against the current directory. Dotenv values
    /// interpolate against `process_env` first, then earlier keys; config
    /// file values later interpolate against `process_env`, then the dotenv
    /// variables.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::Read`] when `config` is missing, not a regular
    /// file, or unreadable, and [`LoadError::Env`] when an explicit env file
    /// cannot be read or any loaded env file has malformed entries.
    pub fn read(
        config: Option<&Path>,
        env_files: &[PathBuf],
        process_env: Lookup<'a>,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<Self, LoadError> {
        let file = config
            .map(|path| {
                read_text(path)
                    .map(|(path, text)| ConfigFile { path, text })
                    .map_err(|e| LoadError::Read {
                        path: e.path,
                        reason: e.reason,
                    })
            })
            .transpose()?;

        let mut dotenv = Dotenv::default();
        for path in env_files {
            let (path, text) = read_text(path).map_err(|e| LoadError::Env {
                path: e.path,
                errors: vec![e.reason],
            })?;
            parse_dotenv(&mut dotenv, &text, path, process_env, warnings)?;
        }
        if env_files.is_empty()
            && let Some(file) = &file
        {
            match read_text(&file.path.with_file_name(".env")) {
                Ok((path, text)) => parse_dotenv(&mut dotenv, &text, path, process_env, warnings)?,
                Err(e) if e.missing => {}
                Err(e) => warnings.push(ConfigWarning::SkippedEnvFile {
                    path: e.path,
                    reason: e.reason,
                }),
            }
        }
        Ok(Self {
            file,
            dotenv,
            process_env,
        })
    }

    /// Returns the variables of every loaded dotenv file.
    #[must_use]
    pub fn dotenv(&self) -> &Dotenv {
        &self.dotenv
    }

    /// Merges `cli` over the config file and resolves the result.
    ///
    /// The file is parsed, its string values are interpolated (unset
    /// variables become `""` and add a warning), and the result is
    /// deserialized; a string in a bool or integer field is parsed like the
    /// matching env var. `provenance` holds the sources of the values in
    /// `cli`; the file's value locations are added below them, and every
    /// validation error is annotated from the result. Path settings read
    /// from the file resolve against the file's
    /// directory, after expanding a leading `~` against `home`;
    /// `database.name` does so only when it came from the file and the
    /// merged backend is `sqlite`. `home` is also forwarded to
    /// [`ConfigLayer::resolve`].
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::Parse`] when the config file is not a valid
    /// config document, [`LoadError::Interpolate`] when a substitution
    /// fails, and [`LoadError::Invalid`] when the merged configuration fails
    /// validation.
    pub fn resolve(
        &self,
        cli: ConfigLayer,
        mut provenance: Provenance,
        transport: Transport,
        home: Option<&Path>,
        warnings: &mut Vec<ConfigWarning>,
    ) -> Result<Resolved, LoadError> {
        let annotate = |provenance: &Provenance, errors: ConfigErrors| {
            let notes = errors.iter().map(|error| provenance.note(error)).collect();
            LoadError::Invalid { errors, notes }
        };
        let Some(file) = &self.file else {
            return cli.resolve(transport, home).map_err(|e| annotate(&provenance, e));
        };
        let lookup = |name: &str| self.dotenv.lookup(self.process_env, name);
        let (mut file_layer, locations) = parse_layer(&file.path, &file.text, &lookup, warnings)?;
        for (path, location) in locations {
            provenance.set_source(&path, Source::File(location));
        }
        // The file's `database.name` is a path only when it wins the merge
        // and the merged backend is sqlite.
        let backend = cli
            .database
            .backend
            .or(file_layer.database.backend)
            .unwrap_or(DatabaseConfig::DEFAULT_BACKEND);
        let name_is_path = cli.database.name.is_none() && backend == DatabaseBackend::Sqlite;
        let dir = file.path.parent().unwrap_or(Path::new("/"));
        resolve_file_paths(&mut file_layer, dir, home, name_is_path);
        cli.or(file_layer)
            .resolve(transport, home)
            .map_err(|e| annotate(&provenance, e))
    }
}

/// Parses one dotenv file into `dotenv`, failing on any malformed entry.
fn parse_dotenv(
    dotenv: &mut Dotenv,
    text: &str,
    path: PathBuf,
    process_env: Lookup<'_>,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<(), LoadError> {
    let errors = dotenv.parse(text, &path, process_env, warnings);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(LoadError::Env { path, errors })
    }
}

/// Resolves every path setting of a file layer against `dir`.
///
/// `database.name` is included when `name_is_path` is set, unless it is
/// `:memory:` or empty.
fn resolve_file_paths(layer: &mut ConfigLayer, dir: &Path, home: Option<&Path>, name_is_path: bool) {
    let db = &mut layer.database;
    if name_is_path
        && let Some(name) = &mut db.name
        && !is_sqlite_special_name(name)
    {
        *name = resolve_path_string(name, dir, home);
    }
    for path in [&mut db.ssl_ca, &mut db.ssl_cert, &mut db.ssl_key]
        .into_iter()
        .flatten()
    {
        *path = resolve_path_string(path, dir, home);
    }
    let ssh = &mut layer.ssh;
    for path in [&mut ssh.key, &mut ssh.known_hosts, &mut layer.pii.ner_model]
        .into_iter()
        .flatten()
    {
        *path = resolve_path(path, dir, home);
    }
}

/// Expands a leading `~` in `path`, then anchors a relative result at `dir`.
///
/// An empty path (often an unset `${VAR}`) stays empty, so validation
/// reports it instead of silently pointing at `dir`.
fn resolve_path(path: &Path, dir: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = ssh::expand_tilde(path, home);
    if expanded.as_os_str().is_empty() || expanded.is_absolute() {
        expanded
    } else {
        dir.join(expanded)
    }
}

/// [`resolve_path`] for settings stored as strings.
fn resolve_path_string(path: &str, dir: &Path, home: Option<&Path>) -> String {
    resolve_path(Path::new(path), dir, home).to_string_lossy().into_owned()
}

/// Returns `true` for `SQLite` names that are not file paths.
///
/// The backend opens `name` as a literal filename, so only `:memory:` (and
/// the empty name validation rejects) are left untouched; a `file:` prefix
/// is an ordinary relative path.
fn is_sqlite_special_name(name: &str) -> bool {
    name.is_empty() || name == ":memory:"
}

/// Why [`read_text`] failed.
struct ReadError {
    /// Absolute path (or the given one, when it cannot be made absolute).
    path: PathBuf,
    /// Why reading failed.
    reason: String,
    /// The file does not exist.
    missing: bool,
}

/// Reads the regular file at `path`, returning its absolute path and contents.
fn read_text(path: &Path) -> Result<(PathBuf, String), ReadError> {
    let fail = |path: PathBuf, reason: String, missing: bool| ReadError { path, reason, missing };
    let path = std::path::absolute(path).map_err(|e| fail(path.to_path_buf(), e.to_string(), false))?;
    let metadata = match std::fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(e) => {
            let missing = e.kind() == std::io::ErrorKind::NotFound;
            return Err(fail(path, e.to_string(), missing));
        }
    };
    if metadata.is_dir() {
        return Err(fail(path, "is a directory".into(), false));
    }
    if !metadata.is_file() {
        return Err(fail(path, "not a regular file".into(), false));
    }
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok((path, text)),
        Err(e) => Err(fail(path, e.to_string(), false)),
    }
}

/// Parses, interpolates, and deserializes the config file at `path`.
///
/// Returns the layer and the `path:line:column` of every value set in it,
/// keyed by config path. An empty or `null` document is an empty layer.
fn parse_layer(
    path: &Path,
    text: &str,
    lookup: Lookup<'_>,
    warnings: &mut Vec<ConfigWarning>,
) -> Result<(ConfigLayer, Vec<(String, String)>), LoadError> {
    let parse_error = |message: String| LoadError::Parse {
        path: path.to_path_buf(),
        message,
    };
    let at = |location: &serde_saphyr::Location| format!("{}:{}", path.display(), yaml::line_col(location));

    let mut root = yaml::parse(text).map_err(|(location, message)| {
        parse_error(match location {
            Some(location) => format!("{}: {message}", at(&location)),
            None => message,
        })
    })?;
    let found = yaml::interpolate(&mut root, lookup);
    warnings.extend(found.unset.into_iter().map(|reference| ConfigWarning::UnsetVariable {
        name: reference.name,
        location: at(&reference.location),
    }));
    if !found.errors.is_empty() {
        return Err(LoadError::Interpolate {
            path: path.to_path_buf(),
            errors: found
                .errors
                .into_iter()
                .map(|e| format!("{}: {}: {}", at(&e.location), e.path, e.error))
                .collect(),
        });
    }
    let layer = yaml::from_node(&root).map_err(|e| {
        parse_error(match (e.path.filter(|p| !p.is_empty()), e.location) {
            (Some(config_path), Some(location)) => format!("{}: {config_path}: {}", at(&location), e.message),
            (None, Some(location)) => format!("{}: {}", at(&location), e.message),
            (_, None) => e.message,
        })
    })?;
    let locations = found
        .locations
        .into_iter()
        .map(|(config_path, location)| (config_path, at(&location)))
        .collect();
    Ok((layer, locations))
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::test_support::home;
    use crate::{ConfigError, LogLevel, PiiCategory, StrictHostKeyChecking};

    fn no_env(_: &str) -> Option<String> {
        None
    }

    /// Reads `file` without env files and resolves `cli` over it.
    fn load(
        cli: ConfigLayer,
        file: Option<&Path>,
        transport: Transport,
        home: Option<&Path>,
    ) -> Result<Resolved, LoadError> {
        Sources::read(file, &[], &no_env, &mut Vec::new())?.resolve(
            cli,
            Provenance::default(),
            transport,
            home,
            &mut Vec::new(),
        )
    }

    /// Parses a config document without any variables set.
    fn parse(text: &str) -> Result<ConfigLayer, String> {
        parse_layer(Path::new("/cfg/dbmcp.yaml"), text, &no_env, &mut Vec::new())
            .map(|(layer, _)| layer)
            .map_err(|e| e.to_string())
    }

    fn parse_ok(text: &str) -> ConfigLayer {
        parse(text).unwrap_or_else(|e| panic!("{text:?} must parse: {e}"))
    }

    fn write_file(text: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(text.as_bytes()).expect("write config");
        file
    }

    #[test]
    fn empty_comment_only_and_null_documents_are_empty_layers() {
        for text in ["", "\n", "# just a comment\n", "null", "~", "---\n"] {
            let layer = parse_ok(text);
            assert_eq!(
                format!("{layer:?}"),
                format!("{:?}", ConfigLayer::default()),
                "{text:?}"
            );
        }
    }

    #[test]
    fn null_values_and_bare_sections_are_absent() {
        let layer = parse_ok("log_level:\ndatabase:\n  host: ~\n  port: null\n  user:\nssh:\nhttp: null\n");
        assert!(layer.log_level.is_none());
        assert!(layer.database.host.is_none());
        assert!(layer.database.port.is_none());
        assert!(layer.database.user.is_none());
        assert!(layer.ssh.host.is_none());
    }

    #[test]
    fn full_schema_parses() {
        let layer = parse_ok(
            "log_level: debug
database:
  backend: postgres
  host: db.internal
  port: 5432
  user: app
  password: hunter2
  name: app
  charset: utf8mb4
  ssl: true
  ssl_ca: /ca.pem
  ssl_cert: /cert.pem
  ssl_key: /key.pem
  ssl_verify_cert: false
  read_only: false
  max_pool_size: 10
  connection_timeout: 5
  query_timeout: 0
  page_size: 50
http:
  host: 0.0.0.0
  port: 9002
  allowed_origins: [http://localhost:*]
  allowed_hosts:
    - localhost
  auth_token: token
  allow_unauthenticated: false
pii:
  enabled: true
  operator: mask
  categories: [financial, digital-identity]
  ner_enabled: false
  ner_model: /models/ner
ssh:
  host: bastion
  port: 2222
  user: deploy
  password: pw
  key: /id
  key_passphrase: pp
  known_hosts: /kh
  strict_host_key_checking: accept-new
  keepalive_interval: 15
  local_port: 4000
",
        );
        assert_eq!(layer.log_level, Some(LogLevel::Debug));
        assert_eq!(layer.database.backend, Some(DatabaseBackend::Postgres));
        assert_eq!(layer.database.port, Some(5432));
        assert_eq!(
            layer.database.password.as_ref().map(crate::Secret::expose),
            Some("hunter2")
        );
        assert_eq!(layer.database.query_timeout, Some(0));
        assert_eq!(layer.http.allowed_origins, Some(vec!["http://localhost:*".to_owned()]));
        assert_eq!(layer.http.allowed_hosts, Some(vec!["localhost".to_owned()]));
        assert_eq!(
            layer.pii.categories,
            Some(vec![PiiCategory::Financial, PiiCategory::DigitalIdentity])
        );
        assert_eq!(
            layer.ssh.strict_host_key_checking,
            Some(StrictHostKeyChecking::AcceptNew)
        );
        assert_eq!(layer.ssh.keepalive_interval, Some(15));
    }

    #[test]
    fn unquoted_yes_and_no_are_strings() {
        for (raw, expected) in [("yes", StrictHostKeyChecking::Yes), ("no", StrictHostKeyChecking::No)] {
            let layer = parse_ok(&format!("ssh:\n  strict_host_key_checking: {raw}\n"));
            assert_eq!(layer.ssh.strict_host_key_checking, Some(expected), "{raw}");
        }
    }

    #[test]
    fn unknown_keys_are_rejected_at_every_level() {
        for text in [
            "databse: {}\n",
            "database:\n  pasword: x\n",
            "http:\n  hots: x\n",
            "pii:\n  enable: true\n",
            "ssh:\n  hostname: x\n",
            "version: 1\n",
        ] {
            let message = parse(text).expect_err(text);
            assert!(message.contains("unknown field"), "{text:?}: {message}");
        }
    }

    #[test]
    fn enum_values_are_case_sensitive_kebab_tokens() {
        parse_ok("database:\n  backend: mariadb\n");
        let message = parse("database:\n  backend: Postgres\n").expect_err("case-sensitive");
        assert!(
            message.contains("expected one of `mysql`, `mariadb`, `postgres`, `sqlite`"),
            "{message}"
        );
        parse("pii:\n  categories: [digital_identity]\n").expect_err("kebab only");
        parse("log_level: INFO\n").expect_err("case-sensitive log level");
    }

    #[test]
    fn lists_must_be_sequences() {
        parse("http:\n  allowed_hosts: localhost\n").expect_err("scalar list");
        parse("pii:\n  categories: financial,contact\n").expect_err("comma list");
        assert_eq!(
            parse_ok("http:\n  allowed_origins: []\n").http.allowed_origins,
            Some(vec![])
        );
    }

    #[test]
    fn bool_fields_parse_strings_like_the_env_layer() {
        assert_eq!(
            parse_ok("database:\n  read_only: false\n").database.read_only,
            Some(false)
        );
        for (raw, expected) in [
            ("yes", true),
            ("'on'", true),
            ("\"1\"", true),
            ("no", false),
            ("OFF", false),
        ] {
            let layer = parse_ok(&format!("database:\n  read_only: {raw}\n"));
            assert_eq!(layer.database.read_only, Some(expected), "{raw}");
        }
        for raw in ["''", "disabled", "2"] {
            parse(&format!("database:\n  read_only: {raw}\n")).expect_err(raw);
        }
    }

    #[test]
    fn integer_fields_parse_strings_and_strings_reject_numbers() {
        assert_eq!(parse_ok("database:\n  port: \"5432\"\n").database.port, Some(5432));
        parse("database:\n  port: '54x'\n").expect_err("not a number");
        parse("database:\n  port: 70000\n").expect_err("out of range");
        let message = parse("database:\n  user: 123\n").expect_err("number in string field");
        assert_eq!(
            message,
            "failed to parse config file /cfg/dbmcp.yaml:\n  - /cfg/dbmcp.yaml:2:9: database.user: expected a string, found integer"
        );
    }

    #[test]
    fn type_errors_never_echo_secret_values() {
        for yaml in [
            "database:\n  password: 31337\n",
            "ssh:\n  key_passphrase: [31337]\n",
            "http:\n  auth_token: {a: 31337}\n",
        ] {
            let message = parse(yaml).expect_err(yaml);
            assert!(!message.contains("31337"), "{message}");
        }
    }

    #[test]
    fn unknown_key_names_location_and_path_without_suggestion() {
        let message = parse("database:\n  pasword: x\n").expect_err("unknown key");
        assert!(
            message.starts_with(
                "failed to parse config file /cfg/dbmcp.yaml:\n  - /cfg/dbmcp.yaml:2:3: database.pasword: unknown field; expected one of `backend`, `host`,"
            ),
            "{message}"
        );
    }

    #[test]
    fn enum_errors_point_at_the_value_not_its_section() {
        let message = parse("database:\n  host: x\n  backend: Postgres\n").expect_err("bad enum");
        assert!(
            message.contains("/cfg/dbmcp.yaml:3:12: database.backend: expected one of `mysql`"),
            "{message}"
        );
        let message = parse("pii:\n  categories: [financial, bogus]\n").expect_err("bad element");
        assert!(message.contains(": pii.categories[1]: expected one of"), "{message}");
    }

    #[test]
    fn yaml_syntax_errors_use_file_line_column_prefix() {
        let message = parse("database: [unclosed\n").expect_err("bad yaml");
        assert!(
            message.starts_with("failed to parse config file /cfg/dbmcp.yaml:\n  - /cfg/dbmcp.yaml:"),
            "{message}"
        );
        assert!(!message.contains(" at line "), "{message}");
    }

    #[test]
    fn out_of_range_values_are_not_echoed() {
        let message = parse("database:\n  port: 70000\n").expect_err("range");
        assert!(
            message.ends_with("database.port: invalid value; expected u16"),
            "{message}"
        );
        assert!(!message.contains("70000"));
    }

    #[test]
    fn validation_errors_name_file_sources() {
        let file = write_file("database:\n  page_size: 0\n  backend: sqlite\n");
        let error = load(ConfigLayer::default(), Some(file.path()), Transport::Stdio, None).expect_err("invalid");
        let at = file.path().display();
        assert_eq!(
            error.to_string(),
            format!(
                "configuration validation failed:\n  \
                 - database.name: required (file path) for the sqlite backend; set database.name in the config file\n  \
                 - database.page_size: must be between 1 and 500 (from {at}:2:14)"
            )
        );
    }

    #[test]
    fn higher_layer_source_wins_over_file_location() {
        let file = write_file("database:\n  page_size: 0\n");
        let mut provenance = Provenance::default();
        provenance.set_source("database.page_size", Source::Env("DB_PAGE_SIZE".into()));
        let mut cli = ConfigLayer::default();
        cli.database.page_size = Some(501);
        let error = Sources::read(Some(file.path()), &[], &no_env, &mut Vec::new())
            .expect("read")
            .resolve(cli, provenance, Transport::Stdio, None, &mut Vec::new())
            .expect_err("invalid");
        assert!(error.to_string().ends_with("(from DB_PAGE_SIZE)"), "{error}");
    }

    #[test]
    fn invalid_secret_never_appears_in_validation_output() {
        let file = write_file("http:\n  auth_token: '   '\n  host: 0.0.0.0\n");
        let error = load(ConfigLayer::default(), Some(file.path()), Transport::Http, None).expect_err("blank token");
        let rendered = error.to_string();
        assert!(
            rendered.contains("http.auth_token: must not be empty when set (from "),
            "{rendered}"
        );
        assert!(!rendered.contains("'   '"), "{rendered}");
    }

    #[test]
    fn anchors_resolve_before_deserializing() {
        let layer = parse_ok("database:\n  host: &host db.internal\nhttp:\n  host: *host\n");
        assert_eq!(layer.http.host.as_deref(), Some("db.internal"));
        parse("x-common: &c 1\n").expect_err("no extension keys");
    }

    /// Resolves `yaml` with `vars` as the process env, returning the layer and warnings.
    fn interpolate_with(yaml: &str, vars: &[(&str, &str)]) -> (Result<ConfigLayer, LoadError>, Vec<ConfigWarning>) {
        let lookup = |name: &str| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        };
        let mut warnings = Vec::new();
        let layer = parse_layer(Path::new("/cfg/dbmcp.yaml"), yaml, &lookup, &mut warnings).map(|(layer, _)| layer);
        (layer, warnings)
    }

    #[test]
    fn required_variable_fails_when_unset_and_works_when_set() {
        let yaml = "database:\n  password: ${DB_PASSWORD:?}\n";
        let (layer, _) = interpolate_with(yaml, &[("DB_PASSWORD", "hunter2")]);
        let layer = layer.expect("set");
        assert_eq!(
            layer.database.password.as_ref().map(crate::Secret::expose),
            Some("hunter2")
        );

        let (error, _) = interpolate_with(yaml, &[]);
        assert_eq!(
            error.expect_err("unset").to_string(),
            "failed to interpolate config file /cfg/dbmcp.yaml:\n  - /cfg/dbmcp.yaml:2:13: database.password: required variable DB_PASSWORD is not set"
        );
    }

    #[test]
    fn unset_variable_cannot_flip_a_security_bool() {
        let (error, warnings) = interpolate_with("database:\n  read_only: ${RO}\n", &[]);
        let error = error.expect_err("empty bool must fail");
        assert!(matches!(error, LoadError::Parse { .. }), "{error}");
        assert_eq!(
            warnings,
            [ConfigWarning::UnsetVariable {
                name: "RO".into(),
                location: "/cfg/dbmcp.yaml:2:14".into()
            }]
        );
    }

    #[test]
    fn unset_variable_in_string_field_is_empty_not_null() {
        let (layer, warnings) = interpolate_with("database:\n  user: ${NOPE}\n", &[]);
        assert_eq!(layer.expect("valid").database.user.as_deref(), Some(""));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn interpolated_numbers_deserialize_and_lists_interpolate_per_element() {
        let (layer, _) = interpolate_with(
            "database:\n  port: ${PORT}\nhttp:\n  allowed_hosts: [localhost, \"${EXTRA}\"]\n",
            &[("PORT", "5433"), ("EXTRA", "db.example")],
        );
        let layer = layer.expect("valid");
        assert_eq!(layer.database.port, Some(5433));
        assert_eq!(
            layer.http.allowed_hosts,
            Some(vec!["localhost".into(), "db.example".into()])
        );
    }

    #[test]
    fn dollar_escape_and_keys_are_not_interpolated() {
        let (layer, _) = interpolate_with("database:\n  password: pa$$word\n", &[]);
        assert_eq!(
            layer
                .expect("valid")
                .database
                .password
                .as_ref()
                .map(crate::Secret::expose),
            Some("pa$word")
        );
        let (error, _) = interpolate_with("database:\n  $KEY: x\n", &[("KEY", "host")]);
        assert!(
            error
                .expect_err("keys stay literal")
                .to_string()
                .contains("database.$KEY: unknown field;")
        );
    }

    #[test]
    fn interpolation_errors_are_sorted_by_path_and_never_echo_values() {
        let (error, _) = interpolate_with(
            "ssh:\n  password: hunter2$\ndatabase:\n  user: ${A:?}\n  host: ${B\n",
            &[],
        );
        let rendered = error.expect_err("must fail").to_string();
        assert_eq!(
            rendered,
            "failed to interpolate config file /cfg/dbmcp.yaml:\n  \
             - /cfg/dbmcp.yaml:5:9: database.host: unterminated `${`\n  \
             - /cfg/dbmcp.yaml:4:9: database.user: required variable A is not set\n  \
             - /cfg/dbmcp.yaml:2:13: ssh.password: invalid `$`; escape a literal `$` as `$$`"
        );
        assert!(!rendered.contains("hunter2"));
    }

    #[test]
    fn file_paths_resolve_after_substitution() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dbmcp.yaml");
        std::fs::write(&path, "database:\n  ssl_ca: ${CERT_DIR}/ca.pem\n").expect("write config");
        let env = |name: &str| (name == "CERT_DIR").then(|| "certs".to_owned());
        let sources = Sources::read(Some(&path), &[], &env, &mut Vec::new()).expect("read");
        let config = sources
            .resolve(
                ConfigLayer::default(),
                Provenance::default(),
                Transport::Stdio,
                None,
                &mut Vec::new(),
            )
            .expect("valid")
            .config;
        assert_eq!(
            config.database.ssl_ca.as_deref().map(PathBuf::from),
            Some(dir.path().join("certs/ca.pem"))
        );
    }

    #[test]
    fn file_values_interpolate_from_process_env_then_dotenv() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dbmcp.yaml");
        std::fs::write(&path, "database:\n  user: ${WHO}\n  host: ${WHERE}\n").expect("write config");
        std::fs::write(dir.path().join(".env"), "WHO=dotenv\nWHERE=dotenv-host\n").expect("write .env");
        let env = |name: &str| (name == "WHO").then(|| "process".to_owned());
        let sources = Sources::read(Some(&path), &[], &env, &mut Vec::new()).expect("read");
        let db = sources
            .resolve(
                ConfigLayer::default(),
                Provenance::default(),
                Transport::Stdio,
                None,
                &mut Vec::new(),
            )
            .expect("valid")
            .config
            .database;
        assert_eq!(db.user, "process");
        assert_eq!(db.host, "dotenv-host");
    }

    #[test]
    fn load_applies_file_values_under_cli_and_over_defaults() {
        let file = write_file("database:\n  backend: postgres\n  host: file-host\n  user: file-user\n");
        let mut cli = ConfigLayer::default();
        cli.database.host = Some("cli-host".into());
        let resolved = load(cli, Some(file.path()), Transport::Stdio, None).expect("valid");
        let db = resolved.config.database;
        assert_eq!(db.backend, DatabaseBackend::Postgres);
        assert_eq!(db.host, "cli-host", "CLI beats file");
        assert_eq!(db.user, "file-user", "file beats default");
        assert_eq!(db.port, 5432, "default follows merged backend");
        assert_eq!(resolved.log_level, LogLevel::Info);
    }

    #[test]
    fn load_without_file_resolves_cli_layer() {
        let resolved = load(ConfigLayer::default(), None, Transport::Stdio, None).expect("valid");
        assert_eq!(resolved.config.database.backend, DatabaseConfig::DEFAULT_BACKEND);
    }

    #[test]
    fn load_validates_merged_config() {
        let file = write_file("database:\n  backend: sqlite\n");
        let error = load(ConfigLayer::default(), Some(file.path()), Transport::Stdio, None).expect_err("no name");
        assert!(
            matches!(&error, LoadError::Invalid { errors: e, .. } if matches!(e[0], ConfigError::MissingSqliteDbName))
        );
        assert_eq!(
            error.to_string(),
            "configuration validation failed:\n  - database.name: required (file path) for the sqlite backend; set database.name in the config file"
        );
    }

    #[test]
    fn load_rejects_missing_directory_and_unparsable_files() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing.yaml");
        let error = load(ConfigLayer::default(), Some(&missing), Transport::Stdio, None).expect_err("missing");
        assert!(matches!(&error, LoadError::Read { path, .. } if *path == missing));
        assert!(
            error
                .to_string()
                .starts_with(&format!("failed to read config file {}: ", missing.display()))
        );

        let error = load(ConfigLayer::default(), Some(dir.path()), Transport::Stdio, None).expect_err("dir");
        assert!(error.to_string().ends_with(": is a directory"), "{error}");

        let file = write_file("database: [unclosed\n");
        let error = load(ConfigLayer::default(), Some(file.path()), Transport::Stdio, None).expect_err("bad yaml");
        assert!(matches!(error, LoadError::Parse { .. }), "{error}");
        assert!(
            error
                .to_string()
                .starts_with(&format!("failed to parse config file {}:\n  - ", file.path().display()))
        );
    }

    /// Writes `yaml` to `dbmcp.yaml` in a fresh directory.
    fn config_dir(yaml: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("dbmcp.yaml");
        std::fs::write(&path, yaml).expect("write config");
        (dir, path)
    }

    fn load_file(path: &Path, cli: ConfigLayer) -> Resolved {
        load(cli, Some(path), Transport::Stdio, Some(&home())).expect("valid")
    }

    #[test]
    fn relative_file_paths_resolve_against_config_dir() {
        let (dir, path) = config_dir(
            "database:\n  ssl_ca: certs/ca.pem\n  ssl_cert: ./cert.pem\n  ssl_key: ../key.pem\n\
             pii:\n  ner_model: models/ner\n\
             ssh:\n  host: bastion\n  user: deploy\n  password: pw\n  known_hosts: kh\n",
        );
        let config = load_file(&path, ConfigLayer::default()).config;
        let base = dir.path();
        let db = &config.database;
        assert_eq!(db.ssl_ca.as_deref().map(PathBuf::from), Some(base.join("certs/ca.pem")));
        assert_eq!(db.ssl_cert.as_deref().map(PathBuf::from), Some(base.join("./cert.pem")));
        assert_eq!(db.ssl_key.as_deref().map(PathBuf::from), Some(base.join("../key.pem")));
        assert_eq!(config.pii.ner_model, Some(base.join("models/ner")));
        assert_eq!(config.ssh.expect("tunnel").known_hosts, Some(base.join("kh")));
    }

    // A relative path to a temp dir on another drive is impossible on Windows.
    #[cfg(unix)]
    #[test]
    fn relative_config_path_still_anchors_file_paths_at_its_directory() {
        let (dir, path) = config_dir("database:\n  ssl_ca: certs/ca.pem\n");
        std::fs::create_dir(dir.path().join("certs")).expect("certs dir");
        let cwd = std::env::current_dir().expect("cwd");
        let relative = pathdiff(&path, &cwd);
        assert!(relative.is_relative());
        let config = load_file(&relative, ConfigLayer::default()).config;
        let ssl_ca = PathBuf::from(config.database.ssl_ca.expect("ssl_ca"));
        assert!(ssl_ca.is_absolute(), "{}", ssl_ca.display());
        assert_eq!(ssl_ca.file_name(), Some("ca.pem".as_ref()));
        let canonical = |p: &Path| std::fs::canonicalize(p).expect("canonicalize");
        assert_eq!(
            canonical(ssl_ca.parent().expect("parent")),
            canonical(&dir.path().join("certs"))
        );
    }

    /// Returns `path` relative to `base` using `..` components.
    #[cfg(unix)]
    fn pathdiff(path: &Path, base: &Path) -> PathBuf {
        let path: Vec<_> = path.components().collect();
        let base: Vec<_> = base.components().collect();
        let common = path.iter().zip(&base).take_while(|(a, b)| a == b).count();
        let mut relative = PathBuf::new();
        for _ in common..base.len() {
            relative.push("..");
        }
        relative.extend(&path[common..]);
        relative
    }

    #[test]
    fn tilde_expands_in_every_file_path_field() {
        let (_dir, path) = config_dir(
            "database:\n  ssl_ca: ~/ca.pem\n  ssl_cert: ~/cert.pem\n  ssl_key: ~/key.pem\n\
             pii:\n  ner_model: ~/ner\n\
             ssh:\n  host: bastion\n  user: deploy\n  password: pw\n  known_hosts: ~/kh\n",
        );
        let mut layer = parse(&std::fs::read_to_string(&path).expect("read")).expect("parse");
        layer.ssh.key = Some(PathBuf::from("~/id"));
        resolve_file_paths(&mut layer, Path::new("/cfg"), Some(&home()), false);
        let db = &layer.database;
        assert_eq!(db.ssl_ca.as_deref().map(PathBuf::from), Some(home().join("ca.pem")));
        assert_eq!(db.ssl_cert.as_deref().map(PathBuf::from), Some(home().join("cert.pem")));
        assert_eq!(db.ssl_key.as_deref().map(PathBuf::from), Some(home().join("key.pem")));
        assert_eq!(layer.pii.ner_model, Some(home().join("ner")));
        assert_eq!(layer.ssh.key, Some(home().join("id")));
        assert_eq!(layer.ssh.known_hosts, Some(home().join("kh")));
    }

    #[test]
    fn absolute_file_paths_are_untouched() {
        let absolute = home().join("ca.pem");
        let mut layer = ConfigLayer::default();
        layer.database.ssl_ca = Some(absolute.to_string_lossy().into_owned());
        resolve_file_paths(&mut layer, Path::new("/cfg"), Some(&home()), false);
        assert_eq!(layer.database.ssl_ca.as_deref().map(PathBuf::from), Some(absolute));
    }

    #[test]
    fn sqlite_name_from_file_resolves_against_config_dir() {
        let (dir, path) = config_dir("database:\n  backend: sqlite\n  name: data/app.db\n");
        let config = load_file(&path, ConfigLayer::default()).config;
        assert_eq!(
            config.database.name.as_deref().map(PathBuf::from),
            Some(dir.path().join("data/app.db"))
        );
    }

    #[test]
    fn sqlite_name_from_cli_stays_relative_to_cwd() {
        let (_dir, path) = config_dir("database:\n  backend: sqlite\n  name: data/app.db\n");
        let mut cli = ConfigLayer::default();
        cli.database.name = Some("data/app.db".into());
        let config = load_file(&path, cli).config;
        assert_eq!(config.database.name.as_deref(), Some("data/app.db"));
    }

    #[test]
    fn sqlite_name_follows_merged_backend() {
        let (dir, path) = config_dir("database:\n  name: app.db\n");
        let mut cli = ConfigLayer::default();
        cli.database.backend = Some(DatabaseBackend::Sqlite);
        let config = load_file(&path, cli).config;
        assert_eq!(
            config.database.name.as_deref().map(PathBuf::from),
            Some(dir.path().join("app.db"))
        );
    }

    #[test]
    fn cli_paths_are_not_resolved_against_config_dir() {
        let (_dir, path) = config_dir("database:\n  ssl_ca: file-ca.pem\n");
        let mut cli = ConfigLayer::default();
        cli.database.ssl_ca = Some("certs/ca.pem".into());
        let config = load_file(&path, cli).config;
        assert_eq!(config.database.ssl_ca.as_deref(), Some("certs/ca.pem"));
    }

    #[test]
    fn non_sqlite_name_is_a_plain_database_name() {
        let (_dir, path) = config_dir("database:\n  backend: postgres\n  name: app\n");
        let config = load_file(&path, ConfigLayer::default()).config;
        assert_eq!(config.database.name.as_deref(), Some("app"));
    }

    #[test]
    fn sqlite_memory_name_passes_through() {
        let (_dir, path) = config_dir("database:\n  backend: sqlite\n  name: ':memory:'\n");
        let config = load_file(&path, ConfigLayer::default()).config;
        assert_eq!(config.database.name.as_deref(), Some(":memory:"));
    }

    #[test]
    fn sqlite_file_prefixed_name_is_a_relative_path() {
        let (dir, path) = config_dir("database:\n  backend: sqlite\n  name: 'file:app.db'\n");
        let config = load_file(&path, ConfigLayer::default()).config;
        assert_eq!(
            config.database.name.as_deref().map(PathBuf::from),
            Some(dir.path().join("file:app.db"))
        );
    }

    #[test]
    fn empty_file_path_stays_empty_and_fails_validation() {
        let (_dir, path) = config_dir("database:\n  ssl: true\n  ssl_ca: ${NOPE_CA}\n");
        let sources = Sources::read(Some(&path), &[], &no_env, &mut Vec::new()).expect("read");
        let error = sources
            .resolve(
                ConfigLayer::default(),
                Provenance::default(),
                Transport::Stdio,
                None,
                &mut Vec::new(),
            )
            .expect_err("empty ca must fail");
        assert!(
            matches!(&error, LoadError::Invalid { errors, .. }
                if matches!(&errors[0], ConfigError::FileNotFound { field: "database.ssl_ca", path } if path.is_empty())),
            "{error}"
        );
    }

    #[test]
    fn sqlite_name_tilde_expands() {
        let (_dir, path) = config_dir("database:\n  backend: sqlite\n  name: ~/app.db\n");
        let config = load_file(&path, ConfigLayer::default()).config;
        assert_eq!(
            config.database.name.as_deref().map(PathBuf::from),
            Some(home().join("app.db"))
        );
    }

    fn read_sources(
        config: Option<&Path>,
        env_files: &[PathBuf],
    ) -> (Result<Sources<'static>, LoadError>, Vec<ConfigWarning>) {
        let mut warnings = Vec::new();
        let sources = Sources::read(config, env_files, &no_env, &mut warnings);
        (sources, warnings)
    }

    fn dotenv_value(sources: &Sources, key: &str) -> Option<String> {
        sources.dotenv().get(key).map(|var| var.value.clone())
    }

    #[test]
    fn default_env_beside_config_is_loaded() {
        let (dir, path) = config_dir("");
        std::fs::write(dir.path().join(".env"), "DB_PASSWORD=from-dotenv\n").expect("write .env");
        let (sources, warnings) = read_sources(Some(&path), &[]);
        let sources = sources.expect("valid");
        assert_eq!(dotenv_value(&sources, "DB_PASSWORD").as_deref(), Some("from-dotenv"));
        let var = sources.dotenv().get("DB_PASSWORD").expect("var");
        assert_eq!(var.path, dir.path().join(".env"));
        assert_eq!(var.line, 1);
        assert!(warnings.is_empty());
    }

    #[test]
    fn missing_default_env_is_skipped_silently() {
        let (_dir, path) = config_dir("");
        let (sources, warnings) = read_sources(Some(&path), &[]);
        assert!(sources.expect("valid").dotenv().get("DB_PASSWORD").is_none());
        assert!(warnings.is_empty());
    }

    #[test]
    fn unreadable_default_env_warns_and_is_skipped() {
        let (dir, path) = config_dir("");
        std::fs::create_dir(dir.path().join(".env")).expect(".env dir");
        let (sources, warnings) = read_sources(Some(&path), &[]);
        sources.expect("unreadable default .env is not fatal");
        assert_eq!(
            warnings,
            [ConfigWarning::SkippedEnvFile {
                path: dir.path().join(".env"),
                reason: "is a directory".into()
            }]
        );
    }

    #[test]
    fn explicit_env_files_override_in_order_and_disable_default() {
        let (dir, path) = config_dir("");
        std::fs::write(dir.path().join(".env"), "A=default\nD=default\n").expect("write .env");
        let a = dir.path().join("a.env");
        let b = dir.path().join("b.env");
        std::fs::write(&a, "A=a\nB=a\n").expect("write a");
        std::fs::write(&b, "B=b\nC=${A}\n").expect("write b");
        let (sources, _) = read_sources(Some(&path), &[a, b]);
        let sources = sources.expect("valid");
        assert_eq!(dotenv_value(&sources, "A").as_deref(), Some("a"));
        assert_eq!(dotenv_value(&sources, "B").as_deref(), Some("b"), "later file wins");
        assert_eq!(dotenv_value(&sources, "C").as_deref(), Some("a"));
        assert_eq!(dotenv_value(&sources, "D"), None, "--env disables the default .env");
    }

    #[test]
    fn env_file_without_config_is_valid() {
        let dir = tempfile::tempdir().expect("temp dir");
        let env = dir.path().join("x.env");
        std::fs::write(&env, "A=1\n").expect("write env");
        let (sources, _) = read_sources(None, &[env]);
        assert_eq!(dotenv_value(&sources.expect("valid"), "A").as_deref(), Some("1"));
    }

    #[test]
    fn no_default_env_without_config() {
        let (sources, _) = read_sources(None, &[]);
        assert!(sources.expect("valid").dotenv().get("PATH").is_none());
    }

    #[test]
    fn missing_explicit_env_file_is_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing.env");
        let (sources, _) = read_sources(None, std::slice::from_ref(&missing));
        let error = sources.expect_err("missing --env");
        assert!(matches!(&error, LoadError::Env { path, .. } if *path == missing));
        assert!(
            error
                .to_string()
                .starts_with(&format!("failed to load env file {}:\n  - ", missing.display())),
            "{error}"
        );
    }

    #[test]
    fn malformed_env_file_errors_list_locations_without_values() {
        let (dir, path) = config_dir("");
        let env = dir.path().join(".env");
        std::fs::write(&env, "OK=1\nDB_PASSWORD='hunter2\n").expect("write .env");
        let (sources, _) = read_sources(Some(&path), &[]);
        let error = sources.expect_err("malformed");
        assert_eq!(
            error.to_string(),
            format!(
                "failed to load env file {0}:\n  - {0}:2: unterminated quoted value",
                env.display()
            )
        );
        assert!(!error.to_string().contains("hunter2"));
    }

    #[test]
    fn config_read_error_comes_before_env_errors() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (sources, _) = read_sources(
            Some(&dir.path().join("missing.yaml")),
            &[dir.path().join("missing.env")],
        );
        assert!(matches!(sources, Err(LoadError::Read { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn load_rejects_non_regular_file() {
        let error = load(
            ConfigLayer::default(),
            Some(Path::new("/dev/null")),
            Transport::Stdio,
            None,
        )
        .expect_err("device");
        assert!(error.to_string().ends_with(": not a regular file"), "{error}");
    }

    #[test]
    fn load_makes_relative_path_absolute() {
        let error = load(
            ConfigLayer::default(),
            Some(Path::new("no-such-dbmcp.yaml")),
            Transport::Stdio,
            None,
        )
        .expect_err("missing");
        let LoadError::Read { path, .. } = error else {
            panic!("expected read error");
        };
        assert!(path.is_absolute(), "{}", path.display());
    }
}
