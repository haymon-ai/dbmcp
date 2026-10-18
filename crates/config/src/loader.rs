//! Loading the YAML config file and layering it under the CLI.
//!
//! [`load`] is the single entry point: it reads the optional config file,
//! stacks the CLI layer on top, and resolves the result into a validated
//! [`Config`](crate::Config). Each stage fails fast with a [`LoadError`].

use std::path::{Path, PathBuf};

use crate::error::ConfigErrors;
use crate::layer::{ConfigLayer, Resolved, Transport};

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
/// A relative `file` path resolves against the current directory. `home`
/// is forwarded to [`ConfigLayer::resolve`].
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
    let file_layer = match file {
        Some(path) => read_file(path)?,
        None => ConfigLayer::default(),
    };
    Ok(cli.or(file_layer).resolve(transport, home)?)
}

/// Reads and parses the config file at `path`.
fn read_file(path: &Path) -> Result<ConfigLayer, LoadError> {
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
    parse(&text).map_err(|message| LoadError::Parse { path, message })
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
    use crate::{ConfigError, DatabaseBackend, DatabaseConfig, LogLevel, PiiCategory, StrictHostKeyChecking};

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
