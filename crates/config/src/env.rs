//! The setting table: each setting's YAML key, env name, and value kind.
//!
//! Every setting is one constant here, and the loader reads it through that
//! constant, so a misspelt setting is a compile error. Env names are
//! hand-picked, not derived from key paths. Every leaf setting has exactly
//! one name and every name maps to exactly one setting. The [`Kind`] carries
//! what the loader needs to cast a value: integer ranges and path rules are
//! stated here and nowhere else.

use crate::database::DatabaseConfig;

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
    /// How the loader casts and checks the value.
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
    /// A kebab-case enum value.
    Enum,
    /// A YAML sequence, or comma-separated text.
    List,
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

const fn int(min: u64, max: u64) -> Kind {
    Kind::Int { min, max }
}

/// A path whose env values stay as given.
const FILE_PATH: Kind = Kind::Path { home_from_env: false };

/// A path whose env values also expand `~`.
const HOME_PATH: Kind = Kind::Path { home_from_env: true };

const fn setting(path: &'static str, env: &'static str, kind: Kind) -> Setting {
    Setting { path, env, kind }
}

/// `transport` / `DBMCP_TRANSPORT`.
pub const TRANSPORT: Setting = setting("transport", "DBMCP_TRANSPORT", Kind::Enum);
/// `log_level` / `DBMCP_LOG_LEVEL`.
pub const LOG_LEVEL: Setting = setting("log_level", "DBMCP_LOG_LEVEL", Kind::Enum);
/// `database.backend` / `DBMCP_DB_BACKEND`.
pub const DB_BACKEND: Setting = setting("database.backend", "DBMCP_DB_BACKEND", Kind::Enum);
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
pub const HTTP_ALLOWED_ORIGINS: Setting = setting("http.allowed_origins", "DBMCP_HTTP_ALLOWED_ORIGINS", Kind::List);
/// `http.allowed_hosts` / `DBMCP_HTTP_ALLOWED_HOSTS`.
pub const HTTP_ALLOWED_HOSTS: Setting = setting("http.allowed_hosts", "DBMCP_HTTP_ALLOWED_HOSTS", Kind::List);
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
pub const PII_OPERATOR: Setting = setting("pii.operator", "DBMCP_PII_OPERATOR", Kind::Enum);
/// `pii.categories` / `DBMCP_PII_CATEGORIES`.
pub const PII_CATEGORIES: Setting = setting("pii.categories", "DBMCP_PII_CATEGORIES", Kind::List);
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
    Kind::Enum,
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

    #[test]
    fn every_config_field_has_a_setting() {
        // One row per leaf field of `Config` plus the top-level log level.
        assert_eq!(SETTINGS.len(), 40);
    }
}
