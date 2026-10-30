//! Top-level [`Config`] composing the transport and the database, http, pii, and ssh sections.

use crate::database::DatabaseConfig;
use crate::http::HttpConfig;
use crate::pii::PiiConfig;
use crate::ssh::SshConfig;
use crate::transport::Transport;

/// Runtime configuration for the MCP server.
///
/// Composes the selected [`Transport`] with [`DatabaseConfig`] and the
/// optional [`HttpConfig`], [`PiiConfig`], and [`SshConfig`] sections.
/// HTTP config is present only when `--transport http` / `DBMCP_TRANSPORT=http`
/// is selected; PII config only when `--pii` / `DBMCP_PII_ENABLE` is on; SSH
/// config only when `--ssh-host` / `DBMCP_SSH_HOST` is set. Logging is configured directly
/// from CLI arguments before `Config` is constructed, so it is not part of
/// this struct.
#[derive(Clone, Debug)]
pub struct Config {
    /// MCP transport the server runs on.
    pub transport: Transport,

    /// Database connection and behavior settings.
    pub database: DatabaseConfig,

    /// HTTP transport settings (present only when the HTTP transport is selected).
    pub http: Option<HttpConfig>,

    /// PII redaction settings (`None` = redaction off).
    pub pii: Option<PiiConfig>,

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
            transport: Transport::Stdio,
            http: None,
            pii: None,
            ssh: None,
        }
    }

    #[test]
    fn config_debug_includes_pii_section() {
        let config = Config {
            pii: Some(PiiConfig {
                operator: PiiOperator::Mask,
                categories: None,
                ..PiiConfig::default()
            }),
            ..mysql_config()
        };
        let debug = format!("{config:?}");
        assert!(
            debug.contains("pii: Some(PiiConfig { operator: Mask"),
            "expected pii section in debug output: {debug}"
        );
    }

    #[test]
    fn config_debug_redacts_every_secret() {
        let config = Config {
            transport: Transport::Http,
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
            pii: None,
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("hunter2"), "secret leaked: {debug}");
        assert_eq!(debug.matches("[REDACTED]").count(), 4, "{debug}");
    }
}
