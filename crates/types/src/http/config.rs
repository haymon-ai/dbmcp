//! HTTP transport binding and security settings.

use std::net::IpAddr;

use secrecy::SecretString;

/// HTTP transport binding and security settings.
#[derive(Clone, Debug)]
pub struct Config {
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

impl Config {
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
}
