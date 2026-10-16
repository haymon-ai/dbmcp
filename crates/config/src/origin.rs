//! Browser `Origin` allowlist parsing and matching.
//!
//! [`HttpConfig::validate`](crate::HttpConfig::validate) uses the parser to
//! reject malformed entries; the HTTP transport's CORS layer uses
//! [`AllowedOrigins`] to match request origins. Mirrors the private `(scheme, host, port)` matching rules rmcp applies
//! in its Streamable HTTP Origin validator (rmcp 3.5), so the CORS preflight
//! layer and rmcp's server-side check share one interpretation of the
//! configured `allowed_origins` list. Re-check these rules when upgrading
//! rmcp: a port-less entry currently matches any port, but rmcp has
//! announced it will match only the scheme default port in a future release.
//! `cors_and_rmcp_agree_on_origin_matching` in the binary's HTTP command
//! tests fails when the two layers drift apart.

use ::http::{HeaderValue, Uri};

/// Parsed allowlist of browser origins.
#[derive(Debug)]
pub struct AllowedOrigins(Vec<Origin>);

impl AllowedOrigins {
    /// Parses each entry, skipping unparseable ones like rmcp does.
    ///
    /// [`HttpConfig::validate`](crate::HttpConfig::validate) rejects such
    /// entries, so a validated config never has any to skip.
    #[must_use]
    pub fn new(entries: &[String]) -> Self {
        Self(entries.iter().filter_map(|raw| parse_entry(raw)).collect())
    }

    /// Returns `true` when the `Origin` header value matches any allowlist entry.
    #[must_use]
    pub fn allows(&self, origin: &HeaderValue) -> bool {
        let Some(origin) = origin.to_str().ok().and_then(Origin::parse) else {
            return false;
        };
        self.0.iter().any(|allowed| allowed.matches(&origin))
    }
}

/// Normalized origin: the browser's `null` or a `(scheme, host, port)` tuple.
#[derive(Debug, PartialEq, Eq)]
enum Origin {
    Null,
    Tuple {
        scheme: String,
        host: String,
        port: Option<u16>,
    },
}

impl Origin {
    fn parse(value: &str) -> Option<Self> {
        let value = value.trim();
        if value.is_empty() {
            return None;
        }
        if value.eq_ignore_ascii_case("null") {
            return Some(Self::Null);
        }
        let uri = Uri::try_from(value).ok()?;
        let scheme = uri.scheme_str()?.to_ascii_lowercase();
        let authority = uri.authority()?;
        Some(Self::Tuple {
            scheme,
            host: authority.host().trim_matches(['[', ']']).to_ascii_lowercase(),
            port: authority.port_u16(),
        })
    }

    /// Matches a request origin against `self` used as an allowlist entry.
    ///
    /// An entry without a port (`:*` or port-less) matches any port; an
    /// explicit port also matches an origin that omits the scheme default.
    fn matches(&self, origin: &Self) -> bool {
        match (self, origin) {
            (Self::Null, Self::Null) => true,
            (
                Self::Tuple { scheme, host, port },
                Self::Tuple {
                    scheme: o_scheme,
                    host: o_host,
                    port: o_port,
                },
            ) => {
                scheme == o_scheme
                    && host == o_host
                    && port.is_none_or(|port| o_port.or_else(|| default_port(o_scheme)) == Some(port))
            }
            _ => false,
        }
    }
}

/// Returns `true` when `value` parses as an allowlist entry.
pub(crate) fn is_valid_entry(value: &str) -> bool {
    parse_entry(value).is_some()
}

/// Parses an allowlist entry; `:*` and a missing port both mean any port.
fn parse_entry(value: &str) -> Option<Origin> {
    let value = value.trim();
    let Some(base) = value.strip_suffix(":*") else {
        return Origin::parse(value);
    };
    match Origin::parse(base)? {
        origin @ Origin::Tuple { port: None, .. } => Some(origin),
        _ => None,
    }
}

/// Port a scheme implies when an origin omits it (RFC 6454 §4).
fn default_port(scheme: &str) -> Option<u16> {
    match scheme {
        "http" => Some(80),
        "https" => Some(443),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Matching semantics are covered against rmcp by the binary's
    // `cors_and_rmcp_agree_on_origin_matching`; these cover only inputs
    // that table cannot express.

    fn allows(entries: &[&str], origin: &str) -> bool {
        let entries: Vec<String> = entries.iter().map(ToString::to_string).collect();
        AllowedOrigins::new(&entries).allows(&HeaderValue::from_str(origin).expect("valid header value"))
    }

    #[test]
    fn malformed_entries_are_ignored() {
        for entry in ["", "not a url", "localhost", "null:*", "https://app.example:443:*"] {
            assert!(!allows(&[entry], "https://app.example"), "{entry}");
        }
    }

    #[test]
    fn malformed_origin_rejected() {
        assert!(!allows(&["http://localhost:*"], "not a url"));
        let origins = AllowedOrigins::new(&["http://localhost:*".into()]);
        let non_utf8 = HeaderValue::from_bytes(&[0xFF, 0xFE]).expect("non-utf8 header value");
        assert!(!origins.allows(&non_utf8));
    }

    #[test]
    fn empty_allowlist_rejects_everything() {
        assert!(!allows(&[], "http://localhost"));
    }
}
