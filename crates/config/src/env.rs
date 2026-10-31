//! The setting table: each setting's YAML key, env name, and value kind.
//!
//! Every setting is one constant here, and the loader reads it through that
//! constant, so a misspelt setting is a compile error. Env names are
//! hand-picked, not derived from key paths. Every leaf setting has exactly
//! one name and every name maps to exactly one setting. The [`Kind`] carries
//! every rule a value must meet: integer ranges, enum choices, and path
//! rules are stated here and nowhere else, and [`Kind::check`] applies them
//! to values from every source.

use std::path::{Path, PathBuf};

use crate::database::DatabaseConfig;
use crate::node::Node;

/// Env var naming the YAML config file; a loader input, not a setting.
pub const CONFIG_VAR: &str = "DBMCP_CONFIG";

/// Env var naming the dotenv file; a loader input, not a setting.
pub const ENV_FILE_VAR: &str = "DBMCP_ENV_FILE";

/// One setting: its dotted YAML key path, env var name, and value kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Setting {
    /// Dotted YAML key path, e.g. `database.host`.
    pub path: &'static str,
    /// Env var name, e.g. `DBMCP_DB_HOST`.
    pub env: &'static str,
    /// How the value is cast and checked.
    pub kind: Kind,
}

/// The type of a setting's value, with the rules the loader applies when casting it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A string.
    Text,
    /// A boolean (`ParseBool` set for text).
    Bool,
    /// A decimal integer in `min..=max`.
    Int {
        /// Smallest accepted value.
        min: u64,
        /// Largest accepted value.
        max: u64,
    },
    /// A kebab-case enum value, one of `choices`.
    Enum {
        /// Accepted names, in the order error messages list them.
        choices: &'static [&'static str],
    },
    /// A YAML sequence, or comma-separated text.
    List {
        /// Accepted item names, when items are enum values.
        choices: Option<&'static [&'static str]>,
    },
    /// A filesystem path.
    ///
    /// Values from the config file expand `~` and resolve against its
    /// directory. Values from env vars stay as given unless `home_from_env`
    /// is set, in which case a leading `~` expands too.
    Path {
        /// Expand `~` in env and env-file values as well.
        home_from_env: bool,
    },
}

/// A raw setting value: env or env-file text, or a YAML node.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Raw<'a> {
    /// Text from the process env or env file.
    Text(&'a str),
    /// A YAML value; strings are cast like env text.
    Node {
        /// The value.
        node: &'a Node,
        /// The config file's directory, which relative paths resolve against.
        dir: &'a Path,
    },
}

/// A setting value that passed its [`Kind`]'s checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Value {
    /// A [`Kind::Text`] value.
    Text(String),
    /// A [`Kind::Path`] value, resolved by where it was found.
    Path(PathBuf),
    /// A [`Kind::Bool`] value.
    Bool(bool),
    /// A [`Kind::Int`] value, within its range.
    Int(u64),
    /// A [`Kind::Enum`] value, one of its choices.
    Choice(&'static str),
    /// A [`Kind::List`] value, each item one of its choices if any.
    List(Vec<String>),
}

impl Kind {
    /// Casts `raw` to this kind and checks it against the kind's rules.
    ///
    /// Returns `Ok(None)` for an unset value: YAML `null`, or empty text at
    /// an integer, boolean, or list. YAML strings are cast like env text;
    /// list text is split on commas. Paths are resolved by their
    /// [`Kind::Path`] rule, expanding `~` against `home`.
    ///
    /// # Errors
    ///
    /// Returns a value-free reason when `raw` has the wrong type, is out of
    /// range, or is not an accepted choice.
    pub(crate) fn check(self, raw: Raw<'_>, home: Option<&Path>) -> Result<Option<Value>, String> {
        let node = match raw {
            Raw::Node { node, .. } => Some(node),
            Raw::Text(_) => None,
        };
        let text = match (raw, node) {
            (_, Some(Node::Null)) => return Ok(None),
            (Raw::Text(text), _) => Some(text),
            (_, Some(Node::Str(text))) => Some(text.as_str()),
            _ => None,
        };
        if text == Some("") && matches!(self, Self::Int { .. } | Self::Bool | Self::List { .. }) {
            return Ok(None);
        }
        let found = || node.map_or("string", Node::kind);
        let value = match self {
            Self::Text => Value::Text(
                text.ok_or_else(|| format!("expected string, found {}", found()))?
                    .to_owned(),
            ),
            Self::Path { home_from_env } => {
                let path = Path::new(text.ok_or_else(|| format!("expected string, found {}", found()))?);
                Value::Path(match raw {
                    // An empty value stays empty so the section's checks still see it.
                    _ if path.as_os_str().is_empty() => PathBuf::new(),
                    Raw::Node { dir, .. } => resolve_in_file(path, dir, home),
                    Raw::Text(_) if home_from_env => expand_tilde(path, home),
                    Raw::Text(_) => path.to_path_buf(),
                })
            }
            Self::Bool => match (node, text) {
                (Some(Node::Bool(value)), _) => Value::Bool(*value),
                (_, Some(text)) => {
                    Value::Bool(parse_bool(text).ok_or_else(|| format!("{EXPECTED_BOOL}, found {}", found()))?)
                }
                _ => return Err(format!("{EXPECTED_BOOL}, found {}", found())),
            },
            Self::Int { min, max } => {
                let value = match (node, text) {
                    (Some(Node::Int(value)), _) => {
                        u64::try_from(*value).map_err(|_| format!("must be at least {min}"))?
                    }
                    (Some(Node::UInt(value)), _) => *value,
                    (_, Some(text)) => text.parse().map_err(|_| format!("{EXPECTED_INT}, found {}", found()))?,
                    _ => return Err(format!("{EXPECTED_INT}, found {}", found())),
                };
                if value < min {
                    return Err(format!("must be at least {min}"));
                }
                if value > max {
                    return Err(format!("must be at most {max}"));
                }
                Value::Int(value)
            }
            Self::Enum { choices } => {
                let text = text.ok_or_else(|| format!("expected string, found {}", found()))?;
                Value::Choice(choose(choices, text)?)
            }
            Self::List { choices } => {
                let items: Vec<&str> = match (node, text) {
                    (Some(Node::Seq(items)), _) => items
                        .iter()
                        .enumerate()
                        .map(|(i, item)| match &item.value {
                            Node::Str(text) => Ok(text.as_str()),
                            other => Err(format!("item {i}: expected string, found {}", other.kind())),
                        })
                        .collect::<Result<_, _>>()?,
                    (_, Some(text)) => {
                        let items: Vec<&str> = text.split(',').map(str::trim).collect();
                        if items.iter().any(|item| item.is_empty()) {
                            return Err("empty item in comma-separated list".into());
                        }
                        items
                    }
                    _ => return Err(format!("expected string or sequence, found {}", found())),
                };
                if let Some(choices) = choices {
                    for (i, item) in items.iter().enumerate() {
                        choose(choices, item).map_err(|reason| format!("item {i}: {reason}"))?;
                    }
                }
                Value::List(items.into_iter().map(str::to_owned).collect())
            }
        };
        Ok(Some(value))
    }
}

/// Returns the entry of `choices` equal to `text`.
fn choose(choices: &'static [&'static str], text: &str) -> Result<&'static str, String> {
    choices
        .iter()
        .find(|name| **name == text)
        .copied()
        .ok_or_else(|| format!("expected one of {}", choices.join(", ")))
}

/// What an integer setting expects; errors append `, found <type>`.
const EXPECTED_INT: &str = "expected non-negative integer";

/// What a boolean setting expects; errors append `, found <type>`.
const EXPECTED_BOOL: &str = "expected boolean (true/false/1/0/t/f)";

/// Parses a boolean the way Go's `strconv.ParseBool` does.
fn parse_bool(text: &str) -> Option<bool> {
    match text {
        "1" | "t" | "T" | "TRUE" | "true" | "True" => Some(true),
        "0" | "f" | "F" | "FALSE" | "false" | "False" => Some(false),
        _ => None,
    }
}

/// Resolves a path found in the config file in `dir`.
///
/// Expands a leading `~` against `home`, then joins a relative result to `dir`.
#[must_use]
pub(crate) fn resolve_in_file(path: &Path, dir: &Path, home: Option<&Path>) -> PathBuf {
    let path = expand_tilde(path, home);
    if path.is_relative() { dir.join(path) } else { path }
}

/// Expands a leading `~` path component against `home`.
///
/// Requires at least one component after `~`, so `~`, `~/` and `~user`
/// stay literal. Returns `path` unchanged when `home` is `None`.
#[must_use]
pub(crate) fn expand_tilde(path: &Path, home: Option<&Path>) -> PathBuf {
    let Some(home) = home else {
        return path.to_path_buf();
    };
    match path.strip_prefix("~") {
        Ok(rest) if rest.components().next().is_some() => {
            let mut expanded = home.to_path_buf();
            expanded.extend(rest.components());
            expanded
        }
        _ => path.to_path_buf(),
    }
}

const fn int(min: u64, max: u64) -> Kind {
    Kind::Int { min, max }
}

const fn choice(choices: &'static [&'static str]) -> Kind {
    Kind::Enum { choices }
}

/// A list of free-form strings.
const LIST: Kind = Kind::List { choices: None };

/// A path whose env values stay as given.
const FILE_PATH: Kind = Kind::Path { home_from_env: false };

/// A path whose env values also expand `~`.
const HOME_PATH: Kind = Kind::Path { home_from_env: true };

const fn setting(path: &'static str, env: &'static str, kind: Kind) -> Setting {
    Setting { path, env, kind }
}

/// `transport` / `DBMCP_TRANSPORT`.
pub const TRANSPORT: Setting = setting("transport", "DBMCP_TRANSPORT", choice(&["stdio", "http"]));
/// `log_level` / `DBMCP_LOG_LEVEL`.
pub const LOG_LEVEL: Setting = setting(
    "log_level",
    "DBMCP_LOG_LEVEL",
    choice(&["error", "warn", "info", "debug", "trace"]),
);
/// `database.backend` / `DBMCP_DB_BACKEND`.
pub const DB_BACKEND: Setting = setting(
    "database.backend",
    "DBMCP_DB_BACKEND",
    choice(&["mysql", "mariadb", "postgres", "sqlite"]),
);
/// `database.host` / `DBMCP_DB_HOST`.
pub const DB_HOST: Setting = setting("database.host", "DBMCP_DB_HOST", Kind::Text);
/// `database.port` / `DBMCP_DB_PORT`.
pub const DB_PORT: Setting = setting("database.port", "DBMCP_DB_PORT", int(0, u16::MAX as u64));
/// `database.user` / `DBMCP_DB_USER`.
pub const DB_USER: Setting = setting("database.user", "DBMCP_DB_USER", Kind::Text);
/// `database.password` / `DBMCP_DB_PASSWORD`.
pub const DB_PASSWORD: Setting = setting("database.password", "DBMCP_DB_PASSWORD", Kind::Text);
/// `database.name` / `DBMCP_DB_NAME`.
pub const DB_NAME: Setting = setting("database.name", "DBMCP_DB_NAME", Kind::Text);
/// `database.charset` / `DBMCP_DB_CHARSET`.
pub const DB_CHARSET: Setting = setting("database.charset", "DBMCP_DB_CHARSET", Kind::Text);
/// `database.ssl` / `DBMCP_DB_SSL`.
pub const DB_SSL: Setting = setting("database.ssl", "DBMCP_DB_SSL", Kind::Bool);
/// `database.ssl_ca` / `DBMCP_DB_SSL_CA`.
pub const DB_SSL_CA: Setting = setting("database.ssl_ca", "DBMCP_DB_SSL_CA", FILE_PATH);
/// `database.ssl_cert` / `DBMCP_DB_SSL_CERT`.
pub const DB_SSL_CERT: Setting = setting("database.ssl_cert", "DBMCP_DB_SSL_CERT", FILE_PATH);
/// `database.ssl_key` / `DBMCP_DB_SSL_KEY`.
pub const DB_SSL_KEY: Setting = setting("database.ssl_key", "DBMCP_DB_SSL_KEY", FILE_PATH);
/// `database.ssl_verify_cert` / `DBMCP_DB_SSL_VERIFY_CERT`.
pub const DB_SSL_VERIFY_CERT: Setting = setting("database.ssl_verify_cert", "DBMCP_DB_SSL_VERIFY_CERT", Kind::Bool);
/// `database.read_only` / `DBMCP_DB_READ_ONLY`.
pub const DB_READ_ONLY: Setting = setting("database.read_only", "DBMCP_DB_READ_ONLY", Kind::Bool);
/// `database.max_pool_size` / `DBMCP_DB_MAX_POOL_SIZE`.
pub const DB_MAX_POOL_SIZE: Setting = setting(
    "database.max_pool_size",
    "DBMCP_DB_MAX_POOL_SIZE",
    int(1, u32::MAX as u64),
);
/// `database.connection_timeout` / `DBMCP_DB_CONNECTION_TIMEOUT`.
pub const DB_CONNECTION_TIMEOUT: Setting = setting(
    "database.connection_timeout",
    "DBMCP_DB_CONNECTION_TIMEOUT",
    int(1, u64::MAX),
);
/// `database.query_timeout` / `DBMCP_DB_QUERY_TIMEOUT`.
pub const DB_QUERY_TIMEOUT: Setting = setting("database.query_timeout", "DBMCP_DB_QUERY_TIMEOUT", int(0, u64::MAX));
/// `database.page_size` / `DBMCP_DB_PAGE_SIZE`.
pub const DB_PAGE_SIZE: Setting = setting(
    "database.page_size",
    "DBMCP_DB_PAGE_SIZE",
    int(1, DatabaseConfig::MAX_PAGE_SIZE as u64),
);
/// `http.host` / `DBMCP_HTTP_HOST`.
pub const HTTP_HOST: Setting = setting("http.host", "DBMCP_HTTP_HOST", Kind::Text);
/// `http.port` / `DBMCP_HTTP_PORT`.
pub const HTTP_PORT: Setting = setting("http.port", "DBMCP_HTTP_PORT", int(0, u16::MAX as u64));
/// `http.allowed_origins` / `DBMCP_HTTP_ALLOWED_ORIGINS`.
pub const HTTP_ALLOWED_ORIGINS: Setting = setting("http.allowed_origins", "DBMCP_HTTP_ALLOWED_ORIGINS", LIST);
/// `http.allowed_hosts` / `DBMCP_HTTP_ALLOWED_HOSTS`.
pub const HTTP_ALLOWED_HOSTS: Setting = setting("http.allowed_hosts", "DBMCP_HTTP_ALLOWED_HOSTS", LIST);
/// `http.auth_token` / `DBMCP_HTTP_AUTH_TOKEN`.
pub const HTTP_AUTH_TOKEN: Setting = setting("http.auth_token", "DBMCP_HTTP_AUTH_TOKEN", Kind::Text);
/// `http.allow_unauthenticated` / `DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED`.
pub const HTTP_ALLOW_UNAUTHENTICATED: Setting = setting(
    "http.allow_unauthenticated",
    "DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED",
    Kind::Bool,
);
/// `pii.enabled` / `DBMCP_PII_ENABLE`.
pub const PII_ENABLE: Setting = setting("pii.enabled", "DBMCP_PII_ENABLE", Kind::Bool);
/// `pii.operator` / `DBMCP_PII_OPERATOR`.
pub const PII_OPERATOR: Setting = setting(
    "pii.operator",
    "DBMCP_PII_OPERATOR",
    choice(&["replace", "mask", "redact", "hash"]),
);
/// `pii.categories` / `DBMCP_PII_CATEGORIES`.
pub const PII_CATEGORIES: Setting = setting(
    "pii.categories",
    "DBMCP_PII_CATEGORIES",
    Kind::List {
        choices: Some(&[
            "personal",
            "financial",
            "government",
            "contact",
            "network",
            "digital-identity",
            "crypto",
        ]),
    },
);
/// `pii.ner_enabled` / `DBMCP_PII_NER_ENABLE`.
pub const PII_NER_ENABLE: Setting = setting("pii.ner_enabled", "DBMCP_PII_NER_ENABLE", Kind::Bool);
/// `pii.ner_model` / `DBMCP_PII_NER_MODEL`.
pub const PII_NER_MODEL: Setting = setting("pii.ner_model", "DBMCP_PII_NER_MODEL", FILE_PATH);
/// `ssh.host` / `DBMCP_SSH_HOST`.
pub const SSH_HOST: Setting = setting("ssh.host", "DBMCP_SSH_HOST", Kind::Text);
/// `ssh.port` / `DBMCP_SSH_PORT`.
pub const SSH_PORT: Setting = setting("ssh.port", "DBMCP_SSH_PORT", int(1, u16::MAX as u64));
/// `ssh.user` / `DBMCP_SSH_USER`.
pub const SSH_USER: Setting = setting("ssh.user", "DBMCP_SSH_USER", Kind::Text);
/// `ssh.password` / `DBMCP_SSH_PASSWORD`.
pub const SSH_PASSWORD: Setting = setting("ssh.password", "DBMCP_SSH_PASSWORD", Kind::Text);
/// `ssh.key` / `DBMCP_SSH_KEY`.
pub const SSH_KEY: Setting = setting("ssh.key", "DBMCP_SSH_KEY", HOME_PATH);
/// `ssh.key_passphrase` / `DBMCP_SSH_KEY_PASSPHRASE`.
pub const SSH_KEY_PASSPHRASE: Setting = setting("ssh.key_passphrase", "DBMCP_SSH_KEY_PASSPHRASE", Kind::Text);
/// `ssh.known_hosts` / `DBMCP_SSH_KNOWN_HOSTS`.
pub const SSH_KNOWN_HOSTS: Setting = setting("ssh.known_hosts", "DBMCP_SSH_KNOWN_HOSTS", HOME_PATH);
/// `ssh.strict_host_key_checking` / `DBMCP_SSH_STRICT_HOST_KEY_CHECKING`.
pub const SSH_STRICT_HOST_KEY_CHECKING: Setting = setting(
    "ssh.strict_host_key_checking",
    "DBMCP_SSH_STRICT_HOST_KEY_CHECKING",
    choice(&["yes", "accept-new", "no"]),
);
/// `ssh.keepalive_interval` / `DBMCP_SSH_KEEPALIVE_INTERVAL`.
pub const SSH_KEEPALIVE_INTERVAL: Setting = setting(
    "ssh.keepalive_interval",
    "DBMCP_SSH_KEEPALIVE_INTERVAL",
    int(0, u64::MAX),
);
/// `ssh.local_port` / `DBMCP_SSH_LOCAL_PORT`.
pub const SSH_LOCAL_PORT: Setting = setting("ssh.local_port", "DBMCP_SSH_LOCAL_PORT", int(1, u16::MAX as u64));

/// Every setting, in documentation order.
pub const SETTINGS: &[Setting] = &[
    TRANSPORT,
    LOG_LEVEL,
    DB_BACKEND,
    DB_HOST,
    DB_PORT,
    DB_USER,
    DB_PASSWORD,
    DB_NAME,
    DB_CHARSET,
    DB_SSL,
    DB_SSL_CA,
    DB_SSL_CERT,
    DB_SSL_KEY,
    DB_SSL_VERIFY_CERT,
    DB_READ_ONLY,
    DB_MAX_POOL_SIZE,
    DB_CONNECTION_TIMEOUT,
    DB_QUERY_TIMEOUT,
    DB_PAGE_SIZE,
    HTTP_HOST,
    HTTP_PORT,
    HTTP_ALLOWED_ORIGINS,
    HTTP_ALLOWED_HOSTS,
    HTTP_AUTH_TOKEN,
    HTTP_ALLOW_UNAUTHENTICATED,
    PII_ENABLE,
    PII_OPERATOR,
    PII_CATEGORIES,
    PII_NER_ENABLE,
    PII_NER_MODEL,
    SSH_HOST,
    SSH_PORT,
    SSH_USER,
    SSH_PASSWORD,
    SSH_KEY,
    SSH_KEY_PASSPHRASE,
    SSH_KNOWN_HOSTS,
    SSH_STRICT_HOST_KEY_CHECKING,
    SSH_KEEPALIVE_INTERVAL,
    SSH_LOCAL_PORT,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn paths_and_env_names_are_unique() {
        let paths: HashSet<_> = SETTINGS.iter().map(|s| s.path).collect();
        let envs: HashSet<_> = SETTINGS.iter().map(|s| s.env).collect();
        assert_eq!(paths.len(), SETTINGS.len(), "duplicate YAML path");
        assert_eq!(envs.len(), SETTINGS.len(), "duplicate env name");
    }

    #[test]
    fn env_names_use_the_dbmcp_prefix_and_skip_loader_inputs() {
        for setting in SETTINGS {
            assert!(setting.env.starts_with("DBMCP_"), "{}", setting.env);
            assert!(![CONFIG_VAR, ENV_FILE_VAR].contains(&setting.env), "{}", setting.env);
        }
    }

    #[test]
    fn int_ranges_are_well_formed() {
        for setting in SETTINGS {
            if let Kind::Int { min, max } = setting.kind {
                assert!(min <= max, "{}", setting.path);
            }
        }
    }

    fn home() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(r"C:\Users\deploy")
        } else {
            PathBuf::from("/home/deploy")
        }
    }

    /// The config file's directory in tests.
    fn dir() -> PathBuf {
        home().join("conf")
    }

    fn text(kind: Kind, text: &str) -> Result<Option<Value>, String> {
        kind.check(Raw::Text(text), Some(&home()))
    }

    fn yaml_with(kind: Kind, yaml: &str, home: Option<&Path>) -> Result<Option<Value>, String> {
        let root = crate::node::parse(&format!("x: {yaml}\n")).expect("valid yaml");
        let Node::Map(entries) = root.value else {
            panic!("expected mapping");
        };
        kind.check(
            Raw::Node {
                node: &entries["x"].1.value,
                dir: &dir(),
            },
            home,
        )
    }

    fn yaml(kind: Kind, yaml: &str) -> Result<Option<Value>, String> {
        yaml_with(kind, yaml, Some(&home()))
    }

    const COLORS: Kind = choice(&["red", "green"]);

    #[test]
    fn text_is_cast_by_kind() {
        assert_eq!(text(int(0, 10), "7"), Ok(Some(Value::Int(7))));
        assert_eq!(
            text(int(0, u64::MAX), "18446744073709551615"),
            Ok(Some(Value::Int(u64::MAX)))
        );
        assert_eq!(
            text(LIST, " a , b"),
            Ok(Some(Value::List(vec!["a".into(), "b".into()])))
        );
        assert_eq!(text(COLORS, "red"), Ok(Some(Value::Choice("red"))));
        assert_eq!(text(FILE_PATH, "~/x"), Ok(Some(Value::Path("~/x".into()))));
    }

    #[test]
    fn bools_accept_the_parse_bool_set() {
        for (value, expected) in [
            ("1", true),
            ("t", true),
            ("T", true),
            ("TRUE", true),
            ("true", true),
            ("True", true),
            ("0", false),
            ("f", false),
            ("F", false),
            ("FALSE", false),
            ("false", false),
            ("False", false),
        ] {
            assert_eq!(text(Kind::Bool, value), Ok(Some(Value::Bool(expected))), "{value}");
        }
        assert_eq!(yaml(Kind::Bool, "\"0\""), Ok(Some(Value::Bool(false))));
        assert_eq!(yaml(Kind::Bool, "true"), Ok(Some(Value::Bool(true))));
    }

    #[test]
    fn yaml_strings_are_cast_like_text() {
        assert_eq!(yaml(int(0, 10), "\"5\""), Ok(Some(Value::Int(5))));
        assert_eq!(
            yaml(LIST, "\"a, b\""),
            Ok(Some(Value::List(vec!["a".into(), "b".into()])))
        );
        assert_eq!(
            yaml(LIST, "[a, b]"),
            Ok(Some(Value::List(vec!["a".into(), "b".into()])))
        );
        assert_eq!(yaml(Kind::Text, "\"1\""), Ok(Some(Value::Text("1".into()))));
    }

    #[test]
    fn empty_and_null_are_unset_except_empty_strings() {
        for kind in [int(0, 1), Kind::Bool, LIST] {
            assert_eq!(text(kind, ""), Ok(None), "{kind:?}");
            assert_eq!(yaml(kind, "\"\""), Ok(None), "{kind:?}");
        }
        for kind in [int(0, 1), Kind::Bool, LIST, Kind::Text, COLORS] {
            assert_eq!(yaml(kind, "~"), Ok(None), "{kind:?}");
        }
        assert_eq!(text(Kind::Text, ""), Ok(Some(Value::Text(String::new()))));
    }

    #[test]
    fn integers_are_checked_against_their_range() {
        assert_eq!(text(int(1, 500), "500"), Ok(Some(Value::Int(500))));
        assert_eq!(text(int(1, 500), "501"), Err("must be at most 500".into()));
        assert_eq!(text(int(1, 500), "0"), Err("must be at least 1".into()));
        assert_eq!(yaml(int(0, 9), "-5"), Err("must be at least 0".into()));
    }

    #[test]
    fn choices_are_checked_for_enums_and_list_items() {
        assert_eq!(text(COLORS, "blue"), Err("expected one of red, green".into()));
        let tags = Kind::List {
            choices: Some(&["a", "b"]),
        };
        assert_eq!(text(tags, "b,a"), Ok(Some(Value::List(vec!["b".into(), "a".into()]))));
        assert_eq!(text(tags, "a,c"), Err("item 1: expected one of a, b".into()));
    }

    #[test]
    fn errors_name_the_type_found_without_the_value() {
        let cases = [
            (
                text(int(0, 1), "-1-secret"),
                "expected non-negative integer, found string",
            ),
            (
                text(Kind::Bool, "yes"),
                "expected boolean (true/false/1/0/t/f), found string",
            ),
            (text(LIST, "a,,b"), "empty item in comma-separated list"),
            (yaml(int(0, 9), "1.0"), "expected non-negative integer, found float"),
            (
                yaml(Kind::Bool, "1"),
                "expected boolean (true/false/1/0/t/f), found integer",
            ),
            (yaml(Kind::Text, "[secret]"), "expected string, found sequence"),
            (yaml(COLORS, "1"), "expected string, found integer"),
            (yaml(LIST, "{a: b}"), "expected string or sequence, found mapping"),
            (yaml(LIST, "[a, 1]"), "item 1: expected string, found integer"),
        ];
        for (result, reason) in cases {
            assert_eq!(result, Err(reason.into()));
        }
    }

    /// Names in a typed `CHOICES` table.
    fn names<T>(choices: &[(&'static str, T)]) -> Vec<&'static str> {
        choices.iter().map(|(name, _)| *name).collect()
    }

    #[test]
    fn choices_match_each_types_names() {
        use crate::database::DatabaseBackend;
        use crate::log::LogLevel;
        use crate::pii::{PiiCategory, PiiOperator};
        use crate::ssh::StrictHostKeyChecking;
        use crate::transport::Transport;

        let tied = [
            (TRANSPORT, names(Transport::CHOICES)),
            (LOG_LEVEL, names(LogLevel::CHOICES)),
            (DB_BACKEND, names(DatabaseBackend::CHOICES)),
            (PII_OPERATOR, names(PiiOperator::CHOICES)),
            (PII_CATEGORIES, names(PiiCategory::CHOICES)),
            (SSH_STRICT_HOST_KEY_CHECKING, names(StrictHostKeyChecking::CHOICES)),
        ];
        let with_choices = SETTINGS
            .iter()
            .filter(|s| matches!(s.kind, Kind::Enum { .. } | Kind::List { choices: Some(_) }))
            .count();
        assert_eq!(with_choices, tied.len(), "tie every setting with choices to its type");
        for (setting, expected) in tied {
            let choices = match setting.kind {
                Kind::Enum { choices } | Kind::List { choices: Some(choices) } => choices,
                kind => panic!("{} has no choices: {kind:?}", setting.path),
            };
            assert_eq!(choices, expected, "{}", setting.path);
        }
    }

    #[test]
    fn every_config_field_has_a_setting() {
        // One row per leaf field of `Config` plus the top-level log level.
        assert_eq!(SETTINGS.len(), 40);
    }
    fn path(result: Result<Option<Value>, String>) -> PathBuf {
        match result {
            Ok(Some(Value::Path(path))) => path,
            other => panic!("expected a path, got {other:?}"),
        }
    }

    #[test]
    fn paths_resolve_by_where_they_are_found() {
        let absolute = home().join("ca.pem");
        let cases = [
            (yaml(FILE_PATH, "certs/ca.pem"), dir().join("certs/ca.pem")),
            (yaml(FILE_PATH, "~/ca.pem"), home().join("ca.pem")),
            (yaml(FILE_PATH, &absolute.display().to_string()), absolute.clone()),
            (yaml_with(FILE_PATH, "~/ca.pem", None), dir().join("~/ca.pem")),
            (yaml(FILE_PATH, "\"\""), PathBuf::new()),
            (text(FILE_PATH, "certs/ca.pem"), PathBuf::from("certs/ca.pem")),
            (text(FILE_PATH, "~/ca.pem"), PathBuf::from("~/ca.pem")),
            (text(HOME_PATH, "~/.ssh/id"), home().join(".ssh").join("id")),
            (text(HOME_PATH, ""), PathBuf::new()),
        ];
        for (i, (result, expected)) in cases.into_iter().enumerate() {
            assert_eq!(path(result), expected, "case {i}");
        }
    }

    fn expand(path: &str) -> PathBuf {
        expand_tilde(Path::new(path), Some(&home()))
    }

    #[test]
    fn expand_tilde_slash_prefix() {
        assert_eq!(expand("~/.ssh/id_ed25519"), home().join(".ssh").join("id_ed25519"));
    }

    #[test]
    fn expand_tilde_double_slash_stays_under_home() {
        assert_eq!(expand("~//.ssh/known_hosts"), home().join(".ssh").join("known_hosts"));
    }

    #[test]
    fn expand_tilde_leaves_bare_and_trailing_slash_literal() {
        assert_eq!(expand("~"), PathBuf::from("~"));
        assert_eq!(expand("~/"), PathBuf::from("~/"));
    }

    #[test]
    fn expand_tilde_leaves_tilde_user_literal() {
        assert_eq!(expand("~deploy/.ssh/id"), PathBuf::from("~deploy/.ssh/id"));
    }

    #[test]
    fn expand_tilde_leaves_non_tilde_paths_untouched() {
        assert_eq!(expand("keys/id"), PathBuf::from("keys/id"));
        assert_eq!(expand("a/~/b"), PathBuf::from("a/~/b"));
    }

    #[test]
    fn expand_tilde_without_home_is_literal() {
        assert_eq!(expand_tilde(Path::new("~/.ssh/id"), None), PathBuf::from("~/.ssh/id"));
    }

    #[cfg(windows)]
    #[test]
    fn expand_tilde_backslash_prefix_on_windows() {
        assert_eq!(expand(r"~\.ssh\id_ed25519"), home().join(".ssh").join("id_ed25519"));
        assert_eq!(expand(r"~\"), PathBuf::from(r"~\"));
    }

    #[cfg(not(windows))]
    #[test]
    fn expand_tilde_backslash_is_literal_off_windows() {
        assert_eq!(expand(r"~\.ssh\id"), PathBuf::from(r"~\.ssh\id"));
    }
}
