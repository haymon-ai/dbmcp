//! The table pairing every setting's YAML key path with its `DBMCP_*` env name.
//!
//! Env names are hand-picked, not derived from key paths: each setting
//! declares its name here once. Every leaf setting has exactly one name and
//! every name maps to exactly one setting.

/// Env var naming the YAML config file; a loader input, not a setting.
pub const CONFIG_VAR: &str = "DBMCP_CONFIG";

/// Env var naming the dotenv file; a loader input, not a setting.
pub const ENV_FILE_VAR: &str = "DBMCP_ENV_FILE";

/// One setting: its dotted YAML key path and its env var name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Setting {
    /// Dotted YAML key path, e.g. `database.host`.
    pub(crate) path: &'static str,
    /// Env var name, e.g. `DBMCP_DB_HOST`.
    pub(crate) env: &'static str,
}

const fn setting(path: &'static str, env: &'static str) -> Setting {
    Setting { path, env }
}

/// Every setting, in documentation order.
pub(crate) const SETTINGS: &[Setting] = &[
    setting("transport", "DBMCP_TRANSPORT"),
    setting("log_level", "DBMCP_LOG_LEVEL"),
    setting("database.backend", "DBMCP_DB_BACKEND"),
    setting("database.host", "DBMCP_DB_HOST"),
    setting("database.port", "DBMCP_DB_PORT"),
    setting("database.user", "DBMCP_DB_USER"),
    setting("database.password", "DBMCP_DB_PASSWORD"),
    setting("database.name", "DBMCP_DB_NAME"),
    setting("database.charset", "DBMCP_DB_CHARSET"),
    setting("database.ssl", "DBMCP_DB_SSL"),
    setting("database.ssl_ca", "DBMCP_DB_SSL_CA"),
    setting("database.ssl_cert", "DBMCP_DB_SSL_CERT"),
    setting("database.ssl_key", "DBMCP_DB_SSL_KEY"),
    setting("database.ssl_verify_cert", "DBMCP_DB_SSL_VERIFY_CERT"),
    setting("database.read_only", "DBMCP_DB_READ_ONLY"),
    setting("database.max_pool_size", "DBMCP_DB_MAX_POOL_SIZE"),
    setting("database.connection_timeout", "DBMCP_DB_CONNECTION_TIMEOUT"),
    setting("database.query_timeout", "DBMCP_DB_QUERY_TIMEOUT"),
    setting("database.page_size", "DBMCP_DB_PAGE_SIZE"),
    setting("http.host", "DBMCP_HTTP_HOST"),
    setting("http.port", "DBMCP_HTTP_PORT"),
    setting("http.allowed_origins", "DBMCP_HTTP_ALLOWED_ORIGINS"),
    setting("http.allowed_hosts", "DBMCP_HTTP_ALLOWED_HOSTS"),
    setting("http.auth_token", "DBMCP_HTTP_AUTH_TOKEN"),
    setting(
        "http.allow_unauthenticated",
        "DBMCP_HTTP_DANGEROUSLY_ALLOW_UNAUTHENTICATED",
    ),
    setting("pii.enabled", "DBMCP_PII_ENABLE"),
    setting("pii.operator", "DBMCP_PII_OPERATOR"),
    setting("pii.categories", "DBMCP_PII_CATEGORIES"),
    setting("pii.ner_enabled", "DBMCP_PII_NER_ENABLE"),
    setting("pii.ner_model", "DBMCP_PII_NER_MODEL"),
    setting("ssh.host", "DBMCP_SSH_HOST"),
    setting("ssh.port", "DBMCP_SSH_PORT"),
    setting("ssh.user", "DBMCP_SSH_USER"),
    setting("ssh.password", "DBMCP_SSH_PASSWORD"),
    setting("ssh.key", "DBMCP_SSH_KEY"),
    setting("ssh.key_passphrase", "DBMCP_SSH_KEY_PASSPHRASE"),
    setting("ssh.known_hosts", "DBMCP_SSH_KNOWN_HOSTS"),
    setting("ssh.strict_host_key_checking", "DBMCP_SSH_STRICT_HOST_KEY_CHECKING"),
    setting("ssh.keepalive_interval", "DBMCP_SSH_KEEPALIVE_INTERVAL"),
    setting("ssh.local_port", "DBMCP_SSH_LOCAL_PORT"),
];

/// Returns the setting at the dotted YAML key `path`.
///
/// # Panics
///
/// Panics when `path` is not in [`SETTINGS`]; callers pass literals.
pub(crate) fn by_path(path: &str) -> Setting {
    *SETTINGS
        .iter()
        .find(|setting| setting.path == path)
        .unwrap_or_else(|| panic!("unknown setting path {path}"))
}

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
    fn every_config_field_has_a_setting() {
        // One row per leaf field of `Config` plus the top-level log level.
        assert_eq!(SETTINGS.len(), 40);
    }
}
