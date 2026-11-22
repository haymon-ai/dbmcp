//! Builds a [`Config`] from the process env, env files, and YAML config files.
//!
//! Stages run in order and the first failing
//! stage aborts; each stage collects all of its own errors:
//!
//! 1. Read every source and pick each setting's winner (see [`settings`]):
//!    the env files, then the config files, each stopping at the first file
//!    with errors, then every set value from every source checked by its
//!    setting's kind, even values that lose to a higher source. Winners
//!    follow non-empty process env > the last env file's value, if
//!    non-empty > the last config file setting it. There is no merged YAML
//!    tree.
//! 2. Build the [`Config`] from the winners and defaults, applying
//!    activation rules and resolving a sqlite database name from a config
//!    or env file against that file's directory, then run
//!    [`Config::validate`] on it.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::env::{self, Kind, SETTINGS, Setting, Value};
use crate::error::{ConfigError, Diagnostic, duplicates};
use crate::http::HttpConfig;
use crate::log::LogLevel;
use crate::options::LoadOptions;
use crate::privacy::{ColumnRule, PrivacyCategory, PrivacyConfig, PrivacyMode, PrivacyOperator};
use crate::rules::{self, Rule};
use crate::settings::{self, InputError, Settings};
use crate::source::Source;
use crate::ssh::{SshConfig, StrictHostKeyChecking};
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

/// Why loading failed; each variant renders one user-facing message.
#[derive(Debug, thiserror::Error)]
pub(crate) enum LoadErrorKind {
    #[error("{origin}: {problem}")]
    List { origin: String, problem: &'static str },

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
/// Returns [`LoadError`] when a file list has an empty entry, a named file
/// cannot be read, an env file or a config file cannot be parsed, `${}`
/// interpolation fails, a config file does not match the config schema,
/// any source's value fails its setting's checks, or validation fails.
pub fn load(options: &LoadOptions) -> Result<Loaded, LoadError> {
    let mut warnings = Vec::new();
    let settings = match settings::read(options, &mut warnings) {
        Ok(settings) => settings,
        Err(kind) => return Err(LoadError { warnings, kind }),
    };

    let resolver = Resolver {
        settings: &settings,
        home: options.home.as_deref(),
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
            Err(LoadError {
                warnings,
                kind: LoadErrorKind::Invalid(diagnostics),
            })
        }
    }
}

/// Builds the configuration from each setting's checked winner and defaults.
struct Resolver<'a> {
    settings: &'a Settings,
    /// The home directory, for `~` and the `known_hosts` default.
    home: Option<&'a Path>,
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

    /// Returns a path setting, already resolved by its [`Kind::Path`] rule.
    fn path(&self, setting: Setting) -> Option<PathBuf> {
        match self.settings.get(setting)? {
            Value::Path(path) => Some(path.clone()),
            _ => unreachable!("{} is not a path setting", setting.path),
        }
    }

    /// Resolves every setting into a [`Config`] and runs [`Config::validate`].
    ///
    /// Activation rules run here: a section exists only when its trigger is
    /// set, and SSH settings without `ssh.host` are an error. `privacy.rules`
    /// resolves and `privacy.key` and `privacy.categories` are checked here
    /// too, so their errors show in every privacy mode, `none` included.
    fn build(&self) -> Result<(Config, LogLevel), Vec<Diagnostic>> {
        let transport = self
            .enumeration(env::TRANSPORT, Transport::CHOICES)
            .unwrap_or(Transport::Stdio);
        let log_level = self.enumeration(env::LOG_LEVEL, LogLevel::CHOICES).unwrap_or_default();
        let database = self.database();
        let http = (transport == Transport::Http).then(|| self.http());
        let operator = self
            .enumeration(env::PRIVACY_OPERATOR, PrivacyOperator::CHOICES)
            .unwrap_or(PrivacyConfig::DEFAULT_OPERATOR);
        let (column_rules, rule_errors) = match rules::resolve(self.rules(), &database, operator) {
            Ok(column_rules) => (column_rules, Vec::new()),
            Err(errors) => (Vec::new(), errors),
        };
        let mode = self
            .enumeration(env::PRIVACY_MODE, PrivacyMode::CHOICES)
            .unwrap_or(PrivacyMode::DEFAULT);
        let categories = self.enum_list(env::PRIVACY_CATEGORIES, PrivacyCategory::CHOICES);
        let mut category_errors = Vec::new();
        if let Some(list) = &categories {
            if list.is_empty() {
                category_errors.push(
                    Diagnostic::new(ConfigError::EmptyList(env::PRIVACY_CATEGORIES.path))
                        .involving(&[env::PRIVACY_CATEGORIES]),
                );
            }
            category_errors.extend(duplicates(env::PRIVACY_CATEGORIES, list));
        }
        let privacy = (mode != PrivacyMode::None).then(|| self.privacy(mode, operator, categories, column_rules));
        let key_error = self.string(env::PRIVACY_KEY).and_then(|key| {
            if key.trim().is_empty() {
                Some(ConfigError::EmptyPrivacyKey)
            } else if key.len() < PrivacyConfig::MIN_KEY_BYTES {
                Some(ConfigError::PrivacyKeyTooShort(key.len()))
            } else {
                None
            }
        });
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
            privacy,
            ssh,
        };
        let mut diagnostics = config.validate().err().unwrap_or_default();
        diagnostics.extend(category_errors);
        if missing_ssh_host {
            diagnostics.push(
                Diagnostic::new(ConfigError::MissingSshHost)
                    .involving(&ssh_set)
                    .missing(&[env::SSH_HOST]),
            );
        }
        if let Some(error) = key_error {
            diagnostics.push(Diagnostic::new(error).involving(&[env::PRIVACY_KEY]));
        }
        diagnostics.extend(rule_errors);
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
                Some(Source::File { path, .. } | Source::EnvFile { path, .. })
                    if backend == DatabaseBackend::Sqlite && !name.is_empty() && name != ":memory:" =>
                {
                    let dir = path.parent().unwrap_or(Path::new(""));
                    env::resolve_in_file(Path::new(&name), dir, self.home)
                        .display()
                        .to_string()
                }
                _ => name,
            });
        let path_string = |setting| self.path(setting).map(|p| p.display().to_string());
        let query_timeout = self
            .int(env::DB_QUERY_TIMEOUT)
            .unwrap_or(DatabaseConfig::DEFAULT_QUERY_TIMEOUT_SECS);
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
            query_timeout,
            max_query_timeout: self.int(env::DB_MAX_QUERY_TIMEOUT).unwrap_or(query_timeout),
            page_size: self.int(env::DB_PAGE_SIZE).unwrap_or(DatabaseConfig::DEFAULT_PAGE_SIZE),
            max_cell_chars: self
                .int(env::DB_MAX_CELL_CHARS)
                .unwrap_or(DatabaseConfig::DEFAULT_MAX_CELL_CHARS),
            max_response_chars: self
                .int(env::DB_MAX_RESPONSE_CHARS)
                .unwrap_or(DatabaseConfig::DEFAULT_MAX_RESPONSE_CHARS),
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

    /// Returns the winning `privacy.rules` value; unset is no rules.
    fn rules(&self) -> &[Rule] {
        match self.settings.get(env::PRIVACY_RULES) {
            None => &[],
            Some(Value::Rules(rules)) => rules,
            Some(_) => unreachable!("privacy.rules is not a rules setting"),
        }
    }

    /// Resolves the privacy section around its mode, operator, categories, and resolved rules.
    fn privacy(
        &self,
        mode: PrivacyMode,
        operator: PrivacyOperator,
        categories: Option<Vec<PrivacyCategory>>,
        rules: Vec<ColumnRule>,
    ) -> PrivacyConfig {
        let hashing =
            operator == PrivacyOperator::Hash || rules.iter().any(|rule| rule.operator == PrivacyOperator::Hash);
        PrivacyConfig {
            mode,
            operator,
            key: hashing.then(|| self.string(env::PRIVACY_KEY).map(Into::into)).flatten(),
            rules,
            categories,
            model: self.path(env::PRIVACY_MODEL),
        }
    }

    fn ssh(&self) -> SshConfig {
        SshConfig {
            host: self.string(env::SSH_HOST).unwrap_or_default(),
            port: self.int(env::SSH_PORT).unwrap_or(SshConfig::DEFAULT_PORT),
            user: self.string(env::SSH_USER).unwrap_or_default(),
            password: self.string(env::SSH_PASSWORD).map(Into::into),
            key: self.path(env::SSH_KEY),
            key_passphrase: self.string(env::SSH_KEY_PASSPHRASE).map(Into::into),
            known_hosts: self
                .path(env::SSH_KNOWN_HOSTS)
                .or_else(|| self.home.map(|home| home.join(".ssh").join("known_hosts"))),
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
/// Panics when `name` is missing, which cannot happen: each setting's
/// choices are the names in its type's `CHOICES`.
fn choose<T: Copy>(choices: &[(&str, T)], name: &str) -> T {
    choices
        .iter()
        .find(|(choice, _)| *choice == name)
        .map(|&(_, value)| value)
        .expect("setting choices are the type's CHOICES names")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use secrecy::ExposeSecret;
    use tempfile::TempDir;

    use super::*;

    /// Returns the validation problems, if that stage failed.
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
            self.options.config_files.push(name.into());
            self
        }

        fn env_file(mut self, name: &str, text: &str) -> Self {
            self.write(name, text);
            self.options.env_files.push(name.into());
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
        assert!(loaded.config.http.is_none() && loaded.config.ssh.is_none());
        assert_eq!(
            loaded.config.privacy.map(|privacy| privacy.mode),
            Some(PrivacyMode::Full)
        );
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
            .env("DBMCP_SSH_HOST", "bastion")
            .env("DBMCP_SSH_USER", "u")
            .env("DBMCP_SSH_PASSWORD", "p")
            .ok();
        let db = &plain.config.database;
        let http = active.config.http.as_ref().expect("http");
        let privacy = active.config.privacy.as_ref().expect("privacy");
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
                "database.max_cell_chars" => db.max_cell_chars.into(),
                "database.max_response_chars" => db.max_response_chars.into(),
                "http.host" => http.host.clone().into(),
                "http.port" => http.port.into(),
                "http.allowed_origins" => http.allowed_origins.clone().into(),
                "http.allowed_hosts" => http.allowed_hosts.clone().into(),
                "http.allow_unauthenticated" => http.allow_unauthenticated.into(),
                "privacy.mode" => choice(
                    PrivacyMode::CHOICES,
                    &plain
                        .config
                        .privacy
                        .as_ref()
                        .map_or(PrivacyMode::None, |privacy| privacy.mode),
                ),
                "privacy.operator" => choice(PrivacyOperator::CHOICES, &privacy.operator),
                "ssh.port" => ssh.port.into(),
                "ssh.strict_host_key_checking" => choice(StrictHostKeyChecking::CHOICES, &ssh.strict_host_key_checking),
                "ssh.keepalive_interval" => ssh.keepalive_interval.as_secs().into(),
                other => panic!("{other} has a schema default; compare it here"),
            };
            assert_eq!(default, &resolved, "{}", setting.path);
            compared += 1;
        }
        assert_eq!(compared, 22);
    }

    #[test]
    fn max_query_timeout_defaults_to_query_timeout() {
        let loaded = Fixture::new().env("DBMCP_DB_QUERY_TIMEOUT", "45").ok();
        assert_eq!(loaded.config.database.max_query_timeout, 45);
        let loaded = Fixture::new()
            .env("DBMCP_DB_QUERY_TIMEOUT", "45")
            .env("DBMCP_DB_MAX_QUERY_TIMEOUT", "300")
            .ok();
        assert_eq!(loaded.config.database.max_query_timeout, 300);
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
    fn env_file_may_name_the_config_file_relative_to_itself() {
        let fixture = Fixture::new().env_file("conf/.env", "DBMCP_CONFIG_FILE=dbmcp.yaml\n");
        fixture.write("conf/dbmcp.yaml", "transport: http\n");
        assert_eq!(fixture.ok().config.transport, Transport::Http);
    }

    #[test]
    fn config_flag_beats_config_env_vars() {
        let fixture = Fixture::new()
            .config("flag.yaml", "log_level: warn\n")
            .env("DBMCP_CONFIG_FILE", "env.yaml");
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
        let fixture = Fixture::new().env("DBMCP_CONFIG_FILE", "nope.yaml");
        let error = fixture.err();
        let expected = format!(
            "failed to read config file {}",
            fixture.dir.path().join("nope.yaml").display()
        );
        assert!(error.starts_with(&expected), "{error}");
        assert!(error.contains("(from DBMCP_CONFIG_FILE)"), "{error}");

        let mut fixture = Fixture::new();
        fixture.options.env_files = vec![".".into()];
        let error = fixture.err();
        assert!(error.contains("failed to read env file") && error.contains("(from --env-file): is a directory"));
    }

    #[test]
    fn later_env_files_win_and_see_earlier_ones() {
        let loaded = Fixture::new()
            .env_file("a.env", "X=a\nDBMCP_DB_HOST=a\nDBMCP_DB_USER=a\n")
            .env_file("b.env", "DBMCP_DB_HOST=${X}-b\nX=b\nDBMCP_DB_CHARSET=${X}\n")
            .ok();
        let db = &loaded.config.database;
        assert_eq!(db.host, "a-b", "b's ${{X}} sees a's X");
        assert_eq!(db.user, "a");
        assert_eq!(db.charset.as_deref(), Some("b"), "b's own earlier line beats a");
    }

    #[test]
    fn env_file_flag_replaces_the_env_var_list() {
        let fixture = Fixture::new()
            .env("DBMCP_ENV_FILE", "x.env")
            .env_file("a.env", "DBMCP_DB_USER=a\n");
        fixture.write("x.env", "DBMCP_DB_HOST=x\n");
        let db = fixture.ok().config.database;
        assert_eq!(
            (db.host.as_str(), db.user.as_str()),
            (DatabaseConfig::DEFAULT_HOST, "a")
        );
    }

    #[test]
    fn env_file_var_is_a_trimmed_comma_list_with_tilde() {
        let mut fixture = Fixture::new().env("DBMCP_ENV_FILE", "a.env , ~/b.env");
        fixture.options.home = Some(fixture.dir.path().join("home"));
        fixture.write("a.env", "DBMCP_DB_HOST=a\nDBMCP_DB_USER=a\n");
        fixture.write("home/b.env", "DBMCP_DB_HOST=b\n");
        let db = fixture.ok().config.database;
        assert_eq!((db.host.as_str(), db.user.as_str()), ("b", "a"));
    }

    #[test]
    fn empty_env_file_entries_are_rejected_before_reading() {
        let error = Fixture::new().env("DBMCP_ENV_FILE", "a.env,,b.env").err();
        assert_eq!(error, "DBMCP_ENV_FILE: empty entry in file list");

        let mut fixture = Fixture::new();
        fixture.options.env_files = vec!["missing.env".into(), PathBuf::new()];
        assert_eq!(fixture.err(), "--env-file: empty path");
    }

    #[test]
    fn a_later_empty_env_file_value_clears_the_setting() {
        let fixture = Fixture::new()
            .env_file("a.env", "DBMCP_DB_HOST=a\n")
            .env_file("b.env", "DBMCP_DB_HOST=\n");
        assert_eq!(fixture.ok().config.database.host, DatabaseConfig::DEFAULT_HOST);
        let loaded = fixture.config("dbmcp.yaml", "database:\n  host: yaml\n").ok();
        assert_eq!(loaded.config.database.host, "yaml");
    }

    #[test]
    fn overwritten_env_file_values_are_still_checked() {
        let fixture = Fixture::new()
            .env_file("a.env", "DBMCP_DB_PORT=abc\n")
            .env_file("b.env", "DBMCP_DB_PORT=5432\n");
        let a = fixture.dir.path().join("a.env").display().to_string();
        assert_eq!(
            fixture.err(),
            format!(
                "invalid settings:\n  - DBMCP_DB_PORT in {a}:1: database.port: expected non-negative integer, found string"
            )
        );
    }

    #[test]
    fn loading_stops_at_the_first_broken_env_file() {
        let fixture = Fixture::new()
            .env_file("a.env", "BAD KEY=1\nok=1\nx$=2\n")
            .env_file("b.env", "B C=1\n");
        let a = fixture.dir.path().join("a.env").display().to_string();
        assert_eq!(
            fixture.err(),
            format!(
                "failed to load env file {a}:\n  \
                 - line 1: key cannot contain a space\n  \
                 - line 3: unexpected character in variable name"
            )
        );
    }

    #[test]
    fn an_env_file_listed_twice_reports_each_problem_once() {
        let fixture = Fixture::new()
            .env_file("a.env", "A=${NOPE}\n")
            .env_file("a.env", "A=${NOPE}\n");
        assert_eq!(fixture.ok().warnings.len(), 1);

        let fixture = Fixture::new()
            .env_file("a.env", "DBMCP_DB_PORT=abc\n")
            .env_file("b.env", "\n")
            .env_file("a.env", "DBMCP_DB_PORT=abc\n");
        assert_eq!(fixture.err().matches("database.port").count(), 1);
    }

    #[test]
    fn the_last_env_file_names_the_config_file() {
        let fixture = Fixture::new()
            .env_file("a.env", "DBMCP_CONFIG_FILE=a.yaml\n")
            .env_file("b.env", "DBMCP_CONFIG_FILE=b.yaml\n");
        fixture.write("a.yaml", "log_level: warn\n");
        fixture.write("b.yaml", "log_level: debug\n");
        assert_eq!(fixture.ok().log_level, LogLevel::Debug);

        let fixture = Fixture::new()
            .env_file("a.env", "DBMCP_CONFIG_FILE=a.yaml\n")
            .env_file("b.env", "DBMCP_CONFIG_FILE=\n");
        assert_eq!(fixture.ok().log_level, LogLevel::Info, "a later empty value clears it");
    }

    #[test]
    fn later_config_files_win_per_setting_and_sections_merge() {
        let loaded = Fixture::new()
            .config("a.yaml", "log_level: warn\ndatabase:\n  host: a\n  user: a\n")
            .config("b.yaml", "database:\n  host: b\n")
            .ok();
        assert_eq!(loaded.log_level, LogLevel::Warn);
        let db = &loaded.config.database;
        assert_eq!((db.host.as_str(), db.user.as_str()), ("b", "a"));
    }

    #[test]
    fn later_config_lists_replace_and_unset_values_keep_earlier_ones() {
        let http = Fixture::new()
            .config(
                "a.yaml",
                "transport: http\ndatabase:\n  port: 1111\nhttp:\n  allowed_hosts: [a, b]\n  allowed_origins: [\"http://x\"]\n",
            )
            .config("b.yaml", "database:\n  port:\nhttp:\n  allowed_hosts: [c]\n  allowed_origins: []\n")
            .ok();
        assert_eq!(http.config.database.port, 1111, "a later null is a no-op");
        let http = http.config.http.expect("http");
        assert_eq!(http.allowed_hosts, ["c"], "lists replace, not append");
        assert!(http.allowed_origins.is_empty(), "a later [] wins");
    }

    #[test]
    fn overridden_config_values_are_checked_in_the_declaring_file() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  port: abc\n")
            .config("b.yaml", "database:\n  port: 5432\n");
        let a = fixture.dir.path().join("a.yaml").display().to_string();
        assert_eq!(
            fixture.err(),
            format!("invalid settings:\n  - {a}:2:9: database.port: expected non-negative integer, found string")
        );
    }

    #[test]
    fn config_paths_resolve_against_the_declaring_file() {
        let fixture = Fixture::new()
            .config(
                "a.yaml",
                "database:\n  backend: sqlite\n  name: a.db\n  ssl_cert: cert.pem\n",
            )
            .config("sub/b.yaml", "database:\n  name: b.db\n  ssl_ca: ca.pem\n");
        let db = fixture.ok().config.database;
        let root = fixture.dir.path();
        assert_eq!(db.name, Some(root.join("sub").join("b.db").display().to_string()));
        assert_eq!(db.ssl_ca, Some(root.join("sub").join("ca.pem").display().to_string()));
        assert_eq!(db.ssl_cert, Some(root.join("cert.pem").display().to_string()));
    }

    #[test]
    fn loading_stops_at_the_first_broken_config_file() {
        let fixture = Fixture::new()
            .config("a.yaml", "database: [1, 2\n")
            .config("b.yaml", "database: [3\n");
        let error = fixture.err();
        assert!(error.starts_with(&format!(
            "failed to parse config file {}",
            fixture.dir.path().join("a.yaml").display()
        )));
        assert!(!error.contains("b.yaml"), "{error}");

        let fixture = Fixture::new()
            .config("a.yaml", "extra: 1\ndatabase:\n  port: abc\n")
            .config("b.yaml", "other: 1\n");
        let error = fixture.err();
        assert!(
            error.contains("a.yaml:1:1: extra: unknown field") && error.contains("database.port"),
            "{error}"
        );
        assert!(!error.contains("b.yaml"), "{error}");

        let error = Fixture::new()
            .env_file(".env", "BAD KEY=1\n")
            .config("a.yaml", "database: [1\n")
            .err();
        assert!(error.starts_with("failed to load env file"), "{error}");
    }

    #[test]
    fn a_duplicate_key_is_reported_once_without_library_hints() {
        let fixture = Fixture::new().config("a.yaml", "database:\n  port: 1\n  port: 2\n");
        let error = fixture.err();
        let a = fixture.dir.path().join("a.yaml").display().to_string();
        assert_eq!(
            error,
            format!("failed to parse config file {a}:\n  - {a}:3:3: duplicate mapping key `port` is not allowed here")
        );
    }

    #[test]
    fn anchors_do_not_cross_config_files() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  host: &h a\n")
            .config("b.yaml", "database:\n  user: *h\n");
        let error = fixture.err();
        let b = fixture.dir.path().join("b.yaml").display().to_string();
        assert!(
            error.starts_with(&format!("failed to parse config file {b}:\n  - {b}:2:")),
            "{error}"
        );
    }

    #[test]
    fn a_config_file_listed_twice_reports_each_problem_once() {
        let fixture = Fixture::new()
            .config("a.yaml", "database:\n  port: abc\n  charset: ${NOPE}\n")
            .config("a.yaml", "database:\n  port: abc\n  charset: ${NOPE}\n");
        let error = fixture.load().expect_err("bad port");
        assert_eq!(error.to_string().matches("database.port").count(), 1, "{error}");
        assert_eq!(error.warnings.len(), 1, "{:?}", error.warnings);
    }

    #[test]
    fn config_var_is_a_list_from_process_env_or_env_files() {
        let fixture = Fixture::new().env("DBMCP_CONFIG_FILE", "a.yaml , b.yaml");
        fixture.write("a.yaml", "log_level: warn\ntransport: http\n");
        fixture.write("b.yaml", "log_level: debug\n");
        let loaded = fixture.ok();
        assert_eq!(
            (loaded.log_level, loaded.config.transport),
            (LogLevel::Debug, Transport::Http)
        );

        let fixture = Fixture::new().env_file(".env", "DBMCP_CONFIG_FILE=a.yaml,b.yaml\n");
        fixture.write("a.yaml", "log_level: warn\n");
        fixture.write("b.yaml", "log_level: debug\n");
        assert_eq!(fixture.ok().log_level, LogLevel::Debug);
        let fixture = fixture.config("c.yaml", "log_level: error\n");
        assert_eq!(
            fixture.ok().log_level,
            LogLevel::Error,
            "--config-file replaces the list"
        );
    }

    #[test]
    fn empty_config_entries_are_rejected_before_reading() {
        assert_eq!(
            Fixture::new().env("DBMCP_CONFIG_FILE", "a.yaml,").err(),
            "DBMCP_CONFIG_FILE: empty entry in file list"
        );
        let fixture = Fixture::new().env_file("x.env", "# c\nDBMCP_CONFIG_FILE=a.yaml,,b.yaml\n");
        let env_file = fixture.dir.path().join("x.env").display().to_string();
        assert_eq!(
            fixture.err(),
            format!("DBMCP_CONFIG_FILE in {env_file}:2: empty entry in file list")
        );
        let mut fixture = Fixture::new();
        fixture.options.config_files = vec![PathBuf::new()];
        assert_eq!(fixture.err(), "--config-file: empty path");
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
        assert!(
            error.contains("extra: unknown field; expected one of transport, log_level, database, http, privacy, ssh")
        );
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
            "database:\n  backend: sqlite\n  name: data/app.db\n  ssl_ca: certs/ca.pem\nprivacy:\n  mode: full\n  model: ~/models\n",
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
            loaded.config.privacy.expect("privacy").model,
            Some(fixture.dir.path().join("home").join("models"))
        );
    }

    #[test]
    fn env_file_paths_resolve_against_the_env_file_directory() {
        let fixture = Fixture::new()
            .config("conf/dbmcp.yaml", "database:\n  backend: sqlite\n")
            .env_file("envs/.env", "DBMCP_DB_NAME=data/app.db\nDBMCP_DB_SSL_CA=certs/ca.pem\n");
        let loaded = fixture.ok();
        let envs = fixture.dir.path().join("envs");
        assert_eq!(
            loaded.config.database.name,
            Some(envs.join("data/app.db").display().to_string())
        );
        assert_eq!(
            loaded.config.database.ssl_ca,
            Some(envs.join("certs/ca.pem").display().to_string())
        );
    }

    #[test]
    fn config_file_flags_expand_tilde() {
        let mut fixture = Fixture::new();
        fixture.write("home/dbmcp.yaml", "database:\n  host: from-home\n");
        fixture.options.config_files.push(Path::new("~").join("dbmcp.yaml"));
        assert_eq!(fixture.ok().config.database.host, "from-home");
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
                "http:\n  allowed_origins: []\nprivacy:\n  mode: full\n  categories: [crypto, network]\n",
            )
            .ok();
        let http = loaded.config.http.expect("http");
        assert_eq!(http.allowed_hosts, ["a", "b"]);
        assert!(http.allowed_origins.is_empty(), "[] disables Origin validation");
        assert_eq!(
            loaded.config.privacy.expect("privacy").categories,
            Some(vec![PrivacyCategory::Crypto, PrivacyCategory::Network])
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
            .env("DBMCP_PRIVACY_MODE", "full")
            .env("DBMCP_PRIVACY_CATEGORIES", "crypto,bogus")
            .err();
        assert!(
            error.contains("DBMCP_HTTP_ALLOWED_HOSTS: http.allowed_hosts: empty item in comma-separated list"),
            "{error}"
        );
        assert!(
            error.contains("DBMCP_PRIVACY_CATEGORIES: privacy.categories: item 1: expected one of personal,"),
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

    const PRIVACY_KEY_32: &str = "0123456789abcdef0123456789abcdef";

    /// A fixture in `full` mode under `operator`.
    fn privacy_fixture(operator: &str) -> Fixture {
        Fixture::new()
            .env("DBMCP_PRIVACY_MODE", "full")
            .env("DBMCP_PRIVACY_OPERATOR", operator)
    }

    #[test]
    fn privacy_key_is_kept_only_for_the_hash_operator() {
        let privacy = privacy_fixture("hash")
            .env("DBMCP_PRIVACY_KEY", PRIVACY_KEY_32)
            .ok()
            .config
            .privacy
            .expect("privacy");
        assert_eq!(
            privacy.key.as_ref().map(ExposeSecret::expose_secret),
            Some(PRIVACY_KEY_32)
        );

        let privacy = privacy_fixture("replace")
            .env("DBMCP_PRIVACY_KEY", PRIVACY_KEY_32)
            .ok()
            .config
            .privacy
            .expect("privacy");
        assert!(privacy.key.is_none());
    }

    #[test]
    fn privacy_key_is_checked_in_every_mode_and_for_every_operator() {
        for fixture in [
            privacy_fixture("replace"),
            Fixture::new().env("DBMCP_PRIVACY_MODE", "rules"),
            Fixture::new().env("DBMCP_PRIVACY_MODE", "none"),
            Fixture::new(),
        ] {
            let error = fixture.env("DBMCP_PRIVACY_KEY", "short").err();
            assert!(
                error.contains("privacy.key must be at least 32 bytes, got 5"),
                "{error}"
            );
        }
    }

    #[test]
    fn privacy_defaults_to_hash_and_keeps_an_unset_key_unset() {
        let loaded = Fixture::new().config("dbmcp.yaml", "privacy:\n  mode: full\n").ok();
        let privacy = loaded.config.privacy.expect("privacy");
        assert_eq!(privacy.operator, PrivacyOperator::Hash);
        assert!(privacy.key.is_none());

        let loaded = Fixture::new()
            .config(
                "dbmcp.yaml",
                &format!("privacy:\n  mode: full\n  key: {PRIVACY_KEY_32}\n"),
            )
            .ok();
        assert!(loaded.config.privacy.expect("privacy").key.is_some());
    }

    #[test]
    fn privacy_key_rules() {
        let error = privacy_fixture("hash").env("DBMCP_PRIVACY_KEY", "s3krt").err();
        assert!(
            error.contains("privacy.key must be at least 32 bytes, got 5"),
            "{error}"
        );
        assert!(error.contains("DBMCP_PRIVACY_KEY"), "{error}");
        assert!(!error.contains("s3krt"), "key leaked: {error}");

        let error = privacy_fixture("hash").env("DBMCP_PRIVACY_KEY", &" ".repeat(32)).err();
        assert!(error.contains("privacy.key must not be empty when set"), "{error}");
    }

    #[test]
    fn privacy_key_env_overrides_yaml() {
        let env_key = "e".repeat(32);
        let loaded = privacy_fixture("hash")
            .config("dbmcp.yaml", &format!("privacy:\n  key: {}\n", "y".repeat(32)))
            .env("DBMCP_PRIVACY_KEY", &env_key)
            .ok();
        let privacy = loaded.config.privacy.expect("privacy");
        assert_eq!(
            privacy.key.as_ref().map(ExposeSecret::expose_secret),
            Some(env_key.as_str())
        );
        let debug = format!("{privacy:?}");
        assert!(!debug.contains(&env_key), "key leaked: {debug}");
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
            .env("DBMCP_CONFIG_FILE", "missing.yaml")
            .load()
            .expect_err("missing file");
        assert!(diagnostics(&error).is_empty());
    }

    #[test]
    fn unsupported_yaml_tags_are_rejected_where_they_sit() {
        let fixture = Fixture::new().config(
            "dbmcp.yaml",
            "transport: http\nhttp:\n  allowed_origins: !reset []\n  allowed_hosts: [a, !override b]\n  port: abc\nssh: !reset\n",
        );
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert_eq!(
            error,
            format!(
                "invalid settings:\n  \
                 - {file}:3:27: http.allowed_origins: YAML tag !reset is not supported\n  \
                 - {file}:4:18: http.allowed_hosts: item 1: YAML tag !override is not supported\n  \
                 - {file}:5:9: http.port: expected non-negative integer, found string\n  \
                 - {file}:6:12: ssh: YAML tag !reset is not supported"
            )
        );
    }

    #[test]
    fn core_yaml_tags_are_accepted() {
        let loaded = Fixture::new()
            .config("dbmcp.yaml", "database:\n  host: !!str 10\n  port: !!int 6000\n")
            .ok();
        assert_eq!(loaded.config.database.host, "10");
        assert_eq!(loaded.config.database.port, 6000);
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

    /// Loads the fixture and returns its privacy section.
    fn privacy_section(fixture: &Fixture) -> PrivacyConfig {
        fixture.ok().config.privacy.expect("privacy on")
    }

    /// Two rules on `MySQL` keys; `cards` sets its own operator.
    const RULES: &str = "privacy:
  mode: full
  rules:
    emails:
      columns:
        - def.Shop.Users.Email
        - def.shop.orders.email
      entity: EMAIL_ADDRESS
    cards:
      columns: [def.shop.payments.pan]
      entity: CREDIT_CARD
      operator: mask
";

    /// Renders rules as `(name, item, key, entity, operator)`.
    fn rule_rows(privacy: &PrivacyConfig) -> Vec<(String, usize, String, &'static str, PrivacyOperator)> {
        privacy
            .rules
            .iter()
            .map(|r| (r.name.clone(), r.item, r.key.join("."), r.entity, r.operator))
            .collect()
    }

    #[test]
    fn rules_flatten_one_column_rule_per_listed_column() {
        let privacy = privacy_section(&Fixture::new().config("dbmcp.yaml", RULES));
        assert_eq!(
            rule_rows(&privacy),
            vec![
                (
                    "emails".into(),
                    0,
                    "def.shop.users.email".into(),
                    "EMAIL_ADDRESS",
                    PrivacyOperator::Hash
                ),
                (
                    "emails".into(),
                    1,
                    "def.shop.orders.email".into(),
                    "EMAIL_ADDRESS",
                    PrivacyOperator::Hash
                ),
                (
                    "cards".into(),
                    0,
                    "def.shop.payments.pan".into(),
                    "CREDIT_CARD",
                    PrivacyOperator::Mask
                ),
            ]
        );
        let privacy = privacy_section(&Fixture::new().config("dbmcp.yaml", &format!("{RULES}  operator: redact\n")));
        assert_eq!(privacy.rules[0].operator, PrivacyOperator::Redact);
        let inherit = RULES.replace("      operator: mask\n", "      operator: ~\n");
        let privacy = privacy_section(&Fixture::new().config("dbmcp.yaml", &inherit));
        assert_eq!(privacy.rules[2].operator, PrivacyOperator::Hash);
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one table with a row per rule error")]
    fn rule_value_errors_are_located_in_the_declaring_file() {
        let rule = "      columns: [a.b.c.d]\n      entity: URL\n";
        let cases: Vec<(String, &str)> = vec![
            (
                format!("    Emails:\n{rule}"),
                "rule 0: rule name must use only a-z, 0-9, _ and -",
            ),
            (
                format!("    a b:\n{rule}"),
                "rule 0: rule name must use only a-z, 0-9, _ and -",
            ),
            (
                format!("    emails:\n{rule}    def.shop.users.secret:\n{rule}"),
                "rule 1: rule name must use only a-z, 0-9, _ and -",
            ),
            (
                format!("    '':\n{rule}"),
                "rule 0: rule name must use only a-z, 0-9, _ and -",
            ),
            ("    emails:\n".into(), "emails: expected mapping, found null"),
            (
                "    emails: [a.b.c.d]\n".into(),
                "emails: expected mapping, found sequence",
            ),
            (
                "    emails:\n      columns:\n      entity: URL\n".into(),
                "emails: columns: required",
            ),
            (
                "    emails:\n      columns: [a.b.c.d]\n      entity: ~\n".into(),
                "emails: entity: required",
            ),
            (
                "    emails:\n      columns: a.b.c.d\n      entity: URL\n".into(),
                "emails: columns: expected sequence, found string",
            ),
            (
                "    emails:\n      columns: [a.b.c.d, 1]\n      entity: URL\n".into(),
                "emails: columns: item 1: expected string, found integer",
            ),
            (
                "    emails:\n      columns: [a.b.c.d]\n      entity: [URL]\n".into(),
                "emails: entity: expected string, found sequence",
            ),
            ("    emails: 1\n".into(), "emails: expected mapping, found integer"),
            (
                format!("    emails:\n{rule}      allow: [join]\n"),
                "emails: allow: unknown field; expected one of columns, entity, operator",
            ),
            ("    emails:\n      entity: URL\n".into(), "emails: columns: required"),
            (
                "    emails:\n      columns: [a.b.c.d]\n".into(),
                "emails: entity: required",
            ),
            (
                "    emails:\n      columns: []\n      entity: URL\n".into(),
                "emails: columns: must not be empty",
            ),
            (
                "    emails:\n      columns: [a]\n      entity: URL\n".into(),
                "emails: columns: item 0: expected 2 to 4 dot-separated segments ([[catalog.]schema.]table.column), found 1",
            ),
            (
                "    emails:\n      columns: [a.b.c.d, a.b.c.d.e]\n      entity: URL\n".into(),
                "emails: columns: item 1: expected 2 to 4 dot-separated segments ([[catalog.]schema.]table.column), found 5",
            ),
            (
                "    emails:\n      columns: ['a..c.d']\n      entity: URL\n".into(),
                "emails: columns: item 0: empty segment",
            ),
            (
                "    emails:\n      columns: ['a.b.*.d']\n      entity: URL\n".into(),
                "emails: columns: item 0: * is not supported",
            ),
            (
                "    emails:\n      columns: ['a.b. c.d']\n      entity: URL\n".into(),
                "emails: columns: item 0: whitespace in a segment",
            ),
            (
                "    emails:\n      columns: ['a.`b`.c.d']\n      entity: URL\n".into(),
                "emails: columns: item 0: quotes are not supported; write the name as the catalog stores it",
            ),
            (
                format!("    emails:\n{rule}      operator: 1\n"),
                "emails: operator: expected string, found integer",
            ),
            (
                "    emails:\n      columns: [a.b.c.d]\n      entity: NOPE\n".into(),
                "emails: entity: unknown entity type; see the data types page",
            ),
            (
                format!("    emails:\n{rule}      operator: shred\n"),
                "emails: operator: expected one of replace, mask, redact, hash",
            ),
            (
                "    emails: !reset {}\n".into(),
                "emails: YAML tag !reset is not supported",
            ),
        ];
        for (body, reason) in cases {
            let fixture = Fixture::new().config("dbmcp.yaml", &format!("privacy:\n  rules:\n{body}"));
            let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
            assert_eq!(
                fixture.err(),
                format!("invalid settings:\n  - {file}:3:5: privacy.rules: {reason}"),
                "{body}"
            );
        }
    }

    #[test]
    fn rule_errors_never_echo_column_keys() {
        let error = Fixture::new()
            .config(
                "dbmcp.yaml",
                "privacy:\n  rules:\n    emails:\n      columns: [s3cr3t.b.c.d.e]\n      entity: URL\n",
            )
            .err();
        assert!(!error.contains("s3cr3t"), "key leaked: {error}");
        let error = Fixture::new()
            .config(
                "dbmcp.yaml",
                "privacy:\n  rules:\n    emails:\n      columns: [a.b.c.d]\n      entity: S3CR3T_ENTITY\n",
            )
            .err();
        assert!(!error.contains("S3CR3T_ENTITY"), "entity leaked: {error}");
    }

    #[test]
    fn a_duplicate_rule_name_is_a_parse_error() {
        let rule = "      columns: [a.b.c.d]\n      entity: URL\n";
        let error = Fixture::new()
            .config(
                "dbmcp.yaml",
                &format!("privacy:\n  rules:\n    emails:\n{rule}    emails:\n{rule}"),
            )
            .err();
        assert!(error.starts_with("failed to parse config file"), "{error}");
        assert!(
            error.contains(":6:5: duplicate mapping key `emails` is not allowed here"),
            "{error}"
        );
    }

    #[test]
    fn later_rules_replace_earlier_ones_whole() {
        let fixture = Fixture::new().config("a.yaml", RULES).config(
            "b.yaml",
            "privacy:\n  rules:\n    phones:\n      columns: [def.shop.users.phone]\n      entity: PHONE_NUMBER\n",
        );
        let names: Vec<String> = privacy_section(&fixture).rules.into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["phones"]);
    }

    #[test]
    fn null_rules_keep_earlier_ones_and_empty_clears() {
        for null in ["privacy:\n  rules:\n", "privacy:\n  rules: ~\n"] {
            let fixture = Fixture::new().config("a.yaml", RULES).config("b.yaml", null);
            assert_eq!(privacy_section(&fixture).rules.len(), 3, "{null:?}");
        }
        let fixture = Fixture::new()
            .config("a.yaml", RULES)
            .config("b.yaml", "privacy:\n  rules: {}\n");
        assert!(privacy_section(&fixture).rules.is_empty());
        assert!(fixture.ok().config.privacy.expect("privacy").rules.is_empty());
    }

    #[test]
    fn a_broken_rule_in_an_overridden_file_still_fails() {
        let fixture = Fixture::new()
            .config(
                "a.yaml",
                "privacy:\n  rules:\n    emails:\n      columns: [a]\n      entity: URL\n",
            )
            .config("b.yaml", "privacy:\n  rules: {}\n");
        let a = fixture.dir.path().join("a.yaml").display().to_string();
        assert_eq!(
            fixture.err(),
            format!(
                "invalid settings:\n  - {a}:3:5: privacy.rules: emails: columns: item 0: \
                 expected 2 to 4 dot-separated segments ([[catalog.]schema.]table.column), found 1"
            )
        );
    }

    #[test]
    fn privacy_rules_has_no_env_var() {
        let loaded = Fixture::new().env("DBMCP_PRIVACY_RULES", "garbage").ok();
        assert!(loaded.config.privacy.expect("privacy").rules.is_empty());
        let privacy = Fixture::new()
            .env_file(".env", "DBMCP_PRIVACY_RULES=garbage\n")
            .ok()
            .config
            .privacy
            .expect("privacy");
        assert!(privacy.rules.is_empty());
    }

    #[test]
    fn rules_are_ignored_under_none() {
        let yaml = RULES.replace("  mode: full\n", "  mode: none\n");
        let loaded = Fixture::new().config("dbmcp.yaml", &yaml).ok();
        assert!(loaded.config.privacy.is_none());
    }

    #[test]
    fn malformed_rules_error_under_none() {
        let rule = "      columns: [def.shop.users.email]\n      entity: URL\n";
        for (yaml, reason) in [
            (
                "privacy:\n  mode: none\n  rules:\n    emails:\n      columns: [main.s.t.c]\n      entity: URL\n"
                    .to_owned(),
                "catalog segment must be def on mysql",
            ),
            (
                format!("privacy:\n  mode: none\n  rules:\n    emails:\n{rule}    phones:\n{rule}"),
                "is also in rule emails",
            ),
        ] {
            let error = Fixture::new().config("dbmcp.yaml", &yaml).err();
            assert!(error.contains(reason), "{error}");
        }
    }

    #[test]
    fn a_short_key_without_database_name_errors_under_none() {
        let error = Fixture::new()
            .config(
                "dbmcp.yaml",
                "privacy:\n  mode: none\n  rules:\n    emails:\n      columns: [users.email]\n      entity: URL\n",
            )
            .err();
        assert!(
            error.contains("a key without its database requires database.name on mysql"),
            "{error}"
        );
    }

    #[test]
    fn rules_mode_accepts_no_rules() {
        for yaml in ["privacy:\n  mode: rules\n", "privacy:\n  mode: rules\n  rules: {}\n"] {
            let privacy = privacy_section(&Fixture::new().config("dbmcp.yaml", yaml));
            assert_eq!(privacy.mode, PrivacyMode::Rules);
            assert!(privacy.rules.is_empty());
        }
    }

    #[test]
    fn each_mode_resolves_its_section() {
        let model = "privacy:\n  model: models/ner\n  categories: [crypto]\n";
        let none = Fixture::new()
            .config("dbmcp.yaml", model)
            .env("DBMCP_PRIVACY_MODE", "none");
        assert!(none.ok().config.privacy.is_none());
        let empty = Fixture::new().config("dbmcp.yaml", model).env("DBMCP_PRIVACY_MODE", "");
        assert_eq!(
            privacy_section(&empty).mode,
            PrivacyMode::Full,
            "empty env value falls back to the default"
        );

        for (name, mode) in [("rules", PrivacyMode::Rules), ("full", PrivacyMode::Full)] {
            let fixture = Fixture::new()
                .config("dbmcp.yaml", model)
                .env("DBMCP_PRIVACY_MODE", name);
            let privacy = privacy_section(&fixture);
            assert_eq!(privacy.mode, mode);
            assert_eq!(privacy.operator, PrivacyOperator::Hash);
            assert_eq!(privacy.categories, Some(vec![PrivacyCategory::Crypto]));
            assert_eq!(privacy.model, Some(fixture.dir.path().join("models/ner")));
        }
    }

    #[test]
    fn unset_mode_resolves_to_full() {
        let privacy = privacy_section(&Fixture::new());
        assert_eq!(privacy.mode, PrivacyMode::Full);
        assert!(privacy.model.is_none() && privacy.rules.is_empty());
    }

    #[test]
    fn privacy_categories_are_checked_in_every_mode() {
        for mode in ["none", "rules", "full"] {
            let error = Fixture::new()
                .env("DBMCP_PRIVACY_MODE", mode)
                .config("dbmcp.yaml", "privacy:\n  categories: [crypto, crypto]\n")
                .err();
            assert!(
                error.contains("privacy.categories: duplicate entry 'crypto'"),
                "{mode}: {error}"
            );
            let error = Fixture::new()
                .env("DBMCP_PRIVACY_MODE", mode)
                .config("dbmcp.yaml", "privacy:\n  categories: []\n")
                .err();
            assert!(error.contains("privacy.categories"), "{mode}: {error}");
        }
    }

    #[test]
    fn old_pii_env_names_name_their_replacement() {
        for (old, new) in [
            ("DBMCP_PII_ENABLE", "privacy.mode: replaced by DBMCP_PRIVACY_MODE"),
            (
                "DBMCP_PII_OPERATOR",
                "privacy.operator: replaced by DBMCP_PRIVACY_OPERATOR",
            ),
            ("DBMCP_PII_KEY", "privacy.key: replaced by DBMCP_PRIVACY_KEY"),
            (
                "DBMCP_PII_CATEGORIES",
                "privacy.categories: replaced by DBMCP_PRIVACY_CATEGORIES",
            ),
            ("DBMCP_PII_NER_ENABLE", "privacy.model: replaced by DBMCP_PRIVACY_MODEL"),
            ("DBMCP_PII_NER_MODEL", "privacy.model: replaced by DBMCP_PRIVACY_MODEL"),
        ] {
            let error = Fixture::new().env(old, "secret-value").err();
            assert!(error.contains(&format!("{old}: {new}")), "{error}");
            assert!(!error.contains("secret-value"), "{error}");
            let error = Fixture::new().env_file(".env", &format!("{old}=secret-value\n")).err();
            assert!(error.contains(&format!("{old} in ")) && error.contains(new), "{error}");
        }
        Fixture::new().env("DBMCP_PII_KEY", "").ok();
    }

    #[test]
    fn unknown_mode_lists_the_choices() {
        let error = Fixture::new().env("DBMCP_PRIVACY_MODE", "off").err();
        assert!(
            error.contains("DBMCP_PRIVACY_MODE: privacy.mode: expected one of none, rules, full"),
            "{error}"
        );
    }

    #[test]
    fn old_pii_section_is_an_unknown_field() {
        let error = Fixture::new().config("dbmcp.yaml", "pii:\n  enabled: true\n").err();
        assert!(
            error.contains("pii: unknown field; expected one of transport, log_level, database, http, privacy, ssh"),
            "{error}"
        );
    }

    #[test]
    fn privacy_key_is_kept_when_a_rule_hashes() {
        let hashing = "  rules:\n    emails:\n      columns: [def.shop.users.email]\n      entity: EMAIL_ADDRESS\n";
        let fixture = privacy_fixture("mask")
            .env("DBMCP_PRIVACY_KEY", PRIVACY_KEY_32)
            .config("dbmcp.yaml", &format!("privacy:\n{hashing}      operator: hash\n"));
        let privacy = privacy_section(&fixture);
        assert_eq!(
            privacy.key.as_ref().map(ExposeSecret::expose_secret),
            Some(PRIVACY_KEY_32)
        );

        let fixture = privacy_fixture("mask")
            .env("DBMCP_PRIVACY_KEY", PRIVACY_KEY_32)
            .config("dbmcp.yaml", &format!("privacy:\n{hashing}"));
        assert!(privacy_section(&fixture).key.is_none());
    }

    #[test]
    fn column_rules_load_expanded_on_every_backend() {
        let cases = [
            ("mysql", "app", "def.app.users.email", "def.app.users.email"),
            ("mariadb", "app", "def.app.users.email", "def.app.users.email"),
            ("mysql", "shop", "users.email", "def.shop.users.email"),
            ("sqlite", "app.db", "main.main.users.email", "main.main.users.email"),
            ("postgres", "app", "app.public.users.email", "app.public.users.email"),
        ];
        for (backend, name, key, expanded) in cases {
            let fixture = Fixture::new()
                .env("DBMCP_DB_BACKEND", backend)
                .env("DBMCP_DB_NAME", name)
                .env("DBMCP_PRIVACY_KEY", PRIVACY_KEY_32)
                .config(
                    "dbmcp.yaml",
                    &format!("privacy:\n  mode: full\n  rules:\n    emails:\n      columns: [{key}]\n      entity: EMAIL_ADDRESS\n"),
                );
            let keys: Vec<String> = privacy_section(&fixture)
                .rules
                .iter()
                .map(|r| r.key.join("."))
                .collect();
            assert_eq!(keys, [expanded], "{backend} {key}");
        }
    }

    #[test]
    fn a_short_key_without_database_name_names_where_to_set_it() {
        let fixture = Fixture::new().env("DBMCP_DB_BACKEND", "postgres").config(
            "dbmcp.yaml",
            "privacy:\n  mode: full\n  rules:\n    emails:\n      columns: [users.email]\n      entity: URL\n",
        );
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        let messages: Vec<String> = diagnostics(&fixture.load().expect_err("needs database.name"))
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            messages,
            [format!(
                "privacy.rules: emails: columns: item 0: a key without its database requires database.name on postgres; \
                 set DBMCP_DB_NAME or database.name in the config file \
                 (privacy.rules from {file}:4:5; database.backend from DBMCP_DB_BACKEND)"
            )]
        );
    }

    #[test]
    fn rule_key_errors_show_under_none() {
        let fixture = Fixture::new()
            .env("DBMCP_DB_BACKEND", "sqlite")
            .env("DBMCP_DB_NAME", "app.db")
            .config(
                "dbmcp.yaml",
                "privacy:\n  rules:\n    emails:\n      columns: [def.main.users.email]\n      entity: URL\n",
            );
        let messages: Vec<String> = diagnostics(&fixture.load().expect_err("malformed rules"))
            .iter()
            .map(|d| d.error.to_string())
            .collect();
        assert_eq!(
            messages,
            ["privacy.rules: emails: columns: item 0: catalog segment must be main on sqlite"]
        );
    }

    #[test]
    fn trimmed_magento_rules_load() {
        let fixture = Fixture::new().config(
            "dbmcp.yaml",
            "privacy:
  mode: full
  operator: hash
  key: prototype-magento-key-0123456789abcdef
  rules:
    emails:
      columns:
        - def.magento.customer_entity.email
        - def.magento.sales_order.customer_email
      entity: EMAIL_ADDRESS
    names:
      columns:
        - def.magento.customer_entity.firstname
        - def.magento.customer_entity.lastname
      entity: PERSON
      operator: replace
    phones:
      columns: [def.magento.sales_order_address.telephone]
      entity: PHONE_NUMBER
      operator: mask
",
        );
        assert_eq!(privacy_section(&fixture).rules.len(), 5);
        assert_eq!(fixture.ok().config.privacy.expect("privacy on").rules.len(), 5);
    }
}
