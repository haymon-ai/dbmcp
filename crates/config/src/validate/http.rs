//! Validation rules for the `http` section.

use dbmcp_types::http;

use crate::env;
use crate::error::{ConfigError, Diagnostic, duplicates, into_result};
use secrecy::ExposeSecret;

/// Validates this configuration, accumulating every rule violation.
///
/// Rules enforced:
/// - `host` MUST NOT be empty or whitespace-only after trim.
/// - `auth_token`, when set, MUST NOT be empty or whitespace-only.
/// - A non-loopback `host` MUST have an `auth_token` unless
///   `allow_unauthenticated` is set.
/// - Every `allowed_origins` entry MUST parse as an origin
///   (`<scheme>://<host>[:<port>|:*]` or `null`). An empty list is
///   allowed: it disables server-side Origin validation.
/// - `allowed_hosts` MUST NOT be empty.
/// - Neither list may repeat an entry.
///
/// # Errors
///
/// Returns one [`Diagnostic`] per failing rule.
pub(crate) fn http(config: &http::Config) -> Result<(), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    if config.host.trim().is_empty() {
        errors.push(Diagnostic::new(ConfigError::EmptyHttpHost).involving(&[env::HTTP_HOST]));
    }
    if config
        .auth_token
        .as_ref()
        .is_some_and(|t| t.expose_secret().trim().is_empty())
    {
        errors.push(Diagnostic::new(ConfigError::EmptyHttpAuthToken).involving(&[env::HTTP_AUTH_TOKEN]));
    }
    if !config.host.trim().is_empty()
        && !config.is_loopback()
        && config.auth_token.is_none()
        && !config.allow_unauthenticated
    {
        errors.push(
            Diagnostic::new(ConfigError::UnauthenticatedNonLoopbackBind(config.host.clone()))
                .involving(&[env::HTTP_HOST])
                .missing(&[env::HTTP_AUTH_TOKEN, env::HTTP_ALLOW_UNAUTHENTICATED]),
        );
    }
    errors.extend(
        config
            .allowed_origins
            .iter()
            .filter(|entry| !http::is_valid_origin(entry))
            .map(|entry| {
                Diagnostic::new(ConfigError::InvalidAllowedOrigin(entry.clone()))
                    .involving(&[env::HTTP_ALLOWED_ORIGINS])
            }),
    );
    if config.allowed_hosts.is_empty() {
        errors.push(
            Diagnostic::new(ConfigError::EmptyList(env::HTTP_ALLOWED_HOSTS.path)).involving(&[env::HTTP_ALLOWED_HOSTS]),
        );
    }
    errors.extend(duplicates(env::HTTP_ALLOWED_ORIGINS, &config.allowed_origins));
    errors.extend(duplicates(env::HTTP_ALLOWED_HOSTS, &config.allowed_hosts));
    into_result(errors)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn config_with_host(host: &str) -> http::Config {
        http::Config {
            host: host.into(),
            port: http::Config::DEFAULT_PORT,
            allowed_origins: http::Config::default_allowed_origins(),
            allowed_hosts: http::Config::default_allowed_hosts(),
            auth_token: None,
            allow_unauthenticated: false,
        }
    }

    fn is_unauthenticated_bind(e: &Diagnostic) -> bool {
        matches!(e.error, ConfigError::UnauthenticatedNonLoopbackBind(_))
    }

    #[test]
    fn default_config_validates_ok() {
        super::http(&config_with_host(http::Config::DEFAULT_HOST)).expect("default host must validate");
    }

    #[test]
    fn validate_rejects_empty_host() {
        let errors = super::http(&config_with_host("")).expect_err("empty host must fail");
        assert!(errors.iter().any(|e| matches!(&e.error, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn validate_rejects_whitespace_only_host() {
        let errors = super::http(&config_with_host("   ")).expect_err("whitespace host must fail");
        assert!(errors.iter().any(|e| matches!(&e.error, ConfigError::EmptyHttpHost)));
    }

    #[test]
    fn loopback_hosts_validate_without_token() {
        for host in ["127.0.0.1", "127.0.0.2", "::1", "[::1]", "localhost", "LOCALHOST"] {
            let config = config_with_host(host);
            assert!(config.is_loopback(), "{host} must be loopback");
            super::http(&config).expect("loopback host must validate without token");
        }
    }

    #[test]
    fn non_loopback_hosts_require_token() {
        for host in ["0.0.0.0", "::", "192.168.1.5", "example.com"] {
            let config = config_with_host(host);
            assert!(!config.is_loopback(), "{host} must not be loopback");
            let errors = super::http(&config).expect_err("non-loopback bind without token must fail");
            assert!(errors.iter().any(is_unauthenticated_bind), "{host}");
        }
    }

    #[test]
    fn non_loopback_host_validates_with_token() {
        let config = http::Config {
            auth_token: Some("secret".into()),
            ..config_with_host("0.0.0.0")
        };
        super::http(&config).expect("token must allow non-loopback bind");
    }

    #[test]
    fn non_loopback_host_validates_with_opt_out() {
        let config = http::Config {
            allow_unauthenticated: true,
            ..config_with_host("0.0.0.0")
        };
        super::http(&config).expect("opt-out must allow non-loopback bind");
    }

    #[test]
    fn validate_rejects_blank_token() {
        for token in ["", "   "] {
            let config = http::Config {
                auth_token: Some(token.into()),
                ..config_with_host(http::Config::DEFAULT_HOST)
            };
            let errors = super::http(&config).expect_err("blank token must fail");
            assert!(
                errors
                    .iter()
                    .any(|e| matches!(&e.error, ConfigError::EmptyHttpAuthToken))
            );
        }
    }

    #[test]
    fn validate_rejects_each_malformed_origin() {
        let config = http::Config {
            allowed_origins: vec![
                "http://localhost:*".into(),
                "localhost".into(),
                String::new(),
                "https://app.example:443:*".into(),
            ],
            ..config_with_host(http::Config::DEFAULT_HOST)
        };
        let errors = super::http(&config).expect_err("malformed origins must fail");
        let rejected: Vec<&str> = errors
            .iter()
            .filter_map(|e| match &e.error {
                ConfigError::InvalidAllowedOrigin(entry) => Some(entry.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(rejected, ["localhost", "", "https://app.example:443:*"]);
    }

    #[test]
    fn validate_accepts_origin_forms() {
        let config = http::Config {
            allowed_origins: vec![
                "null".into(),
                "https://app.example".into(),
                "https://app.example:8443".into(),
                "http://[::1]:*".into(),
            ],
            ..config_with_host(http::Config::DEFAULT_HOST)
        };
        super::http(&config).expect("valid origins must pass");
    }
}
