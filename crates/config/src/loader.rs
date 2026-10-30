//! Builds a [`Config`] from the process env, an env file, and a YAML file.
//!
//! Stages run in order and the first failing
//! stage aborts; each stage collects all of its own errors:
//!
//! 1. Read and parse the env file (`--env` > `DBMCP_ENV_FILE`).
//! 2. Pick the config file (`--config` > process `DBMCP_CONFIG` > env-file
//!    `DBMCP_CONFIG`), read it, and parse it into an untyped tree.
//! 3. `${}`-interpolate every string value against process env, then env file.
//! 4. Check keys, then resolve each setting: non-empty process env >
//!    non-empty env file > YAML > default, casting text by setting type.
//! 5. Run every section's `validate()` on the result.

use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::database::{DatabaseBackend, DatabaseConfig};
use crate::dotenv::{self, EnvFile};
use crate::env::{self, CONFIG_VAR, ENV_FILE_VAR, Kind, SETTINGS, Setting};
use crate::error::{ConfigError, Diagnostic};
use crate::http::HttpConfig;
use crate::interpolate;
use crate::log::LogLevel;
use crate::node::{self, Node, SpannedNode};
use crate::options::LoadOptions;
use crate::pii::{PiiCategory, PiiConfig, PiiOperator};
use crate::source::Source;
use crate::ssh::{self, SshConfig, StrictHostKeyChecking};
use crate::transport::Transport;

/// YAML sections; every other top-level key is a scalar setting.
const SECTIONS: [&str; 4] = ["database", "http", "pii", "ssh"];

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

    #[error("configuration validation failed:{}", Bullets(.0))]
    Invalid(Vec<Diagnostic>),
}

impl LoadError {
    /// Returns the cast and validation problems, if that stage failed.
    ///
    /// Empty when loading stopped earlier (reading, parsing, interpolation).
    #[must_use]
    pub fn diagnostics(&self) -> &[Diagnostic] {
        match &self.kind {
            LoadErrorKind::Invalid(diagnostics) => diagnostics,
            _ => &[],
        }
    }
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
/// YAML cannot be parsed, `${}` interpolation fails, a value has the wrong
/// type, or validation fails.
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
        let outcome = interpolate::interpolate(tree, &lookup);
        warnings.extend(outcome.warnings.iter().map(|w| format!("{}:{w}", path.display())));
        if !outcome.errors.is_empty() {
            let errors = outcome
                .errors
                .iter()
                .map(|e| format!("{}:{e}", path.display()))
                .collect();
            return Err(fail(
                warnings,
                LoadErrorKind::Interpolate {
                    path: path.clone(),
                    errors,
                },
            ));
        }
    }
    let leaves = match &file {
        Some((path, tree)) => match flatten(tree) {
            Ok(leaves) => leaves,
            Err(errors) => {
                let errors = errors.into_iter().map(|e| e.render(path)).collect();
                return Err(fail(
                    warnings,
                    LoadErrorKind::Parse {
                        path: path.clone(),
                        errors,
                    },
                ));
            }
        },
        None => HashMap::new(),
    };

    let mut resolver = Resolver {
        options,
        env_file: env_file.as_ref(),
        file: file.as_ref().map(|(path, _)| path.as_path()),
        leaves,
        sources: HashMap::new(),
        diagnostics: Vec::new(),
    };
    match resolver.build() {
        Ok((config, log_level)) => Ok(Loaded {
            config,
            log_level,
            warnings,
        }),
        Err(mut diagnostics) => {
            for diagnostic in &mut diagnostics {
                diagnostic.attach_sources(&resolver.sources);
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

/// A key-structure error in the YAML file.
struct KeyError {
    line: u64,
    column: u64,
    path: String,
    reason: String,
}

impl KeyError {
    fn new(node: &SpannedNode, path: String, reason: String) -> Self {
        Self {
            line: node.referenced.line(),
            column: node.referenced.column(),
            path,
            reason,
        }
    }

    fn render(&self, file: &Path) -> String {
        format!(
            "{}:{}:{}: {}: {}",
            file.display(),
            self.line,
            self.column,
            self.path,
            self.reason
        )
    }
}

/// Checks the tree's keys and returns every non-null setting by path.
fn flatten(root: &SpannedNode) -> Result<HashMap<&'static str, &SpannedNode>, Vec<KeyError>> {
    let mut leaves = HashMap::new();
    let mut errors = Vec::new();
    let top = match &root.value {
        Node::Null => return Ok(leaves),
        Node::Map(map) => map,
        other => {
            let reason = format!("expected mapping, found {}", other.kind());
            return Err(vec![KeyError::new(root, "<root>".into(), reason)]);
        }
    };
    for (key, value) in top {
        if SECTIONS.contains(&key.as_str()) {
            match &value.value {
                Node::Null => {}
                Node::Map(section) => {
                    for (field, leaf) in section {
                        let path = format!("{key}.{field}");
                        collect_leaf(&mut leaves, &mut errors, &path, leaf, Some(key));
                    }
                }
                other => {
                    let reason = format!("expected mapping, found {}", other.kind());
                    errors.push(KeyError::new(value, key.clone(), reason));
                }
            }
        } else {
            collect_leaf(&mut leaves, &mut errors, key, value, None);
        }
    }
    if errors.is_empty() { Ok(leaves) } else { Err(errors) }
}

fn collect_leaf<'a>(
    leaves: &mut HashMap<&'static str, &'a SpannedNode>,
    errors: &mut Vec<KeyError>,
    path: &str,
    node: &'a SpannedNode,
    section: Option<&str>,
) {
    let Some(setting) = SETTINGS.iter().find(|s| s.path == path) else {
        errors.push(KeyError::new(node, path.to_owned(), unknown_field(section)));
        return;
    };
    if node.value != Node::Null {
        leaves.insert(setting.path, node);
    }
}

/// Lists the keys accepted at the level an unknown key was found.
fn unknown_field(section: Option<&str>) -> String {
    let expected: Vec<&str> = match section {
        Some(section) => SETTINGS
            .iter()
            .filter_map(|s| s.path.strip_prefix(section)?.strip_prefix('.'))
            .collect(),
        None => SETTINGS
            .iter()
            .filter(|s| !s.path.contains('.'))
            .map(|s| s.path)
            .chain(SECTIONS)
            .collect(),
    };
    format!("unknown field; expected one of {}", expected.join(", "))
}

/// A setting's raw value before casting.
enum Raw<'a> {
    /// Text from the process env or env file.
    Text(&'a str),
    /// A YAML value.
    Node(&'a Node),
}

impl Raw<'_> {
    fn kind(&self) -> &'static str {
        match self {
            Self::Text(_) => "string",
            Self::Node(node) => node.kind(),
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            Self::Text(text) => Some(text),
            Self::Node(Node::Str(text)) => Some(text),
            Self::Node(_) => None,
        }
    }
}

/// Resolves each setting across the sources and casts it to its type.
struct Resolver<'a> {
    options: &'a LoadOptions,
    env_file: Option<&'a EnvFile>,
    file: Option<&'a Path>,
    leaves: HashMap<&'static str, &'a SpannedNode>,
    sources: HashMap<&'static str, Source>,
    /// Cast errors collected while resolving settings.
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Resolver<'a> {
    /// Returns the winning raw value for `setting` and records its source.
    fn raw(&mut self, setting: Setting) -> Option<Raw<'a>> {
        let Setting { path, env, .. } = setting;
        let options = self.options;
        let (raw, source) = if let Some(value) = non_empty(&options.environment, env) {
            (Raw::Text(value), Source::Env(env))
        } else if let Some((value, line)) = self
            .env_file
            .and_then(|file| file.get_with_line(env))
            .filter(|(value, _)| !value.is_empty())
        {
            let path = self.env_file.map(|f| f.path().to_path_buf()).unwrap_or_default();
            (Raw::Text(value), Source::EnvFile { name: env, path, line })
        } else {
            let node = *self.leaves.get(path)?;
            let source = Source::File {
                path: self.file.map(Path::to_path_buf).unwrap_or_default(),
                line: node.referenced.line(),
                column: node.referenced.column(),
            };
            (Raw::Node(&node.value), source)
        };
        self.sources.insert(path, source);
        Some(raw)
    }

    /// Returns `true` when any source sets `setting`, recording that source.
    fn is_set(&mut self, setting: Setting) -> bool {
        self.raw(setting).is_some()
    }

    /// Resolves `setting` and casts it; records a value-free error on failure.
    fn get<T>(&mut self, setting: Setting, cast: impl FnOnce(&Raw<'a>) -> Result<T, String>) -> Option<T> {
        let raw = self.raw(setting)?;
        match cast(&raw) {
            Ok(value) => Some(value),
            Err(reason) => {
                self.invalid(setting, reason);
                None
            }
        }
    }

    fn string(&mut self, setting: Setting) -> Option<String> {
        self.get(setting, |raw| {
            raw.as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("expected string, found {}", raw.kind()))
        })
    }

    fn bool(&mut self, setting: Setting) -> Option<bool> {
        self.get(setting, |raw| match raw {
            Raw::Node(Node::Bool(value)) => Ok(*value),
            raw => raw
                .as_str()
                .and_then(parse_bool)
                .ok_or_else(|| format!("expected boolean (true/false/1/0/t/f), found {}", raw.kind())),
        })
    }

    /// Resolves an integer and checks it against the setting's range.
    ///
    /// # Panics
    ///
    /// Panics when `setting` is not a [`Kind::Int`] or its `max` does not fit
    /// `T`: both are mistakes in the setting table, hit by every load.
    fn int<T: TryFrom<u64>>(&mut self, setting: Setting) -> Option<T> {
        let Kind::Int { min, max } = setting.kind else {
            unreachable!("{} is not an integer setting", setting.path);
        };
        assert!(T::try_from(max).is_ok(), "{} max does not fit its field", setting.path);
        let value = self.get(setting, |raw| {
            let parsed = match raw {
                Raw::Node(Node::Int(value)) => u64::try_from(*value).ok(),
                Raw::Node(Node::UInt(value)) => Some(*value),
                raw => raw.as_str().and_then(|text| text.parse().ok()),
            };
            parsed.ok_or_else(|| format!("expected non-negative integer, found {}", raw.kind()))
        })?;
        let expected = if value < min {
            format!("at least {min}")
        } else if value > max {
            format!("at most {max}")
        } else {
            return T::try_from(value).ok();
        };
        let path = setting.path;
        self.diagnostics
            .push(Diagnostic::new(ConfigError::OutOfRange { path, expected }).involving(&[setting]));
        None
    }

    /// Resolves an enum setting by matching its text against `choices`.
    fn enumeration<T: Copy>(&mut self, setting: Setting, choices: &[(&str, T)]) -> Option<T> {
        self.get(setting, |raw| {
            raw.as_str()
                .and_then(|text| choices.iter().find(|(name, _)| *name == text))
                .map(|&(_, value)| value)
                .ok_or_else(|| {
                    let names: Vec<&str> = choices.iter().map(|(name, _)| *name).collect();
                    format!("expected one of {}", names.join(", "))
                })
        })
    }

    /// Resolves a list: comma-separated text, or a YAML sequence of strings.
    fn list(&mut self, setting: Setting) -> Option<Vec<String>> {
        self.get(setting, |raw| match raw {
            Raw::Text(text) => {
                let items: Vec<String> = text.split(',').map(|item| item.trim().to_owned()).collect();
                if items.iter().any(String::is_empty) {
                    return Err("empty item in comma-separated list".into());
                }
                Ok(items)
            }
            Raw::Node(Node::Seq(items)) => items
                .iter()
                .enumerate()
                .map(|(i, item)| match &item.value {
                    Node::Str(text) => Ok(text.clone()),
                    other => Err(format!("item {i}: expected string, found {}", other.kind())),
                })
                .collect(),
            raw @ Raw::Node(_) => Err(format!("expected sequence, found {}", raw.kind())),
        })
    }

    /// Resolves a list setting whose items each match one of `choices`.
    fn enum_list<T: Copy>(&mut self, setting: Setting, choices: &[(&str, T)]) -> Option<Vec<T>> {
        let items = self.list(setting)?;
        let parsed: Option<Vec<T>> = items
            .iter()
            .map(|item| choices.iter().find(|(name, _)| name == item).map(|&(_, value)| value))
            .collect();
        if parsed.is_none() {
            let names: Vec<&str> = choices.iter().map(|(name, _)| *name).collect();
            self.invalid(setting, format!("expected items from {}", names.join(", ")));
        }
        parsed
    }

    /// Resolves a path setting by its [`Kind::Path`] rule.
    ///
    /// # Panics
    ///
    /// Panics when `setting` is not a [`Kind::Path`], a mistake in the setting table.
    fn path(&mut self, setting: Setting) -> Option<PathBuf> {
        let Kind::Path { home_from_env } = setting.kind else {
            unreachable!("{} is not a path setting", setting.path);
        };
        let value = PathBuf::from(self.string(setting)?);
        let home = self.options.home.as_deref();
        Some(match self.sources.get(setting.path) {
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

    /// Records a cast error for `setting`; its source is attached on failure.
    fn invalid(&mut self, setting: Setting, reason: String) {
        let path = setting.path;
        self.diagnostics
            .push(Diagnostic::new(ConfigError::InvalidValue { path, reason }).involving(&[setting]));
    }

    /// Resolves every setting into a [`Config`] and runs `validate()`.
    ///
    /// Cast errors stop before validation, since rules on half-cast values
    /// would only add noise.
    fn build(&mut self) -> Result<(Config, LogLevel), Vec<Diagnostic>> {
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
        let ssh = if ssh_set.contains(&env::SSH_HOST) {
            Some(Ok(self.ssh()))
        } else if ssh_set.is_empty() {
            None
        } else {
            Some(Err(Diagnostic::new(ConfigError::MissingSshHost)
                .involving(&ssh_set)
                .missing(&[env::SSH_HOST])))
        };
        if !self.diagnostics.is_empty() {
            return Err(std::mem::take(&mut self.diagnostics));
        }

        let mut diagnostics = Vec::new();
        diagnostics.extend(database.validate().err().into_iter().flatten());
        if let Some(http) = &http {
            diagnostics.extend(http.validate().err().into_iter().flatten());
        }
        if let Some(pii) = &pii {
            diagnostics.extend(pii.validate().err().into_iter().flatten());
        }
        if !ssh_set.is_empty() && database.backend == DatabaseBackend::Sqlite {
            diagnostics
                .push(Diagnostic::new(ConfigError::SshSqliteUnsupported).involving(&[env::SSH_HOST, env::DB_BACKEND]));
        }
        let ssh = match ssh {
            Some(Ok(ssh)) => {
                diagnostics.extend(ssh.validate().err().into_iter().flatten());
                Some(ssh)
            }
            Some(Err(diagnostic)) => {
                diagnostics.push(diagnostic);
                None
            }
            None => None,
        };
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        Ok((
            Config {
                transport,
                database,
                http,
                pii,
                ssh,
            },
            log_level,
        ))
    }

    fn database(&mut self) -> DatabaseConfig {
        let backend = self
            .enumeration(env::DB_BACKEND, DatabaseBackend::CHOICES)
            .unwrap_or(DatabaseConfig::DEFAULT_BACKEND);
        let name = self
            .string(env::DB_NAME)
            .map(|name| match self.sources.get(env::DB_NAME.path) {
                Some(Source::File { .. })
                    if backend == DatabaseBackend::Sqlite && !name.is_empty() && name != ":memory:" =>
                {
                    let home = self.options.home.as_deref();
                    let expanded = ssh::expand_tilde(Path::new(&name), home);
                    self.relative_to_file(&expanded).display().to_string()
                }
                _ => name,
            });
        let path_string = |r: &mut Self, setting| r.path(setting).map(|p| p.display().to_string());
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
            ssl_ca: path_string(self, env::DB_SSL_CA),
            ssl_cert: path_string(self, env::DB_SSL_CERT),
            ssl_key: path_string(self, env::DB_SSL_KEY),
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

    fn http(&mut self) -> HttpConfig {
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

    fn pii(&mut self) -> PiiConfig {
        PiiConfig {
            operator: self
                .enumeration(env::PII_OPERATOR, PiiOperator::CHOICES)
                .unwrap_or(PiiConfig::DEFAULT_OPERATOR),
            categories: self.enum_list(env::PII_CATEGORIES, PiiCategory::CHOICES),
            ner_enabled: self.bool(env::PII_NER_ENABLE).unwrap_or(PiiConfig::DEFAULT_NER_ENABLED),
            ner_model: self.path(env::PII_NER_MODEL),
        }
    }

    fn ssh(&mut self) -> SshConfig {
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

/// Parses a boolean the way Go's `strconv.ParseBool` does.
fn parse_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use secrecy::ExposeSecret;
    use tempfile::TempDir;

    use super::*;

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
            error.starts_with(&format!("failed to parse config file {}:", file.display())),
            "{error}"
        );
        assert!(
            error.contains(&format!(
                "{}:2:12: database.pasword: unknown field; expected one of backend,",
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
    fn type_errors_name_path_and_source_without_the_value() {
        let fixture = Fixture::new()
            .config("dbmcp.yaml", "database:\n  port: \"abc-secret\"\n  ssl: yes\n")
            .env("DBMCP_DB_READ_ONLY", "on");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert!(error.starts_with("configuration validation failed:"), "{error}");
        assert!(error.contains(&format!(
            "database.port: expected non-negative integer, found string (from {file}:2:9)"
        )));
        assert!(error.contains(&format!(
            "database.ssl: expected boolean (true/false/1/0/t/f), found string (from {file}:3:8)"
        )));
        assert!(error.contains(
            "database.read_only: expected boolean (true/false/1/0/t/f), found string (from DBMCP_DB_READ_ONLY)"
        ));
        assert!(!error.contains("abc-secret"));
    }

    #[test]
    fn bools_accept_the_parse_bool_set() {
        for (value, expected) in [
            ("1", true),
            ("t", true),
            ("TRUE", true),
            ("0", false),
            ("F", false),
            ("False", false),
        ] {
            let loaded = Fixture::new().env("DBMCP_DB_READ_ONLY", value).ok();
            assert_eq!(loaded.config.database.read_only, expected, "{value}");
        }
        let loaded = Fixture::new()
            .config("dbmcp.yaml", "database:\n  read_only: \"0\"\n")
            .ok();
        assert!(!loaded.config.database.read_only);
    }

    #[test]
    fn enums_reject_unknown_values() {
        let error = Fixture::new().env("DBMCP_TRANSPORT", "sse").err();
        assert!(
            error.contains("transport: expected one of stdio, http (from DBMCP_TRANSPORT)"),
            "{error}"
        );
    }

    #[test]
    fn validation_errors_carry_the_source() {
        let fixture = Fixture::new().config("dbmcp.yaml", "database:\n  page_size: 0\n");
        let error = fixture.err();
        let file = fixture.dir.path().join("dbmcp.yaml").display().to_string();
        assert!(
            error.contains(&format!("database.page_size must be at least 1 (from {file}:2:14)")),
            "{error}"
        );
    }

    #[test]
    fn integer_ranges_come_from_the_setting_table() {
        let error = Fixture::new()
            .env("DBMCP_DB_PAGE_SIZE", "501")
            .env("DBMCP_DB_PORT", "70000")
            .env("DBMCP_DB_QUERY_TIMEOUT", "-1")
            .err();
        assert!(
            error.contains("database.page_size must be at most 500 (from DBMCP_DB_PAGE_SIZE)"),
            "{error}"
        );
        assert!(
            error.contains("database.port must be at most 65535 (from DBMCP_DB_PORT)"),
            "{error}"
        );
        assert!(
            error.contains("database.query_timeout: expected non-negative integer, found string"),
            "{error}"
        );
        let loaded = Fixture::new().env("DBMCP_DB_PAGE_SIZE", "500").ok();
        assert_eq!(loaded.config.database.page_size, 500);
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
    fn list_errors() {
        let error = Fixture::new()
            .env("DBMCP_TRANSPORT", "http")
            .env("DBMCP_HTTP_ALLOWED_HOSTS", "a,,b")
            .env("DBMCP_PII_ENABLE", "true")
            .env("DBMCP_PII_CATEGORIES", "crypto,bogus")
            .err();
        assert!(
            error.contains("http.allowed_hosts: empty item in comma-separated list"),
            "{error}"
        );
        assert!(
            error.contains("pii.categories: expected items from personal,"),
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
            .config("dbmcp.yaml", "http:\n  host: \"\"\n  port: nope\n")
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
            error.contains(&format!("(from DBMCP_DB_PAGE_SIZE in {file}:2)")),
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
        let [diagnostic] = error.diagnostics() else {
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
    fn cast_errors_stop_before_validation() {
        let error = Fixture::new()
            .env("DBMCP_DB_PORT", "x")
            .env("DBMCP_DB_BACKEND", "sqlite")
            .load()
            .expect_err("bad port");
        let [diagnostic] = error.diagnostics() else {
            panic!("only the cast error: {error}");
        };
        assert!(matches!(
            &diagnostic.error,
            ConfigError::InvalidValue {
                path: "database.port",
                ..
            }
        ));
    }

    #[test]
    fn earlier_stages_have_no_diagnostics() {
        let error = Fixture::new()
            .env("DBMCP_CONFIG", "missing.yaml")
            .load()
            .expect_err("missing file");
        assert!(error.diagnostics().is_empty());
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
