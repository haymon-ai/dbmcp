//! US SSN validator filtering reserved area, group, and serial values.

use crate::recognizer::{ValidationOutcome, Validator};

/// US Social Security Number validator. Rejects reserved area / group / serial values
/// — replaces the negative-lookahead constructs Presidio's regex used.
#[derive(Debug, Default, Clone, Copy)]
pub struct UsSsnValidator;

impl Validator for UsSsnValidator {
    fn validate(&self, candidate: &str) -> ValidationOutcome {
        let Some(digits) = collect_digits::<9>(candidate) else {
            return ValidationOutcome::Invalid;
        };
        let area = digits[0] * 100 + digits[1] * 10 + digits[2];
        let group = digits[3] * 10 + digits[4];
        let serial = digits[5] * 1000 + digits[6] * 100 + digits[7] * 10 + digits[8];
        let valid = area != 0 && area != 666 && area < 900 && group != 0 && serial != 0;
        ValidationOutcome::from_bool(valid)
    }
}

/// Collect exactly `N` ASCII digits from `candidate`; returns `None` for any other count.
///
/// Iterates bytes (not chars) since every candidate that reaches a numeric
/// validator is ASCII-only post-regex-match.
fn collect_digits<const N: usize>(candidate: &str) -> Option<[u32; N]> {
    let mut out = [0u32; N];
    let mut i = 0usize;
    for &b in candidate.as_bytes() {
        if !b.is_ascii_digit() {
            continue;
        }
        if i == N {
            return None;
        }
        out[i] = u32::from(b - b'0');
        i += 1;
    }
    (i == N).then_some(out)
}
