//! `CRYPTO` recognizer for BTC and ETH wallet addresses.
//!
//! No checksum validator (`Base58Check` / `EIP-55`) yet — future work.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `CRYPTO` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn crypto() -> Recognizer {
    let s = Score::from_static(0.5);
    let patterns = vec![
        Pattern::new("BTC (legacy / SegWit-P2SH)", r"\b[13][a-km-zA-HJ-NP-Z1-9]{25,34}\b", s).expect("BTC compiles"),
        Pattern::new("ETH", r"\b0x[a-fA-F0-9]{40}\b", s).expect("ETH compiles"),
    ];
    Recognizer::new(Entity::Crypto, patterns)
        .expect("non-empty pattern list")
        .with_name("CryptoRecognizer")
        .with_category(Category::Crypto)
}
