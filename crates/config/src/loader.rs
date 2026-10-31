//! Builds a [`Config`] from the process env, an env file, and a YAML file.
//!
//! Stages run in order and the first failing
//! stage aborts; each stage collects all of its own errors:
//!
//! 1. Read and parse the env file (`--env` > `DBMCP_ENV_FILE`).
//! 2. Pick the config file (`--config` > process `DBMCP_CONFIG` > env-file
//!    `DBMCP_CONFIG`), read it, and parse it into an untyped tree.
//! 3. `${}`-interpolate every string value against process env, then env file.
//! 4. Check the file's structure against the config schema and every set
//!    value from every source by its setting's kind, even values that lose
//!    to a higher source, and pick each winner: non-empty process env >
//!    non-empty env file > YAML (see [`settings`](crate::settings)).
//! 5. Build the [`Config`] from the winners and defaults, applying
//!    activation rules, then run [`Config::validate`] on it.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::dotenv::{self, EnvFile};
use crate::env::{self, CONFIG_VAR, ENV_FILE_VAR, Kind, SETTINGS, Setting, Value};
use crate::error::{ConfigError, Diagnostic};
use crate::http::HttpConfig;
use crate::interpolate;
use crate::log::LogLevel;
use crate::node::{self, SpannedNode};
use crate::options::LoadOptions;
use crate::pii::{PiiCategory, PiiConfig, PiiOperator};
use crate::settings::{self, InputError, Settings};
use crate::source::Source;
use crate::ssh::{self, SshConfig, StrictHostKeyChecking};
use crate::transport::Transport;

/// A successfully loaded configuration.
#[derive(Debug)]
pub struct Loaded {
    /// The validated configuration.
    pub config: Config,
    /// The resolved `log_level` setting.
    pub log_level: LogLevel,
    /// Load-time warnings, to emit once logging is set up.
    pub warnings: Vec<String>,
}

/// Loading failed; carries the warnings collected before the failure.
#[derive(Debug, thiserror::Error)]
#[error("{kind}")]
pub struct LoadError {
    /// Warnings collected before the failing stage.
    pub warnings: Vec<String>,
    kind: LoadErrorKind,
}

#[derive(Debug, thiserror::Error)]
enum LoadErrorKind {
    #[error("failed to read {what} {} (from {origin}): {source}", path.display())]
    Read {
        what: &'static str,
        path: PathBuf,
        origin: String,
        source: std::io::Error,
    },

    #[error("failed to load env file {}:{}", path.display(), Bullets(errors))]
    EnvFile { path: PathBuf, errors: Vec<String> },

    #[error("failed to parse config file {}:{}", path.display(), Bullets(errors))]
    Parse { path: PathBuf, errors: Vec<String> },

    #[error("failed to interpolate config file {}:{}", path.display(), Bullets(errors))]
    Interpolate { path: PathBuf, errors: Vec<String> },

    #[error("invalid settings:{}", Bullets(.0))]
    Settings(Vec<InputError>),

    #[error("configuration validation failed:{}", Bullets(.0))]
    Invalid(Vec<Diagnostic>),
}

struct Bullets<'a, T>(&'a [T]);

impl<T: fmt::Display> fmt::Display for Bullets<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for error in self.0 {
            write!(f, "\n  - {error}")?;
        }
        Ok(())
    }
}

/// Loads and validates the configuration described by `options`.
///
/// # Errors
///
/// Returns [`LoadError`] when a named file cannot be read, the env file or
/// YAML cannot be parsed, `${}` interpolation fails, the YAML does not match
/// the config schema, any source's value fails its setting's checks, or
/// validation fails.
pub fn load(options: &LoadOptions) -> Result<Loaded, LoadError> {
    let mut warnings = Vec::new();
    let fail = |warnings: Vec<String>, kind| LoadError { warnings, kind };

    let env_file = match read_env_file(options) {
        Ok(Some(parsed)) => {
            warnings.extend(parsed.warnings);
            Some(parsed.file)
        }
        Ok(None) => None,
        Err(kind) => return Err(fail(warnings, kind)),
    };
    let lookup = |name: &str| -> Option<String> {
        options
            .environment
            .get(name)
            .cloned()
            .or_else(|| env_file.as_ref().and_then(|file| file.get(name)).map(str::to_owned))
    };

    let mut file = match read_config_file(options, env_file.as_ref()) {
        Ok(file) => file,
        Err(kind) => return Err(fail(warnings, kind)),
    };

    if let Some((path, tree)) = &mut file {
        let in_file = |messages: &[String]| -> Vec<String> {
            messages.iter().map(|m| format!("{}:{m}", path.display())).collect()
        };
        let outcome = interpolate::interpolate(tree, &lookup);
        warnings.extend(in_file(&outcome.warnings));
        if !outcome.errors.is_empty() {
            let errors = in_file(&outcome.errors);
            return Err(fail(
                warnings,
                LoadErrorKind::Interpolate {
                    path: path.clone(),
                    errors,
                },
            ));
        }
    }
    let settings = match settings::check(
        file.as_ref().map(|(path, tree)| (path.as_path(), tree)),
        env_file.as_ref(),
        &options.environment,
    ) {
        Ok(settings) => settings,
        Err(errors) => return Err(fail(warnings, LoadErrorKind::Settings(errors))),
    };

    let resolver = Resolver {
        options,
        file: file.as_ref().map(|(path, _)| path.as_path()),
        settings: &settings,
    };
    match resolver.build() {
        Ok((config, log_level)) => Ok(Loaded {
            config,
            log_level,
            warnings,
        }),
        Err(mut diagnostics) => {
            for diagnostic in &mut diagnostics {
                diagnostic.attach_sources(settings.sources());
            }
            Err(fail(warnings, LoadErrorKind::Invalid(diagnostics)))
        }
    }
}

/// Reads the env file named by `--env` or process `DBMCP_ENV_FILE`, if any.
fn read_env_file(options: &LoadOptions) -> Result<Option<dotenv::Parsed>, LoadErrorKind> {
    let (raw, origin) = if let Some(path) = &options.env_file {
        (path.clone(), "--env".to_owned())
    } else if let Some(path) = non_empty(&options.environment, ENV_FILE_VAR) {
        (PathBuf::from(path), ENV_FILE_VAR.to_owned())
    } else {
        return Ok(None);
    };
    let path = options
        .working_dir
        .join(ssh::expand_tilde(&raw, options.home.as_deref()));
    let text = read_file(&path).map_err(|source| LoadErrorKind::Read {
        what: "env file",
        path: path.clone(),
        origin,
        source,
    })?;
    let lookup = |name: &str| options.environment.get(name).cloned();
    dotenv::parse(&path, &text, &lookup)
        .map(Some)
        .map_err(|errors| LoadErrorKind::EnvFile { path, errors })
}

/// Reads and parses the YAML file, if one is named.
fn read_config_file(
    options: &LoadOptions,
    env_file: Option<&EnvFile>,
) -> Result<Option<(PathBuf, SpannedNode)>, LoadErrorKind> {
    let (raw, origin) = if let Some(path) = &options.config {
        (path.clone(), "--config".to_owned())
    } else if let Some(path) = non_empty(&options.environment, CONFIG_VAR) {
        (PathBuf::from(path), CONFIG_VAR.to_owned())
    } else if let Some((path, line)) = env_file
        .and_then(|file| file.get_with_line(CONFIG_VAR))
        .filter(|(path, _)| !path.is_empty())
    {
        let origin = format!(
            "{CONFIG_VAR} in {}:{line}",
            env_file.map(|f| f.path().display().to_string()).unwrap_or_default()
        );
        (PathBuf::from(path), origin)
    } else {
        return Ok(None);
    };
    let path = options.working_dir.join(raw);
    let text = read_file(&path).map_err(|source| LoadErrorKind::Read {
        what: "config file",
        path: path.clone(),
        origin,
        source,
    })?;
    match node::parse(&text) {
        Ok(tree) => Ok(Some((path, tree))),
        Err(error) => {
            let location = error
                .location()
                .filter(|l| l.line() > 0)
                .map(|l| format!("{}:{}: ", l.line(), l.column()))
                .unwrap_or_default();
            let message = error.to_string();
            let message = message.lines().next().unwrap_or_default();
            Err(LoadErrorKind::Parse {
                errors: vec![format!("{}:{location}{message}", path.display())],
                path,
            })
        }
    }
}

/// Reads `path`, rejecting directories; pipes such as `<(…)` are allowed.
fn read_file(path: &Path) -> std::io::Result<String> {
    if std::fs::metadata(path)?.is_dir() {
        return Err(std::io::Error::other("is a directory"));
    }
    std::fs::read_to_string(path)
}

fn non_empty<'a>(environment: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    environment.get(name).map(String::as_str).filter(|v| !v.is_empty())
}

/// Builds the configuration from each setting's checked winner and defaults.
struct Resolver<'a> {
    options: &'a LoadOptions,
    file: Option<&'a Path>,
    settings: &'a Settings,
}

impl Resolver<'_> {
    /// Returns `true` when any source sets `setting`.
    fn is_set(&self, setting: Setting) -> bool {
        self.settings.get(setting).is_some()
    }

    fn string(&self, setting: Setting) -> Option<String> {
        match self.settings.get(setting)? {
            Value::Text(text) => Some(text.clone()),
            _ => unreachable!("{} is not a text setting", setting.path),
        }
    }

    fn bool(&self, setting: Setting) -> Option<bool> {
        match self.settings.get(setting)? {
            Value::Bool(value) => Some(*value),
            _ => unreachable!("{} is not a boolean setting", setting.path),
        }
    }

    /// Returns an integer setting, already checked against its range.
    ///
    /// # Panics
    ///
    /// Panics when `setting` is not a [`Kind::Int`] or its `max` does not fit
    /// `T`: both are mistakes in the setting table, hit by every load.
    fn int<T: TryFrom<u64>>(&self, setting: Setting) -> Option<T> {
        let Kind::Int { max, .. } = setting.kind else {
            unreachable!("{} is not an integer setting", setting.path);
        };
        assert!(T::try_from(max).is_ok(), "{} max does not fit its field", setting.path);
        match self.settings.get(setting)? {
            Value::Int(value) => T::try_from(*value).ok(),
            _ => unreachable!("{} is not an integer setting", setting.path),
        }
    }

    /// Returns an enum setting, mapping its checked name through `choices`.
    fn enumeration<T: Copy>(&self, setting: Setting, choices: &[(&str, T)]) -> Option<T> {
        match self.settings.get(setting)? {
            Value::Choice(name) => Some(choose(choices, name)),
            _ => unreachable!("{} is not a choice setting", setting.path),
        }
    }

    fn list(&self, setting: Setting) -> Option<Vec<String>> {
        match self.settings.get(setting)? {
            Value::List(items) => Some(items.clone()),
            _ => unreachable!("{} is not a list setting", setting.path),
        }
    }

    /// Returns a list setting whose items were checked against `choices`.
    fn enum_list<T: Copy>(&self, setting: Setting, choices: &[(&str, T)]) -> Option<Vec<T>> {
        Some(self.list(setting)?.iter().map(|item| choose(choices, item)).collect())
    }

    /// Resolves a path setting by its [`Kind::Path`] rule.
    ///
    /// # Panics
    ///
    /// Panics when `setting` is not a [`Kind::Path`], a mistake in the setting table.
    fn path(&self, setting: Setting) -> Option<PathBuf> {
        let Kind::Path { home_from_env } = setting.kind else {
            unreachable!("{} is not a path setting", setting.path);
        };
        let value = PathBuf::from(self.string(setting)?);
        let home = self.options.home.as_deref();
        Some(match self.settings.source(setting) {
            // An empty value stays empty so the section's checks still see it.
            Some(Source::File { .. }) if !value.as_os_str().is_empty() => {
                self.relative_to_file(&ssh::expand_tilde(&value, home))
            }
            _ if home_from_env => ssh::expand_tilde(&value, home),
            _ => value,
        })
    }

    fn relative_to_file(&self, path: &Path) -> PathBuf {
        match self.file.and_then(Path::parent) {
            Some(dir) if path.is_relative() => dir.join(path),
            _ => path.to_path_buf(),
        }
    }

    /// Resolves every setting into a [`Config`] and runs [`Config::validate`].
    ///
    /// Activation rules run here: a section exists only when its trigger is
    /// set, and SSH settings without `ssh.host` are an error.
    fn build(&self) -> Result<(Config, LogLevel), Vec<Diagnostic>> {
        let transport = self
            .enumeration(env::TRANSPORT, Transport::CHOICES)
            .unwrap_or(Transport::Stdio);
        let log_level = self.enumeration(env::LOG_LEVEL, LogLevel::CHOICES).unwrap_or_default();
        let database = self.database();
        let http = (transport == Transport::Http).then(|| self.http());
        let pii = self.bool(env::PII_ENABLE).unwrap_or(false).then(|| self.pii());
        let ssh_set: Vec<Setting> = SETTINGS
            .iter()
            .copied()
            .filter(|setting| setting.path.starts_with("ssh.") && self.is_set(*setting))
            .collect();
        let ssh = ssh_set.contains(&env::SSH_HOST).then(|| self.ssh());
        let missing_ssh_host = ssh.is_none() && !ssh_set.is_empty();

        let config = Config {
            transport,
            database,
            http,
            pii,
            ssh,
        };
        let mut diagnostics = config.validate().err().unwrap_or_default();
        if missing_ssh_host {
            diagnostics.push(
                Diagnostic::new(ConfigError::MissingSshHost)
                    .involving(&ssh_set)
                    .missing(&[env::SSH_HOST]),
            );
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        Ok((config, log_level))
    }

    fn database(&self) -> DatabaseConfig {
        let backend = self
            .enumeration(env::DB_BACKEND, DatabaseBackend::CHOICES)
            .unwrap_or(DatabaseConfig::DEFAULT_BACKEND);
        let name = self
            .string(env::DB_NAME)
            .map(|name| match self.settings.source(env::DB_NAME) {
                Some(Source::File { .. })
                    if backend == DatabaseBackend::Sqlite && !name.is_empty() && name != ":memory:" =>
                {
                    let home = self.options.home.as_deref();
                    let expanded = ssh::expand_tilde(Path::new(&name), home);
                    self.relative_to_file(&expanded).display().to_string()
                }
                _ => name,
            });
        let path_string = |setting| self.path(setting).map(|p| p.display().to_string());
        DatabaseConfig {
            backend,
            host: self
                .string(env::DB_HOST)
                .unwrap_or_else(|| DatabaseConfig::DEFAULT_HOST.into()),
            port: self.int(env::DB_PORT).unwrap_or_else(|| backend.default_port()),
            user: self
                .string(env::DB_USER)
                .unwrap_or_else(|| backend.default_user().into()),
            password: self.string(env::DB_PASSWORD).map(Into::into),
            name,
            charset: self.string(env::DB_CHARSET),
            ssl: self.bool(env::DB_SSL).unwrap_or(DatabaseConfig::DEFAULT_SSL),
            ssl_ca: path_string(env::DB_SSL_CA),
            ssl_cert: path_string(env::DB_SSL_CERT),
            ssl_key: path_string(env::DB_SSL_KEY),
            ssl_verify_cert: self
                .bool(env::DB_SSL_VERIFY_CERT)
                .unwrap_or(DatabaseConfig::DEFAULT_SSL_VERIFY_CERT),
            read_only: self
                .bool(env::DB_READ_ONLY)
                .unwrap_or(DatabaseConfig::DEFAULT_READ_ONLY),
            max_pool_size: self
                .int(env::DB_MAX_POOL_SIZE)
                .unwrap_or(DatabaseConfig::DEFAULT_MAX_POOL_SIZE),
            connection_timeout: self.int(env::DB_CONNECTION_TIMEOUT),
            query_timeout: self
                .int(env::DB_QUERY_TIMEOUT)
                .unwrap_or(DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS),
            page_size: self.int(env::DB_PAGE_SIZE).unwrap_or(DatabaseConfig::DEFAULT_PAGE_SIZE),
        }
    }

    fn http(&self) -> HttpConfig {
        HttpConfig {
            host: self
                .string(env::HTTP_HOST)
                .unwrap_or_else(|| HttpConfig::DEFAULT_HOST.into()),
            port: self.int(env::HTTP_PORT).unwrap_or(HttpConfig::DEFAULT_PORT),
            allowed_origins: self
                .list(env::HTTP_ALLOWED_ORIGINS)
                .unwrap_or_else(HttpConfig::default_allowed_origins),
            allowed_hosts: self
                .list(env::HTTP_ALLOWED_HOSTS)
                .unwrap_or_else(HttpConfig::default_allowed_hosts),
            auth_token: self.string(env::HTTP_AUTH_TOKEN).map(Into::into),
            allow_unauthenticated: self.bool(env::HTTP_ALLOW_UNAUTHENTICATED).unwrap_or(false),
        }
    }

    fn pii(&self) -> PiiConfig {
        PiiConfig {
            operator: self
                .enumeration(env::PII_OPERATOR, PiiOperator::CHOICES)
                .unwrap_or(PiiConfig::DEFAULT_OPERATOR),
            categories: self.enum_list(env::PII_CATEGORIES, PiiCategory::CHOICES),
            ner_enabled: self.bool(env::PII_NER_ENABLE).unwrap_or(PiiConfig::DEFAULT_NER_ENABLED),
            ner_model: self.path(env::PII_NER_MODEL),
        }
    }

    fn ssh(&self) -> SshConfig {
        let home = self.options.home.clone();
        SshConfig {
            host: self.string(env::SSH_HOST).unwrap_or_default(),
            port: self.int(env::SSH_PORT).unwrap_or(SshConfig::DEFAULT_PORT),
            user: self.string(env::SSH_USER).unwrap_or_default(),
            password: self.string(env::SSH_PASSWORD).map(Into::into),
            key: self.path(env::SSH_KEY),
            key_passphrase: self.string(env::SSH_KEY_PASSPHRASE).map(Into::into),
            known_hosts: self
                .path(env::SSH_KNOWN_HOSTS)
                .or_else(|| home.map(|home| home.join(".ssh").join("known_hosts"))),
            strict_host_key_checking: self
                .enumeration(env::SSH_STRICT_HOST_KEY_CHECKING, StrictHostKeyChecking::CHOICES)
                .unwrap_or(SshConfig::DEFAULT_STRICT_HOST_KEY_CHECKING),
            keepalive_interval: Duration::from_secs(
                self.int(env::SSH_KEEPALIVE_INTERVAL)
                    .unwrap_or(SshConfig::DEFAULT_KEEPALIVE_INTERVAL_SECS),
            ),
            local_port: self.int(env::SSH_LOCAL_PORT),
        }
    }
}

/// Maps a checked name to its value in `choices`.
///
/// # Panics
///
/// Panics when `name` is missing: the setting's choices and the type's
/// `CHOICES` disagree, which the setting table's tests rule out.
fn choose<T: Copy>(choices: &[(&str, T)], name: &str) -> T {
    choices
        .iter()
        .find(|(choice, _)| *choice == name)
        .map(|&(_, value)| value)
        .expect("setting choices match the type's CHOICES")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use secrecy::ExposeSecret;
    use tempfile::TempDir;

    use super::*;

    /// Returns the cast and validation problems, if that stage failed.
    fn diagnostics(error: &LoadError) -> &[Diagnostic] {
        match &error.kind {
            LoadErrorKind::Invalid(diagnostics) => diagnostics,
            _ => &[],
        }
    }

    /// A scratch working directory with helpers to write files and load.
    struct Fixture {
        dir: TempDir,
        options: LoadOptions,
    }

    impl Fixture {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            // A home inside the tempdir is absolute on every platform.
            let options = LoadOptions {
                working_dir: dir.path().to_path_buf(),
                home: Some(dir.path().join("home")),
                ..LoadOptions::default()
            };
            Self { dir, options }
        }

        fn write(&self, name: &str, text: &str) -> PathBuf {
            let path = self.dir.path().join(name);
            fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            fs::write(&path, text).expect("write");
            path
        }

        fn env(mut self, name: &str, value: &str) -> Self {
            self.options.environment.insert(name.into(), value.into());
            self
        }

        fn config(mut self, name: &str, text: &str) -> Self {
            self.write(name, text);
            self.options.config = Some(name.into());
            self
        }

        fn env_file(mut self, name: &str, text: &str) -> Self {
            self.write(name, text);
            self.options.env_file = Some(name.into());
            self
        }

        fn load(&self) -> Result<Loaded, LoadError> {
            load(&self.options)
        }

        #[track_caller]
        fn ok(&self) -> Loaded {
            self.load().unwrap_or_else(|e| panic!("load failed: {e}"))
        }

        #[track_caller]
        fn err(&self) -> String {
            self.load().expect_err("load must fail").to_string()
        }
    }

    #[test]
    fn no_inputs_yields_defaults() {
        let loaded = Fixture::new().ok();
        assert_eq!(loaded.config.transport, Transport::Stdio);
        assert_eq!(loaded.log_level, LogLevel::Info);
        assert_eq!(loaded.config.database.backend, DatabaseBackend::Mysql);
        assert_eq!(loaded.config.database.port, 3306);
        assert!(loaded.config.database.read_only);
        assert!(loaded.config.http.is_none() && loaded.config.pii.is_none() && loaded.config.ssh.is_none());
        assert!(loaded.warnings.is_empty());
    }

    /// Names the `choices` entry for `value`.
    fn choice<T: PartialEq>(choices: &[(&'static str, T)], value: &T) -> serde_json::Value {
        let (name, _) = choices.iter().find(|(_, v)| v == value).expect("value has a name");
        (*name).into()
    }

    #[test]
    fn schema_defaults_equal_the_resolved_defaults() {
        use serde_json::Value;

        let plain = Fixture::new().ok();
        let active = Fixture::new()
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_PII_ENABLE", "true")
            .env("DBMCP_SSH_HOST", "bastion")
            .env("DBMCP_SSH_USER", "u")
            .env("DBMCP_SSH_PASSWORD", "p")
            .ok();
        let db = &plain.config.database;
        let http = active.config.http.as_ref().expect("http");
        let pii = active.config.pii.as_ref().expect("pii");
        let ssh = active.config.ssh.as_ref().expect("ssh");
        let document = crate::schema::tests::document();
        let mut compared = 0;
        for setting in SETTINGS {
            let Some(default) = crate::schema::tests::property(&document, setting.path).and_then(|s| s.get("default"))
            else {
                continue;
            };
            let resolved: Value = match setting.path {
                "transport" => choice(Transport::CHOICES, &plain.config.transport),
                "log_level" => choice(LogLevel::CHOICES, &plain.log_level),
                "database.backend" => choice(DatabaseBackend::CHOICES, &db.backend),
                "database.host" => db.host.clone().into(),
                "database.ssl" => db.ssl.into(),
                "database.ssl_verify_cert" => db.ssl_verify_cert.into(),
                "database.read_only" => db.read_only.into(),
                "database.max_pool_size" => db.max_pool_size.into(),
                "database.query_timeout" => db.query_timeout.into(),
                "database.page_size" => db.page_size.into(),
                "http.host" => http.host.clone().into(),
                "http.port" => http.port.into(),
                "http.allowed_origins" => http.allowed_origins.clone().into(),
                "http.allowed_hosts" => http.allowed_hosts.clone().into(),
                "http.allow_unauthenticated" => http.allow_unauthenticated.into(),
                "pii.enabled" => plain.config.pii.is_some().into(),
                "pii.operator" => choice(PiiOperator::CHOICES, &pii.operator),
                "pii.ner_enabled" => pii.ner_enabled.into(),
                "ssh.port" => ssh.port.into(),
                "ssh.strict_host_key_checking" => choice(StrictHostKeyChecking::CHOICES, &ssh.strict_host_key_checking),
                "ssh.keepalive_interval" => ssh.keepalive_interval.as_secs().into(),
                other => panic!("{other} has a schema default; compare it here"),
            };
            assert_eq!(default, &resolved, "{}", setting.path);
            compared += 1;
        }
        assert_eq!(compared, 21);
    }

    #[test]
    fn env_sets_settings_and_empty_is_unset() {
        let loaded = Fixture::new()
            .env("DBMCP_DB_BACKEND", "postgres")
            .env("DBMCP_DB_HOST", "")
            .env("DBMCP_LOG_LEVEL", "debug")
            .ok();
        assert_eq!(loaded.config.database.backend, DatabaseBackend::Postgres);
        assert_eq!(loaded.config.database.port, 5432);
        assert_eq!(loaded.config.database.host, DatabaseConfig::DEFAULT_HOST);
        assert_eq!(loaded.log_level, LogLevel::Debug);
    }

    #[test]
    fn precedence_is_process_env_then_env_file_then_yaml() {
        let loaded = Fixture::new()
            .config(
                "dbmcp.yaml",
                "database:\n  host: yaml\n  user: yaml\n  port: 1111\n  charset: yaml\n",
            )
            .env_file(".env", "DBMCP_DB_HOST=file\nDBMCP_DB_USER=file\nDBMCP_DB_PORT=\n")
            .env("DBMCP_DB_HOST", "process")
            .env("DBMCP_DB_USER", "")
            .ok();
        let db = &loaded.config.database;
        assert_eq!(db.host, "process");
        assert_eq!(db.user, "file", "empty process value lets the env file through");
        assert_eq!(db.port, 1111, "empty env-file value lets YAML through");
        assert_eq!(db.charset.as_deref(), Some("yaml"));
    }

    #[test]
    fn env_file_may_name_the_config_file() {
        let fixture = Fixture::new().env_file("conf/.env", "DBMCP_CONFIG=conf/dbmcp.yaml\n");
        fixture.write("conf/dbmcp.yaml", "transport: http\n");
        assert_eq!(fixture.ok().config.transport, Transport::Http);
    }

    #[test]
    fn config_flag_beats_config_env_vars() {
        let fixture = Fixture::new()
            .config("flag.yaml", "log_level: warn\n")
            .env("DBMCP_CONFIG", "env.yaml");
        fixture.write("env.yaml", "log_level: trace\n");
        assert_eq!(fixture.ok().log_level, LogLevel::Warn);
    }

    #[test]
    fn env_file_var_is_read_from_process_env_with_tilde() {
        let mut fixture = Fixture::new().env("DBMCP_ENV_FILE", "~/x.env");
        fixture.options.home = Some(fixture.dir.path().to_path_buf());
        fixture.write("x.env", "DBMCP_DB_HOST=tilde\n");
        assert_eq!(fixture.ok().config.database.host, "tilde");
    }

    #[test]
    fn missing_files_are_hard_errors_naming_path_and_origin() {
        let fixture = Fixture::new().env("DBMCP_CONFIG", "nope.yaml");
        let error = fixture.err();
        let expected = format!(
            "failed to read config file {}",
            fixture.dir.path().join("nope.yaml").display()
        );
        assert!(error.starts_with(&expected), "{error}");
        assert!(error.contains("(from DBMCP_CONFIG)"), "{error}");

        let mut fixture = Fixture::new();
        fixture.options.env_file = Some(".".into());
        let error = fixture.err();
        assert!(error.contains("failed to read env file") && error.contains("(from --env): is a directory"));
    }

    #[test]
    fn empty_files_are_valid() {
        let loaded = Fixture::new().config("dbmcp.yaml", "").env_file(".env", "").ok();
        assert_eq!(loaded.config.transport, Transport::Stdio);
    }

    #[test]
    fn unknown_keys_are_rejected_with_location() {
        let fixture = Fixture::new().config("dbmcp.yaml", "database:\n  pasword: x\nextra: 1\n");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml");
        assert!(
            error.starts_with(&format!("invalid settings:\n  - {}:", file.display())),
            "{error}"
        );
        assert!(
            error.contains(&format!(
                "{}:2:3: database.pasword: unknown field; expected one of backend,",
                file.display()
            )),
            "{error}"
        );
        assert!(error.contains("extra: unknown field; expected one of transport, log_level, database, http, pii, ssh"));
    }

    #[test]
    fn yaml_values_are_interpolated_from_env_and_env_file() {
        let loaded = Fixture::new()
            .config(
                "dbmcp.yaml",
                "database:\n  password: ${DB_PASS:?set DB_PASS}\n  port: ${PORT}\n  user: $${literal}\n",
            )
            .env_file(".env", "DB_PASS=s3cret\nPORT=6000\n")
            .ok();
        let db = &loaded.config.database;
        assert_eq!(db.password.as_ref().map(ExposeSecret::expose_secret), Some("s3cret"));
        assert_eq!(db.port, 6000);
        assert_eq!(db.user, "${literal}");
    }

    #[test]
    fn interpolation_errors_fail_the_load() {
        let error = Fixture::new()
            .config("dbmcp.yaml", "database:\n  password: ${DB_PASS:?set DB_PASS}\n")
            .err();
        assert!(error.contains("failed to interpolate config file"), "{error}");
        assert!(error.contains(":2:13: database.password: required variable DB_PASS is missing a value: set DB_PASS"));
    }

    #[test]
    fn unset_variables_warn() {
        let loaded = Fixture::new()
            .config("dbmcp.yaml", "database:\n  charset: x${NOPE}\n")
            .ok();
        assert_eq!(loaded.warnings.len(), 1);
        assert!(
            loaded.warnings[0]
                .ends_with(":2:12: database.charset: the \"NOPE\" variable is not set; defaulting to a blank string")
        );
    }

    #[test]
    fn yaml_cast_errors_name_path_and_location_without_the_value() {
        let fixture = Fixture::new().config("dbmcp.yaml", "database:\n  port: \"abc-secret\"\n  ssl: yes\n");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert!(error.starts_with("invalid settings:"), "{error}");
        assert!(error.contains(&format!(
            "{file}:2:9: database.port: expected non-negative integer, found string"
        )));
        assert!(error.contains(&format!(
            "{file}:3:8: database.ssl: expected boolean (true/false/1/0/t/f), found string"
        )));
        assert!(!error.contains("abc-secret"));
    }

    #[test]
    fn empty_yaml_values_at_typed_settings_are_unset() {
        let loaded = Fixture::new()
            .config(
                "dbmcp.yaml",
                "database:\n  port: ${PORT}\n  read_only: \"\"\n  charset: ${NOPE}\n",
            )
            .ok();
        let db = &loaded.config.database;
        assert_eq!(db.port, 3306, "an unset variable falls back to the default");
        assert!(db.read_only);
        assert_eq!(db.charset.as_deref(), Some(""), "strings keep the empty value");
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);

        let loaded = Fixture::new()
            .config("dbmcp.yaml", "database:\n  port: ${PORT}\n")
            .env_file(".env", "DBMCP_DB_PORT=6000\n")
            .ok();
        assert_eq!(loaded.config.database.port, 6000);
    }

    #[test]
    fn env_type_errors_name_path_and_source_without_the_value() {
        let error = Fixture::new()
            .env("DBMCP_DB_PORT", "abc-secret")
            .env("DBMCP_DB_READ_ONLY", "on")
            .err();
        assert_eq!(
            error,
            "invalid settings:\n  \
             - DBMCP_DB_PORT: database.port: expected non-negative integer, found string\n  \
             - DBMCP_DB_READ_ONLY: database.read_only: expected boolean (true/false/1/0/t/f), found string"
        );
        assert!(!error.contains("abc-secret"));
    }

    #[test]
    fn input_errors_are_sorted_by_location() {
        let fixture = Fixture::new().config(
            "dbmcp.yaml",
            "ssh:\n  port: 0\ndatabase:\n  backend: oracle\n  page_size: 1.5\n  host: [a]\nhttp:\n  allowed_hosts: [a, 1]\n",
        );
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert_eq!(
            error,
            format!(
                "invalid settings:\n  \
                 - {file}:2:9: ssh.port: must be at least 1\n  \
                 - {file}:4:12: database.backend: expected one of mysql, mariadb, postgres, sqlite\n  \
                 - {file}:5:14: database.page_size: expected non-negative integer, found float\n  \
                 - {file}:6:9: database.host: expected string, found sequence\n  \
                 - {file}:8:18: http.allowed_hosts: item 1: expected string, found integer"
            )
        );
        assert!(!error.contains("oracle"));
    }

    #[test]
    fn whole_file_is_validated_even_when_env_overrides() {
        let error = Fixture::new()
            .config("dbmcp.yaml", "database:\n  port: abc\n")
            .env("DBMCP_DB_PORT", "5432")
            .err();
        assert!(error.starts_with("invalid settings:"), "{error}");

        let error = Fixture::new()
            .config("dbmcp.yaml", "transport: sse\n")
            .env("DBMCP_TRANSPORT", "stdio")
            .err();
        assert!(error.starts_with("invalid settings:"), "{error}");
        assert!(error.contains("transport: expected one of stdio, http"), "{error}");
    }

    #[test]
    fn non_mapping_root_is_rejected() {
        let error = Fixture::new().config("dbmcp.yaml", "- a\n").err();
        assert!(
            error.contains(":1:1: <root>: expected mapping, found sequence"),
            "{error}"
        );
    }

    #[test]
    fn validation_errors_carry_the_source() {
        let fixture = Fixture::new().config("dbmcp.yaml", "transport: http\nhttp:\n  host: \"\"\n");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert!(
            error.contains(&format!("http.host must not be empty (from {file}:3:9)")),
            "{error}"
        );
    }

    #[test]
    fn missing_required_settings_name_every_route() {
        let error = Fixture::new().env("DBMCP_DB_BACKEND", "sqlite").err();
        assert!(
            error.contains(
                "database.name (file path) is required for sqlite; set DBMCP_DB_NAME or database.name in the config file \
                 (from DBMCP_DB_BACKEND)"
            ),
            "{error}"
        );
    }

    #[test]
    fn file_paths_resolve_against_the_config_directory() {
        let fixture = Fixture::new().config(
            "conf/dbmcp.yaml",
            "database:\n  backend: sqlite\n  name: data/app.db\n  ssl_ca: certs/ca.pem\npii:\n  enabled: true\n  ner_model: ~/models\n",
        );
        let loaded = fixture.ok();
        let conf = fixture.dir.path().join("conf");
        assert_eq!(
            loaded.config.database.name,
            Some(conf.join("data/app.db").display().to_string())
        );
        assert_eq!(
            loaded.config.database.ssl_ca,
            Some(conf.join("certs/ca.pem").display().to_string())
        );
        assert_eq!(
            loaded.config.pii.expect("pii").ner_model,
            Some(fixture.dir.path().join("home").join("models"))
        );
    }

    #[test]
    fn env_paths_and_special_sqlite_names_stay_as_given() {
        let loaded = Fixture::new()
            .config(
                "conf/dbmcp.yaml",
                "database:\n  backend: sqlite\n  name: \":memory:\"\n",
            )
            .env("DBMCP_DB_SSL_CA", "rel/ca.pem")
            .ok();
        assert_eq!(loaded.config.database.name.as_deref(), Some(":memory:"));
        assert_eq!(loaded.config.database.ssl_ca.as_deref(), Some("rel/ca.pem"));
    }

    #[test]
    fn lists_come_from_comma_text_or_yaml_sequences() {
        let loaded = Fixture::new()
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_HTTP_ALLOWED_HOSTS", " a , b")
            .config(
                "dbmcp.yaml",
                "http:\n  allowed_origins: []\npii:\n  enabled: true\n  categories: [crypto, network]\n",
            )
            .ok();
        let http = loaded.config.http.expect("http");
        assert_eq!(http.allowed_hosts, ["a", "b"]);
        assert!(http.allowed_origins.is_empty(), "[] disables Origin validation");
        assert_eq!(
            loaded.config.pii.expect("pii").categories,
            Some(vec![PiiCategory::Crypto, PiiCategory::Network])
        );
    }

    #[test]
    fn yaml_strings_are_comma_separated_lists() {
        let loaded = Fixture::new()
            .config("dbmcp.yaml", "transport: http\nhttp:\n  allowed_hosts: ${HOSTS}\n")
            .env("HOSTS", "a, b")
            .ok();
        assert_eq!(loaded.config.http.expect("http").allowed_hosts, ["a", "b"]);
    }

    #[test]
    fn list_errors() {
        let error = Fixture::new()
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_HTTP_ALLOWED_HOSTS", "a,,b")
            .env("DBMCP_PII_ENABLE", "true")
            .env("DBMCP_PII_CATEGORIES", "crypto,bogus")
            .err();
        assert!(
            error.contains("DBMCP_HTTP_ALLOWED_HOSTS: http.allowed_hosts: empty item in comma-separated list"),
            "{error}"
        );
        assert!(
            error.contains("DBMCP_PII_CATEGORIES: pii.categories: item 1: expected one of personal,"),
            "{error}"
        );

        let error = Fixture::new()
            .config("dbmcp.yaml", "transport: http\nhttp:\n  allowed_hosts: []\n")
            .err();
        assert!(
            error.contains("http.allowed_hosts must not be empty when set"),
            "{error}"
        );
    }

    #[test]
    fn http_settings_are_ignored_under_stdio() {
        let loaded = Fixture::new()
            .config("dbmcp.yaml", "http:\n  host: \"\"\n  allowed_hosts: []\n")
            .ok();
        assert!(loaded.config.http.is_none());
    }

    #[test]
    fn ssh_activates_on_host_and_requires_it_otherwise() {
        let fixture = Fixture::new()
            .env("DBMCP_SSH_HOST", "bastion")
            .env("DBMCP_SSH_USER", "deploy")
            .env("DBMCP_SSH_PASSWORD", "pw");
        let ssh = fixture.ok().config.ssh.expect("ssh");
        assert_eq!(ssh.host, "bastion");
        assert_eq!(
            ssh.known_hosts,
            Some(fixture.dir.path().join("home").join(".ssh").join("known_hosts"))
        );

        let error = Fixture::new().config("dbmcp.yaml", "ssh:\n  user: deploy\n").err();
        assert!(
            error.contains("ssh.host is required when other ssh settings are set"),
            "{error}"
        );
    }

    #[test]
    fn ssh_on_sqlite_names_both_sources() {
        let fixture = Fixture::new()
            .config("dbmcp.yaml", "ssh:\n  host: bastion\n  user: u\n  password: p\n")
            .env("DBMCP_DB_BACKEND", "sqlite")
            .env("DBMCP_DB_NAME", "x.db");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert!(
            error.contains(&format!(
                "ssh.host: not supported with the sqlite backend (ssh.host from {file}:2:9; database.backend from DBMCP_DB_BACKEND)"
            )),
            "{error}"
        );
    }

    #[test]
    fn env_file_errors_are_reported_per_line() {
        let fixture = Fixture::new().env_file(".env", "OK=1\nBAD KEY=2\n");
        let error = fixture.err();
        assert!(error.contains("failed to load env file"), "{error}");
        assert!(error.contains("\n  - line 2: key cannot contain a space"), "{error}");
    }

    #[test]
    fn env_file_sources_show_file_and_line() {
        let fixture = Fixture::new().env_file(".env", "# c\nDBMCP_DB_PAGE_SIZE=9999\n");
        let error = fixture.err();
        let file = fixture.dir.path().join(".env").display().to_string();
        assert!(
            error.contains(&format!(
                "DBMCP_DB_PAGE_SIZE in {file}:2: database.page_size: must be at most 500"
            )),
            "{error}"
        );
    }

    #[test]
    fn yaml_syntax_errors_name_the_file() {
        let fixture = Fixture::new().config("dbmcp.yaml", "database: [1, 2\n");
        let error = fixture.err();
        assert!(error.starts_with("failed to parse config file"), "{error}");
    }

    #[test]
    fn diagnostics_are_typed_and_carry_their_sources() {
        let fixture = Fixture::new()
            .config("dbmcp.yaml", "ssh:\n  user: deploy\n")
            .env("DBMCP_SSH_PORT", "2222");
        let error = fixture.load().expect_err("ssh without host");
        let [diagnostic] = diagnostics(&error) else {
            panic!("one diagnostic: {error}");
        };
        assert!(matches!(diagnostic.error, ConfigError::MissingSshHost));
        assert_eq!(diagnostic.missing, [env::SSH_HOST]);
        let file = fixture.dir.path().join("dbmcp.yaml");
        assert_eq!(
            diagnostic.involved,
            [
                (env::SSH_PORT, Some(Source::Env("DBMCP_SSH_PORT"))),
                (
                    env::SSH_USER,
                    Some(Source::File {
                        path: file.clone(),
                        line: 2,
                        column: 9
                    })
                ),
            ]
        );
        assert!(error.to_string().contains(&format!(
            "ssh.host is required when other ssh settings are set; set DBMCP_SSH_HOST or ssh.host in the config file \
             (ssh.port from DBMCP_SSH_PORT; ssh.user from {}:2:9)",
            file.display()
        )));
    }

    #[test]
    fn input_errors_stop_before_validation() {
        let error = Fixture::new()
            .env("DBMCP_DB_PORT", "x")
            .env("DBMCP_DB_BACKEND", "sqlite")
            .load()
            .expect_err("bad port");
        let LoadErrorKind::Settings(errors) = &error.kind else {
            panic!("only the input error: {error}");
        };
        assert_eq!(errors.len(), 1, "{error}");
        assert_eq!(errors[0].path, "database.port");
    }

    #[test]
    fn earlier_stages_have_no_diagnostics() {
        let error = Fixture::new()
            .env("DBMCP_CONFIG", "missing.yaml")
            .load()
            .expect_err("missing file");
        assert!(diagnostics(&error).is_empty());
    }

    #[test]
    fn secrets_never_appear_in_errors() {
        let error = Fixture::new()
            .config("dbmcp.yaml", "database:\n  password: 12345\n")
            .err();
        assert!(
            error.contains("database.password: expected string, found integer"),
            "{error}"
        );
        assert!(!error.contains("12345"));
    }
}
