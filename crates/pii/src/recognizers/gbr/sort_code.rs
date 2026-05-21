//! `SORT_CODE_UK` recognizer (weak digit pattern, keyword-context gated).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Context keywords for UK sort code.
const CONTEXT: &[&str] = &["sort", "sortcode"];

/// Build the `SORT_CODE_UK` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction.
#[must_use]
pub fn sort_code_gbr() -> Recognizer {
    let pattern = Pattern::new(
        "UK sort code",
        r"\b\d{2}[- ]?\d{2}[- ]?\d{2}\b",
        Score::from_static(0.05),
    )
    .expect("static UK sort-code pattern compiles");
    Recognizer::new(Entity::SortCodeUk, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("SortCodeGbrRecognizer")
        .with_category(Category::Financial)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, sort_code_gbr};

    #[test]
    fn carries_context_list() {
        assert_eq!(sort_code_gbr().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        sort_code_gbr()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_sort_code_gbr() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("sort 12-34-56", &[("12-34-56", 0.05)]),
            ("sort code 12 34 56", &[("12 34 56", 0.05)]),
            ("123456", &[("123456", 0.05)]),
            ("483163", &[("483163", 0.05)]),
            ("2021-10", &[("2021-10", 0.05)]),
            ("12345", &[]),
            ("1234567", &[]),
            ("1234567890", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
