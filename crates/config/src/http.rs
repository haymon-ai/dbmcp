//! HTTP transport binding and security settings.

use std::net::IpAddr;

use crate::error::{ConfigError, ConfigErrors};

/// HTTP transport binding and security settings.
#[derive(Clone)]
pub struct HttpConfig {
    /// Bind host for HTTP transport.
    pub host: String,

    /// Bind port for HTTP transport.
    pub port: u16,

    /// Allowed browser origins for both CORS preflight and rmcp server-side validation.
    pub allowed_origins: Vec<String>,

    /// Allowed host names.
    pub allowed_hosts: Vec<String>,

    /// Bearer token required on every request; `None` disables authentication.
    pub auth_token: Option<String>,

    /// Permits a non-loopback bind without an `auth_token`.
    pub allow_unauthenticated: bool,
}

impl std::fmt::Debug for HttpConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("allowed_origins", &self.allowed_origins)
            .field("allowed_hosts", &self.allowed_hosts)
            .field("auth_token", &self.auth_token.as_ref().map(|_| "[REDACTED]"))
            .field("allow_unauthenticated", &self.allow_unauthenticated)
            .finish()
    }
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

    /// Returns `true` when `host` only accepts connections from this machine.
    ///
    /// Loopback IP literals and `localhost` count as loopback. Any other
    /// host name, and the unspecified addresses (`0.0.0.0`, `::`), do not.
    #[must_use]
    pub fn is_loopback(&self) -> bool {
        let host = self.host.trim();
        let host = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')).unwrap_or(host);
        if host.eq_ignore_ascii_case("localhost") {
            return true;
        }
        host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    /// Validates this configuration, accumulating every rule violation.
    ///
    /// Rules enforced:
    /// - `host` MUST NOT be empty or whitespace-only after trim.
    /// - `auth_token`, when set, MUST NOT be empty or whitespace-only.
    /// - A non-loopback `host` MUST have an `auth_token` unless
    ///   `allow_unauthenticated` is set.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] containing one [`ConfigError`] per failing rule.
    pub fn validate(&self) -> Result<(), ConfigErrors> {
        let mut errors = Vec::new();
        if self.host.trim().is_empty() {
            errors.push(ConfigError::EmptyHttpHost);
        }
        if self.auth_token.as_deref().is_some_and(|t| t.trim().is_empty()) {
            errors.push(ConfigError::EmptyHttpAuthToken);
        }
        if !self.host.trim().is_empty()
            && !self.is_loopback()
            && self.auth_token.is_none()
            && !self.allow_unauthenticated
        {
            errors.push(ConfigError::UnauthenticatedNonLoopbackBind(self.host.clone()));
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
            auth_token: None,
            allow_unauthenticated: false,
        }
    }

    fn is_unauthenticated_bind(e: &ConfigError) -> bool {
        matches!(e, ConfigError::UnauthenticatedNonLoopbackBind(_))
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

    #[test]
    fn loopback_hosts_validate_without_token() {
        for host in ["127.0.0.1", "127.0.0.2", "::1", "[::1]", "localhost", "LOCALHOST"] {
            let config = config_with_host(host);
            assert!(config.is_loopback(), "{host} must be loopback");
            config.validate().expect("loopback host must validate without token");
        }
    }

    #[test]
    fn non_loopback_hosts_require_token() {
        for host in ["0.0.0.0", "::", "192.168.1.5", "example.com"] {
            let config = config_with_host(host);
            assert!(!config.is_loopback(), "{host} must not be loopback");
            let errors = config
                .validate()
                .expect_err("non-loopback bind without token must fail");
            assert!(errors.iter().any(is_unauthenticated_bind), "{host}");
        }
    }

    #[test]
    fn non_loopback_host_validates_with_token() {
        let config = HttpConfig {
            auth_token: Some("secret".into()),
            ..config_with_host("0.0.0.0")
        };
        config.validate().expect("token must allow non-loopback bind");
    }

    #[test]
    fn non_loopback_host_validates_with_opt_out() {
        let config = HttpConfig {
            allow_unauthenticated: true,
            ..config_with_host("0.0.0.0")
        };
        config.validate().expect("opt-out must allow non-loopback bind");
    }

    #[test]
    fn validate_rejects_blank_token() {
        for token in ["", "   "] {
            let config = HttpConfig {
                auth_token: Some(token.into()),
                ..config_with_host(HttpConfig::DEFAULT_HOST)
            };
            let errors = config.validate().expect_err("blank token must fail");
            assert!(errors.iter().any(|e| matches!(e, ConfigError::EmptyHttpAuthToken)));
        }
    }

    #[test]
    fn debug_redacts_token() {
        let config = HttpConfig {
            auth_token: Some("super-secret-token".into()),
            ..config_with_host(HttpConfig::DEFAULT_HOST)
        };
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("super-secret-token"), "{rendered}");
        assert!(rendered.contains("[REDACTED]"), "{rendered}");
    }
}
