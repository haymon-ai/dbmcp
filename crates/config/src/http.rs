//! HTTP transport binding and security settings.

use crate::error::{ConfigError, ConfigErrors};

/// HTTP transport binding and security settings.
#[derive(Clone, Debug)]
pub struct HttpConfig {
    /// Bind host for HTTP transport.
    pub host: String,

    /// Bind port for HTTP transport.
    pub port: u16,

    /// Allowed browser origins for both CORS preflight and rmcp server-side validation.
    pub allowed_origins: Vec<String>,

    /// Allowed host names.
    pub allowed_hosts: Vec<String>,
}

impl HttpConfig {
    /// Default HTTP bind host.
    pub const DEFAULT_HOST: &'static str = "127.0.0.1";
    /// Default HTTP bind port.
    pub const DEFAULT_PORT: u16 = 9001;

    /// Return default allowed CORS origins.
    #[must_use]
    pub fn default_allowed_origins() -> Vec<String> {
        vec![
            "http://localhost".into(),
            "http://127.0.0.1".into(),
            "https://localhost".into(),
            "https://127.0.0.1".into(),
        ]
    }

    /// Returns default allowed host names.
    #[must_use]
    pub fn default_allowed_hosts() -> Vec<String> {
        vec!["localhost".into(), "127.0.0.1".into(), "::1".into()]
    }

    /// Validates this configuration, accumulating every rule violation.
    ///
    /// Rules enforced:
    /// - `host` MUST NOT be empty or whitespace-only after trim.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] containing one [`ConfigError`] per failing rule.
    pub fn validate(&self) -> Result<(), ConfigErrors> {
        let mut errors = Vec::new();
        if self.host.trim().is_empty() {
            errors.push(ConfigError::EmptyHttpHost);
        }
        ConfigErrors::from_vec(errors).map_or(Ok(()), Err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_host(host: &str) -> HttpConfig {
        HttpConfig {
            host: host.into(),
            port: HttpConfig::DEFAULT_PORT,
            allowed_origins: HttpConfig::default_allowed_origins(),
            allowed_hosts: HttpConfig::default_allowed_hosts(),
        }
    }

    #[test]
    fn default_config_validates_ok() {
        config_with_host(HttpConfig::DEFAULT_HOST)
            .validate()
            .expect("default host must validate");
    }

    #[test]
    fn validate_rejects_empty_host() {
        let errors = config_with_host("").validate().expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn validate_rejects_whitespace_only_host() {
        let errors = config_with_host("   ")
            .validate()
            .expect_err("whitespace host must fail");
        assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpHost)));
    }
}
