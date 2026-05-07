//! Digit and alphanumeric extraction helpers shared by validators.

/// Collect exactly `N` ASCII digits from `candidate`; returns `None` for any other count.
///
/// Iterates bytes (not chars) since every candidate that reaches a numeric
/// validator is ASCII-only post-regex-match.
pub(super) fn collect_digits<const N: usize>(candidate: &str) -> Option<[u32; N]> {
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

/// Collect alphanumeric bytes from `candidate` into a fixed-size buffer, uppercasing as we go.
///
/// Returns `(buffer, length)` on success, or `None` if more than `N` alphanumerics were seen.
pub(super) fn collect_upper_alnum<const N: usize>(candidate: &str) -> Option<([u8; N], usize)> {
    let mut buf = [0u8; N];
    let mut len = 0usize;
    for &b in candidate.as_bytes() {
        if !b.is_ascii_alphanumeric() {
            continue;
        }
        if len == N {
            return None;
        }
        buf[len] = b.to_ascii_uppercase();
        len += 1;
    }
    Some((buf, len))
}
