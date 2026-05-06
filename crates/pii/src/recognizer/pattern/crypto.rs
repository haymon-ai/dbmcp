//! `CRYPTO` recognizer for BTC and ETH wallet addresses.
//!
//! No checksum validator (`Base58Check` / `EIP-55`) yet — future work.

use crate::recognizer::{Category, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `CRYPTO` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn crypto() -> Pattern {
    let s = Score::from_static(0.5);
    let patterns = vec![
        Regex::new("BTC (legacy / SegWit-P2SH)", r"\b[13][a-km-zA-HJ-NP-Z1-9]{25,34}\b", s).expect("BTC compiles"),
        Regex::new("ETH", r"\b0x[a-fA-F0-9]{40}\b", s).expect("ETH compiles"),
    ];
    Pattern::new(entity::CRYPTO, patterns)
        .expect("non-empty pattern list")
        .with_name("CryptoRecognizer")
        .with_category(Category::Crypto)
}
