//! HTTP transport binding and security settings.

use std::net::IpAddr;

use crate::error::{ConfigError, ConfigErrors};
use crate::origin;
use secrecy::{ExposeSecret, SecretString};

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

    /// Bearer token required on every request; `None` disables authentication.
    pub auth_token: Option<SecretString>,

    /// Permits a non-loopback bind without an `auth_token`.
    pub allow_unauthenticated: bool,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            host: Self::DEFAULT_HOST.into(),
            port: Self::DEFAULT_PORT,
            allowed_origins: Self::default_allowed_origins(),
            allowed_hosts: Self::default_allowed_hosts(),
            auth_token: None,
            allow_unauthenticated: false,
        }
    }
}

impl HttpConfig {
    /// Default HTTP bind host.
    pub const DEFAULT_HOST: &'static str = "127.0.0.1";
    /// Default HTTP bind port.
    pub const DEFAULT_PORT: u16 = 9001;

    /// Return default allowed CORS origins.
    ///
    /// Covers every loopback host in [`Self::default_allowed_hosts`], each
    /// with the `:*` port wildcard so local browser clients on any port
    /// (e.g. `http://localhost:5173`) are accepted.
    #[must_use]
    pub fn default_allowed_origins() -> Vec<String> {
        vec![
            "http://localhost:*".into(),
            "http://127.0.0.1:*".into(),
            "http://[::1]:*".into(),
            "https://localhost:*".into(),
            "https://127.0.0.1:*".into(),
            "https://[::1]:*".into(),
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
    /// - Every `allowed_origins` entry MUST parse as an origin
    ///   (`<scheme>://<host>[:<port>|:*]` or `null`).
    /// - `allowed_origins` and `allowed_hosts` MUST NOT contain empty or
    ///   duplicate entries.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigErrors`] containing one [`ConfigError`] per failing rule.
    pub fn validate(&self) -> Result<(), ConfigErrors> {
        let mut errors = Vec::new();
        if self.host.trim().is_empty() {
            errors.push(ConfigError::EmptyHttpHost);
        }
        if self
            .auth_token
            .as_ref()
            .is_some_and(|t| t.expose_secret().trim().is_empty())
        {
            errors.push(ConfigError::EmptyHttpAuthToken);
        }
        if !self.host.trim().is_empty()
            && !self.is_loopback()
            && self.auth_token.is_none()
            && !self.allow_unauthenticated
        {
            errors.push(ConfigError::UnauthenticatedNonLoopbackBind(self.host.clone()));
        }
        let blank = |entry: &String| entry.trim().is_empty();
        errors.extend(ConfigError::check_list(
            "http.allowed_origins",
            &self.allowed_origins,
            blank,
        ));
        errors.extend(
            self.allowed_origins
                .iter()
                .filter(|entry| !blank(entry) && !origin::is_valid_entry(entry))
                .map(|entry| ConfigError::InvalidAllowedOrigin(entry.clone())),
        );
        errors.extend(ConfigError::check_list(
            "http.allowed_hosts",
            &self.allowed_hosts,
            blank,
        ));
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
    fn validate_rejects_each_malformed_origin() {
        let config = HttpConfig {
            allowed_origins: vec![
                "http://localhost:*".into(),
                "localhost".into(),
                String::new(),
                "https://app.example:443:*".into(),
            ],
            ..config_with_host(HttpConfig::DEFAULT_HOST)
        };
        let errors = config.validate().expect_err("malformed origins must fail");
        let rejected: Vec<&str> = errors
            .iter()
            .filter_map(|e| match e {
                ConfigError::InvalidAllowedOrigin(entry) => Some(entry.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(rejected, ["localhost", "https://app.example:443:*"]);
    }

    #[test]
    fn validate_rejects_empty_and_duplicate_list_entries() {
        let config = HttpConfig {
            allowed_origins: vec!["http://a.example:*".into(), " ".into(), "http://a.example:*".into()],
            allowed_hosts: vec!["localhost".into(), "localhost".into(), String::new()],
            ..config_with_host(HttpConfig::DEFAULT_HOST)
        };
        let errors: Vec<String> = config
            .validate()
            .expect_err("bad lists must fail")
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(
            errors,
            [
                "http.allowed_origins: must not contain empty entries",
                "http.allowed_origins: contains duplicate entry 'http://a.example:*'",
                "http.allowed_hosts: must not contain empty entries",
                "http.allowed_hosts: contains duplicate entry 'localhost'",
            ]
        );
    }

    #[test]
    fn validate_accepts_empty_origin_list() {
        let config = HttpConfig {
            allowed_origins: Vec::new(),
            ..config_with_host(HttpConfig::DEFAULT_HOST)
        };
        config.validate().expect("empty list disables origin checks");
    }

    #[test]
    fn validate_accepts_origin_forms() {
        let config = HttpConfig {
            allowed_origins: vec![
                "null".into(),
                "https://app.example".into(),
                "https://app.example:8443".into(),
                "http://[::1]:*".into(),
            ],
            ..config_with_host(HttpConfig::DEFAULT_HOST)
        };
        config.validate().expect("valid origins must pass");
    }
}
