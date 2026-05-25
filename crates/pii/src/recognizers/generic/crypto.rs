//! `CRYPTO` recognizer for BTC (legacy / P2SH / Bech32 / Bech32m) and ETH wallet addresses.
//!
//! BTC checksums (`Base58Check` + Bech32/Bech32m) enforced via [`Validator::Crypto`].
//! ETH (`0x...`) candidates are unvalidated.

use crate::recognizers::prelude::*;

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
    use super::{CONTEXT, crypto};

    #[test]
    fn carries_context_list() {
        assert_eq!(crypto().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        crypto()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_crypto() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            (
                "16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ",
                &[("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ", 1.0)],
            ),
            (
                "3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
                &[("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", 1.0)],
            ),
            (
                "bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq",
                &[("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq", 1.0)],
            ),
            (
                "bc1p5d7rjq7g6rdk2yhzks9smlaqtedr4dekq08ge8ztwac72sfr9rusxg3297",
                &[("bc1p5d7rjq7g6rdk2yhzks9smlaqtedr4dekq08ge8ztwac72sfr9rusxg3297", 1.0)],
            ),
            (
                "16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ 3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy",
                &[
                    ("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ", 1.0),
                    ("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy", 1.0),
                ],
            ),
            (
                "my wallet address is: 16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ",
                &[("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ", 1.0)],
            ),
            ("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ2", &[]),
            ("my wallet address is: 16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ2", &[]),
            ("", &[]),
            ("8f953371d3e85eddb89b05ed6b9e680791055315c73e1025ab5dba7bb2aee189", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
