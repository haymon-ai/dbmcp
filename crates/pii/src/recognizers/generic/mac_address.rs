//! `MAC_ADDRESS` recognizer.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

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
    use super::mac_address;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        mac_address()
            .analyze(text)
            .into_iter()
            .map(|r| (r.start, r.end))
            .collect()
    }

    #[test]
    fn recognizes_mac_address() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("interface 01:23:45:AB:CD:EF", &[(10, 27)]),
            ("nic 01-23-45-ab-cd-ef present", &[(4, 21)]),
            ("01-23-45-AB-CD-EF", &[(0, 17)]),
            ("dev1 00:11:22:33:44:55 dev2 aa-bb-cc-dd-ee-ff", &[(5, 22), (28, 45)]),
            ("01:23:45:AB:CD:EF:01", &[(0, 17)]),
            ("01:23:45:AB:CD", &[]),
            ("01:23:45:AB:CD:GG", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
