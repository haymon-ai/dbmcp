//! Top-level [`Config`] composing the database, http, pii, and ssh sections.

use crate::database::DatabaseConfig;
use crate::http::HttpConfig;
use crate::pii::PiiConfig;
use crate::ssh::SshConfig;

/// Runtime configuration for the MCP server.
///
/// Composes [`DatabaseConfig`] with an optional [`HttpConfig`], a
/// [`PiiConfig`], and an optional [`SshConfig`]. HTTP config is present
/// only when the HTTP transport is selected (via subcommand or
/// `MCP_TRANSPORT` env var); PII config is always present; SSH config is
/// present only when `DB_SSH_HOST` is set. Logging is configured directly from CLI arguments
/// before `Config` is constructed, so it is not part of this struct.
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
    use super::*;
    use crate::pii::PiiOperator;

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
}
