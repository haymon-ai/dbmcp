//! Bitcoin address checksum validator: `Base58Check` (P2PKH/P2SH) and Bech32/Bech32m (segwit).

use super::prelude::*;

/// Validate a Bitcoin address candidate by checksum.
///
/// `1`/`3` prefix → `Base58Check` (verified via `bs58::with_check`);
/// `bc1` prefix → Bech32/Bech32m (verified via `bech32::decode`);
/// anything else (including ETH `0x...`) returns [`ValidationOutcome::Unknown`].
pub(super) fn validate(candidate: &str) -> ValidationOutcome {
    if candidate.starts_with("bc1") {
        return ValidationOutcome::from_bool(bech32_passes(candidate));
    }
    match candidate.as_bytes().first() {
        Some(&b'1') => ValidationOutcome::from_bool(base58check_passes(candidate, 0x00)),
        Some(&b'3') => ValidationOutcome::from_bool(base58check_passes(candidate, 0x05)),
        _ => ValidationOutcome::Unknown,
    }
}

fn base58check_passes(candidate: &str, version: u8) -> bool {
    bs58::decode(candidate).with_check(Some(version)).into_vec().is_ok()
}

fn bech32_passes(candidate: &str) -> bool {
    matches!(bech32::decode(candidate), Ok((hrp, _)) if hrp.as_str() == "bc")
}

#[cfg(test)]
mod tests {
    use super::validate;
    use crate::ValidationOutcome;

    #[test]
    fn base58check_p2pkh_valid() {
        assert_eq!(validate("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ"), ValidationOutcome::Valid);
    }

    #[test]
    fn base58check_p2sh_valid() {
        assert_eq!(validate("3J98t1WpEZ73CNmQviecrnyiWrnqRhWNLy"), ValidationOutcome::Valid);
    }

    #[test]
    fn base58check_invalid_checksum() {
        assert_eq!(
            validate("16Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ2"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn base58check_rejects_non_alphabet_char() {
        // `0` is in the recognizer regex char class but not in Base58.
        assert_eq!(
            validate("10Yeky6GMjeNkAiNcBY7ZhrLoMSgg1BoyZ"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn bech32_valid() {
        assert_eq!(
            validate("bc1qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"),
            ValidationOutcome::Valid
        );
    }

    #[test]
    fn bech32m_valid() {
        assert_eq!(
            validate("bc1p5d7rjq7g6rdk2yhzks9smlaqtedr4dekq08ge8ztwac72sfr9rusxg3297"),
            ValidationOutcome::Valid
        );
    }

    #[test]
    fn bech32_rejects_mixed_case() {
        assert_eq!(
            validate("bc1Qar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn bech32_rejects_charset_violation() {
        // `b` is not in BECH32_CHARSET.
        assert_eq!(
            validate("bc1bar0srrr7xfkvy5l643lydnw9re59gtzzwf5mdq"),
            ValidationOutcome::Invalid
        );
    }

    #[test]
    fn eth_address_abstains() {
        assert_eq!(
            validate("0x52908400098527886E0F7030069857D2E4169EE7"),
            ValidationOutcome::Unknown
        );
    }
}
