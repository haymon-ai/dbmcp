//! Top-level [`Config`] composing the database, http, and pii sections.

use crate::database::DatabaseConfig;
use crate::http::HttpConfig;
use crate::pii::PiiConfig;

/// Runtime configuration for the MCP server.
///
/// Composes [`DatabaseConfig`] with an optional [`HttpConfig`] and a
/// [`PiiConfig`]. HTTP config is present only when the HTTP transport
/// is selected (via subcommand or `MCP_TRANSPORT` env var); PII config
/// is always present. Logging is configured directly from CLI arguments
/// before `Config` is constructed, so it is not part of this struct.
#[derive(Clone, Debug)]
pub struct Config {
    /// Database connection and behavior settings.
    pub database: DatabaseConfig,

    /// HTTP transport settings (present only when HTTP transport is active).
    pub http: Option<HttpConfig>,

    /// PII redaction settings.
    pub pii: PiiConfig,
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
        }
    }

    #[test]
    fn config_debug_includes_pii_section() {
        let config = Config {
            pii: PiiConfig {
                enabled: true,
                operator: PiiOperator::Mask,
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
