//! Top-level [`Config`] composing the database, http, pii, and ssh sections.

use crate::database::DatabaseConfig;
use crate::http::HttpConfig;
use crate::pii::PiiConfig;
use crate::ssh::SshConfig;

/// Runtime configuration for the MCP server.
///
/// Composes [`DatabaseConfig`] with an optional [`HttpConfig`], a
/// [`PiiConfig`], and an optional [`SshConfig`]. HTTP config is present
/// only when the HTTP transport is selected; PII config is always present;
/// SSH config is present only when an SSH host is set. The log level is
/// resolved alongside it in [`Resolved`](crate::Resolved), so it is not
/// part of this struct.
#[derive(Clone, Debug)]
pub struct Config {
    /// Database connection and behavior settings.
    pub database: DatabaseConfig,

    /// HTTP transport settings (present only when HTTP transport is active).
    pub http: Option<HttpConfig>,

    /// PII redaction settings.
    pub pii: PiiConfig,

    /// SSH tunnel settings (`None` = connect to the database directly).
    pub ssh: Option<SshConfig>,
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::pii::PiiOperator;
    use crate::ssh::StrictHostKeyChecking;

    fn mysql_config() -> Config {
        Config {
            database: DatabaseConfig {
                port: 3306,
                user: "root".into(),
                password: Some("secret".into()),
                ..DatabaseConfig::default()
            },
            http: None,
            pii: PiiConfig::default(),
            ssh: None,
        }
    }

    #[test]
    fn config_debug_includes_pii_section() {
        let config = Config {
            pii: PiiConfig {
                enabled: true,
                operator: PiiOperator::Mask,
                categories: None,
                ..PiiConfig::default()
            },
            ..mysql_config()
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("pii: PiiConfig { enabled: true"),
            "expected pii section in debug output: {debug}"
        );
        assert!(debug.contains("operator: Mask"), "expected operator in debug: {debug}");
    }

    #[test]
    fn config_debug_redacts_every_secret() {
        let config = Config {
            database: DatabaseConfig {
                password: Some("hunter2-db-password".into()),
                ..DatabaseConfig::default()
            },
            http: Some(HttpConfig {
                host: HttpConfig::DEFAULT_HOST.into(),
                port: HttpConfig::DEFAULT_PORT,
                allowed_origins: HttpConfig::default_allowed_origins(),
                allowed_hosts: HttpConfig::default_allowed_hosts(),
                auth_token: Some("hunter2-token".into()),
                allow_unauthenticated: false,
            }),
            ssh: Some(SshConfig {
                host: "bastion".into(),
                port: SshConfig::DEFAULT_PORT,
                user: "deploy".into(),
                password: Some("hunter2-ssh-password".into()),
                key: None,
                key_passphrase: Some("hunter2-passphrase".into()),
                known_hosts: None,
                strict_host_key_checking: StrictHostKeyChecking::Yes,
                keepalive_interval: Duration::ZERO,
                local_port: None,
            }),
            pii: PiiConfig::default(),
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"), "secret leaked: {debug}");
        assert_eq!(debug.matches("[REDACTED]").count(), 4, "{debug}");
    }
}
