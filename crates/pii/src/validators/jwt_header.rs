//! JWT header structural validator (signature NOT verified).

use base64::Engine;
use serde::Deserialize;

use crate::ValidationOutcome;

#[derive(Deserialize)]
struct Header {
    alg: Option<String>,
}

/// JWT header validator.
///
/// Accepts the candidate iff splitting on `.` yields three segments and the
/// first segment base64url-decodes to a JSON object containing a string `alg`
/// field. Does NOT verify the signature.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    let mut parts = candidate.split('.');
    let (Some(header), Some(_), Some(_), None) = (parts.next(), parts.next(), parts.next(), parts.next()) else {
        return ValidationOutcome::Invalid;
    };
    let Ok(decoded) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(header) else {
        return ValidationOutcome::Invalid;
    };
    let Ok(parsed) = serde_json::from_slice::<Header>(&decoded) else {
        return ValidationOutcome::Invalid;
    };
    ValidationOutcome::from_bool(parsed.alg.is_some())
}
