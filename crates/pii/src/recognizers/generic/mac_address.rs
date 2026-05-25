//! `MAC_ADDRESS` recognizer.

use crate::recognizers::prelude::*;

/// Context keywords for MAC addresses.
const CONTEXT: &[&str] = &["mac", "mac address", "hardware address", "physical address", "ethernet"];

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
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, mac_address};

    #[test]
    fn carries_context_list() {
        assert_eq!(mac_address().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        mac_address()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_mac_address() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("interface 01:23:45:AB:CD:EF", &[("01:23:45:AB:CD:EF", 0.5)]),
            ("nic 01-23-45-ab-cd-ef present", &[("01-23-45-ab-cd-ef", 0.5)]),
            ("01-23-45-AB-CD-EF", &[("01-23-45-AB-CD-EF", 0.5)]),
            (
                "dev1 00:11:22:33:44:55 dev2 aa-bb-cc-dd-ee-ff",
                &[("00:11:22:33:44:55", 0.5), ("aa-bb-cc-dd-ee-ff", 0.5)],
            ),
            ("01:23:45:AB:CD:EF:01", &[("01:23:45:AB:CD:EF", 0.5)]),
            ("01:23:45:AB:CD", &[]),
            ("01:23:45:AB:CD:GG", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
