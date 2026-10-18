//! Loading the YAML config file and layering it under the CLI.
//!
//! [`load`] is the single entry point: it reads the optional config file,
//! stacks the CLI layer on top, and resolves the result into a validated
//! [`Config`](crate::Config). Each stage fails fast with a [`LoadError`].
//!
//! Paths written in the file expand a leading `~` and then resolve
//! relative to the file's directory, so the file means the same thing
//! whatever the current directory is. CLI and env paths are left alone.

use std::path::{Path, PathBuf};

use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::error::ConfigErrors;
use crate::layer::{ConfigLayer, Resolved, Transport};
use crate::ssh;

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

    /// The merged configuration violates one or more rules.
    #[error("configuration validation failed:{}", Bullets(.0))]
    Invalid(#[from] ConfigErrors),
}

struct Bullets<'a>(&'a ConfigErrors);

impl std::fmt::Display for Bullets<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for error in self.0.iter() {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

/// Merges `cli` over the optional config `file` and resolves the result.
///
/// A relative `file` path resolves against the current directory. Path
/// settings read from the file resolve against the file's directory, after
/// expanding a leading `~` against `home`; `database.name` does so only when
/// it came from the file and the merged backend is `sqlite`. `home` is also
/// forwarded to [`ConfigLayer::resolve`].
///
/// # Errors
///
/// Returns [`LoadError::Read`] when `file` is missing, not a regular file,
/// or unreadable; [`LoadError::Parse`] when it is not a valid config
/// document; and [`LoadError::Invalid`] when the merged configuration
/// fails validation.
pub fn load(
    cli: ConfigLayer,
    file: Option<&Path>,
    transport: Transport,
    home: Option<&Path>,
) -> Result<Resolved, LoadError> {
    let Some(file) = file else {
        return Ok(cli.resolve(transport, home)?);
    };
    let (path, mut file_layer) = read_file(file)?;
    let dir = path.parent().unwrap_or(Path::new("/"));
    resolve_file_paths(&mut file_layer, dir, home);

    let name_from_file = cli.database.name.is_none() && file_layer.database.name.is_some();
    let mut merged = cli.or(file_layer);
    let backend = merged.database.backend.unwrap_or(DatabaseConfig::DEFAULT_BACKEND);
    if name_from_file
        && backend == DatabaseBackend::Sqlite
        && let Some(name) = &mut merged.database.name
        && !is_sqlite_special_name(name)
    {
        *name = resolve_path_string(name, dir, home);
    }
    Ok(merged.resolve(transport, home)?)
}

/// Resolves every path setting of a file layer against `dir`.
fn resolve_file_paths(layer: &mut ConfigLayer, dir: &Path, home: Option<&Path>) {
    let db = &mut layer.database;
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
fn resolve_path(path: &Path, dir: &Path, home: Option<&Path>) -> PathBuf {
    let expanded = ssh::expand_tilde(path, home);
    if expanded.is_absolute() {
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
fn is_sqlite_special_name(name: &str) -> bool {
    name.is_empty() || name == ":memory:" || name.starts_with("file:")
}

/// Reads and parses the config file at `path`, returning its absolute path.
fn read_file(path: &Path) -> Result<(PathBuf, ConfigLayer), LoadError> {
    let read_error = |path: PathBuf, reason: String| LoadError::Read { path, reason };
    let path = std::path::absolute(path).map_err(|e| read_error(path.to_path_buf(), e.to_string()))?;
    let metadata = std::fs::metadata(&path).map_err(|e| read_error(path.clone(), e.to_string()))?;
    if metadata.is_dir() {
        return Err(read_error(path, "is a directory".into()));
    }
    if !metadata.is_file() {
        return Err(read_error(path, "not a regular file".into()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| read_error(path.clone(), e.to_string()))?;
    match parse(&text) {
        Ok(layer) => Ok((path, layer)),
        Err(message) => Err(LoadError::Parse { path, message }),
    }
}

/// Parses a config document; an empty or `null` document is an empty layer.
fn parse(text: &str) -> Result<ConfigLayer, String> {
    let options = serde_saphyr::options! {
        strict_booleans: true,
        with_snippet: false,
    };
    serde_saphyr::from_str_with_options::<Option<ConfigLayer>>(text, options)
        .map(Option::unwrap_or_default)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;
    use crate::{ConfigError, LogLevel, PiiCategory, StrictHostKeyChecking};

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
    fn bool_fields_take_only_true_and_false() {
        assert_eq!(
            parse_ok("database:\n  read_only: false\n").database.read_only,
            Some(false)
        );
        parse("database:\n  read_only: yes\n").expect_err("yes is not a bool");
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
        assert!(matches!(&error, LoadError::Invalid(e) if matches!(e[0], ConfigError::MissingSqliteDbName)));
        assert_eq!(
            error.to_string(),
            "configuration validation failed:\n  - DB_NAME (file path) is required for SQLite"
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

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
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
        let mut layer = read_file(&path).expect("parse").1;
        layer.ssh.key = Some(PathBuf::from("~/id"));
        resolve_file_paths(&mut layer, Path::new("/cfg"), Some(&home()));
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
        resolve_file_paths(&mut layer, Path::new("/cfg"), Some(&home()));
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
    fn sqlite_memory_and_uri_names_pass_through() {
        for name in [":memory:", "file:app.db?mode=ro"] {
            let (_dir, path) = config_dir(&format!("database:\n  backend: sqlite\n  name: '{name}'\n"));
            let config = load_file(&path, ConfigLayer::default()).config;
            assert_eq!(config.database.name.as_deref(), Some(name));
        }
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
