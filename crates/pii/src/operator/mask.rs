//! `Mask` operator: replace code points with `masking_char`.

use super::ChunkCount;

pub(crate) fn apply(candidate: &str, masking_char: char, chars_to_mask: ChunkCount, from_end: bool) -> String {
    let total = candidate.chars().count();
    let to_mask = match chars_to_mask {
        ChunkCount::All => total,
        ChunkCount::N(n) => n.min(total),
    };
    if to_mask == 0 {
        return candidate.to_owned();
    }
    let keep = total - to_mask;
    candidate
        .chars()
        .enumerate()
        .map(|(i, ch)| {
            let mask_this = if from_end { i >= keep } else { i < to_mask };
            if mask_this { masking_char } else { ch }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{ChunkCount, apply};

    #[test]
    fn defaults_full_span_from_end() {
        let out = apply("4111-1111-1111-1111", '*', ChunkCount::All, true);
        assert_eq!(out, "*******************");
    }

    #[test]
    fn n_from_end_keeps_prefix() {
        let out = apply("4111-1111-1111-1111", '*', ChunkCount::N(12), true);
        assert_eq!(out, "4111-11************");
        assert_eq!(out.chars().count(), 19);
    }

    #[test]
    fn n_from_start_keeps_suffix() {
        let out = apply("4111111111111111", '*', ChunkCount::N(12), false);
        assert_eq!(out, "************1111");
    }

    #[test]
    fn n_clamps_to_span_length() {
        let out = apply("abcd", '*', ChunkCount::N(99), true);
        assert_eq!(out, "****");
    }

    #[test]
    fn unicode_preserves_codepoints() {
        // 4 code points, 8 bytes (CJK).
        let out = apply("公司机密", '*', ChunkCount::All, true);
        assert_eq!(out, "****");
    }
}
