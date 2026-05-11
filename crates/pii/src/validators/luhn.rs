//! Luhn checksum validator for credit-card numbers.

use crate::ValidationOutcome;

/// Luhn checksum validator for credit-card numbers.
///
/// Strips spaces and dashes before checking.
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    // Buffer fits the longest valid card (19 digits); avoids a heap allocation.
    // Iterates bytes — credit-card candidates are ASCII-only after the regex match,
    // so the `chars()` decode is unnecessary work.
    let mut digits = [0u32; 19];
    let mut len = 0usize;
    for &b in candidate.as_bytes() {
        if !b.is_ascii_digit() {
            continue;
        }
        if len == digits.len() {
            return ValidationOutcome::Invalid;
        }
        digits[len] = u32::from(b - b'0');
        len += 1;
    }
    ValidationOutcome::from_bool((12..=19).contains(&len) && luhn_passes(digits[..len].iter().copied()))
}

/// Returns true iff the right-to-left Luhn weighted sum over `digits` is divisible by 10.
pub(super) fn luhn_passes<I: IntoIterator<Item = u32>>(digits: I) -> bool
where
    I::IntoIter: DoubleEndedIterator,
{
    let sum: u32 = digits
        .into_iter()
        .rev()
        .enumerate()
        .map(|(i, d)| {
            if i.is_multiple_of(2) {
                d
            } else {
                let n = d * 2;
                if n > 9 { n - 9 } else { n }
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn luhn_valid_visa() {
        assert_eq!(validate("4111-1111-1111-1111"), ValidationOutcome::Valid);
    }

    #[test]
    fn luhn_invalid_visa() {
        assert_eq!(validate("4111-1111-1111-1112"), ValidationOutcome::Invalid);
    }

    #[test]
    fn luhn_rejects_short() {
        assert_eq!(validate("4111111"), ValidationOutcome::Invalid);
    }
}
