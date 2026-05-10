//! `MAC_ADDRESS` recognizer.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `MAC_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn mac_address() -> Recognizer {
    let pattern = Pattern::new(
        "MAC (colon/dash)",
        r"(?i)\b(?:[0-9A-F]{2}[:-]){5}[0-9A-F]{2}\b",
        Score::from_static(0.5),
    )
    .expect("static MAC pattern compiles");
    Recognizer::new(Entity::MacAddress, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("MacAddressRecognizer")
        .with_category(Category::Network)
}

#[cfg(test)]
mod tests {
    use super::mac_address;

    fn matches(text: &str) -> Vec<String> {
        let r = mac_address();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    #[test]
    fn positive_colon() {
        assert_eq!(matches("interface 01:23:45:AB:CD:EF"), vec!["01:23:45:AB:CD:EF"]);
    }

    #[test]
    fn positive_dash() {
        assert_eq!(matches("nic 01-23-45-ab-cd-ef present"), vec!["01-23-45-ab-cd-ef"]);
    }

    #[test]
    fn negative_too_short() {
        assert!(matches("01:23:45:AB:CD").is_empty());
    }

    #[test]
    fn seven_octets_still_match_first_six() {
        // Regex anchors on \b before/after the 6-octet token; the trailing `:01`
        // does not break the first match. Documented: oversized run yields the
        // legal 6-octet span at the head.
        assert_eq!(matches("01:23:45:AB:CD:EF:01"), vec!["01:23:45:AB:CD:EF"]);
    }
}
