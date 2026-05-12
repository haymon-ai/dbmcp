//! `CRYPTO` recognizer for BTC (legacy / P2SH / Bech32 / Bech32m) and ETH wallet addresses.
//!
//! BTC checksums (`Base58Check` + Bech32/Bech32m) enforced via [`Validator::Crypto`].
//! ETH (`0x...`) candidates are unvalidated.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords for crypto wallet addresses.
const CONTEXT: &[&str] = &["wallet", "btc", "bitcoin", "crypto"];

/// Build the `CRYPTO` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn crypto() -> Recognizer {
    let s = Score::from_static(0.5);
    let patterns = vec![
        Pattern::new("Crypto (Medium)", r"\b(bc1|[13])[a-zA-HJ-NP-Z0-9]{25,59}\b", s).expect("BTC compiles"),
        Pattern::new("ETH", r"\b0x[a-fA-F0-9]{40}\b", s).expect("ETH compiles"),
    ];
    Recognizer::new(Entity::Crypto, patterns)
        .expect("non-empty pattern list")
        .with_name("CryptoRecognizer")
        .with_validator(Validator::Crypto)
        .with_category(Category::Crypto)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::crypto;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        crypto().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_crypto() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ", &[(0, 34)]),
            ("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", &[(0, 34)]),
            ("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", &[(0, 42)]),
            (
                "bc1p5d7rjq7g6rdk2yhzks9smlaqtedr4dekq08ge8ztwac72sfr9rusxg3297",
                &[(0, 62)],
            ),
            (
                "16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ 3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
                &[(0, 34), (35, 69)],
            ),
            ("my wallet address is: 16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ", &[(22, 56)]),
            ("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ2", &[]),
            ("my wallet address is: 16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ2", &[]),
            ("", &[]),
            ("8f953371d3e85eddb89b05ed6b9e680791055315c73e1025ab5dba7bb2aee189", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
