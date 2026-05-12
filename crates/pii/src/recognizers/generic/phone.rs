//! `PHONE_NUMBER` recognizer.
//!
//! Per-region candidate regex prefilter, gated by [`Validator::PhoneNational`]
//! cleaned-form grammar (E.164/US/UK/DE). Rejects bare leading-`0` digit
//! runs that aren't valid national-format phones (issue #147).

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::validators::Validator;
use crate::{Category, Entity};

/// Context keywords used by the boost step.
const CONTEXT: &[&str] = &["phone", "number", "telephone", "cell", "cellphone", "mobile", "call"];

/// Build the `PHONE_NUMBER` recognizer.
///
/// # Panics
///
/// Panics only if any bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn phone_number() -> Recognizer {
    let s = Score::from_static(0.4);
    let patterns = vec![
        Pattern::new("E.164", r"\+\d{8,15}\b", s).expect("E.164 compiles"),
        Pattern::new("US", r"[+(]?\b(?:1[\s-]?)?\d{3}\)?[\s-]?\d{3}[\s-]?\d{4}\b", s).expect("US compiles"),
        Pattern::new("UK", r"\+?\b(?:44[\s-]?)?0?[1-9](?:[\s-]?\d){8,9}\b", s).expect("UK compiles"),
        Pattern::new("DE", r"\+?\b(?:49[\s-]?)?0?[1-9](?:[\s-]?\d){7,11}\b", s).expect("DE compiles"),
    ];
    Recognizer::new(Entity::PhoneNumber, patterns)
        .expect("non-empty pattern list")
        .with_name("PhoneRecognizer")
        .with_validator(Validator::PhoneNational)
        .with_category(Category::Contact)
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, phone_number};
    use crate::analyzer::{AnalyzeOptions, Analyzer};
    use crate::context::{ContextMatchingMode, ContextSettings};
    use crate::score::Score;

    fn matches(text: &str) -> Vec<String> {
        let r = phone_number();
        r.analyze(text)
            .into_iter()
            .map(|res| text[res.start..res.end].to_string())
            .collect()
    }

    fn redacts(text: &str) -> bool {
        !matches(text).is_empty()
    }

    #[test]
    fn phone_recognizer_carries_context_list() {
        assert!(!phone_number().context().is_empty());
        assert_eq!(phone_number().context(), CONTEXT);
    }

    #[test]
    fn phone_context_boost_lifts_score() {
        let mut a = Analyzer::empty();
        a.register(phone_number());
        let opts = AnalyzeOptions {
            min_score: Score::default(),
            context: Some(ContextSettings {
                similarity_factor: Score::from_static(0.35),
                min_score_with_context: Score::from_static(0.4),
                prefix_words: 5,
                suffix_words: 0,
                matching_mode: ContextMatchingMode::WholeWord,
            }),
        };
        let out = a.analyze("my phone 415 555 0142", &opts);
        assert!(!out.is_empty());
        let r = &out[0];
        assert_eq!(r.explanation.supportive_keyword.as_deref(), Some("phone"));
    }

    #[test]
    fn issue_147_negatives_pass_through() {
        // The four reproduction strings from issue #147 must NOT be classified
        // as PHONE_NUMBER.
        assert!(!redacts("000-12-3456"));
        assert!(!redacts("07-1234567"));
        assert!(!redacts("046 454 287"));
        assert!(!redacts("01234567"));
    }

    #[test]
    fn issue_147_extra_negative_pass_through() {
        assert!(!redacts("0461234567"));
    }

    #[test]
    fn canonical_positives_redact() {
        assert!(redacts("+14155552671"));
        assert!(redacts("(415) 555-2671"));
        assert!(redacts("+44 20 7946 0958"));
        assert!(redacts("+49 30 12345678"));
    }

    #[test]
    fn uk_local_form_redacts() {
        assert!(redacts("02012345678"));
    }

    #[test]
    fn span_includes_leading_paren_and_plus() {
        // Leading `(` and `+` must be inside at least one match span — the
        // overlap-resolver later picks the longest. The previous bug left the
        // sigil outside every span, producing `(<PHONE_NUMBER>` /
        // `+<PHONE_NUMBER>` after redaction.
        for input in [
            "(415) 555-2671",
            "+44 20 7946 0958",
            "+49 30 12345678",
            "+14155552671",
            "02012345678",
        ] {
            let hits = matches(input);
            assert!(
                hits.iter().any(|h| h == input),
                "{input}: no full-span hit; got {hits:?}"
            );
        }
    }

    #[test]
    fn audit_distribution_count() {
        // FR / SC-003 / US3 — mixed batch: 10 phones, 10 leading-zero non-phones.
        let phones = [
            "+14155552671",
            "(415) 555-2671",
            "+44 20 7946 0958",
            "+49 30 12345678",
            "02012345678",
            "+1 415 555 2671",
            "415-555-2671",
            "+44 7700 900123",
            "+49 151 12345678",
            "(212) 555-0199",
        ];
        let non_phones = [
            "000-12-3456",
            "07-1234567",
            "046 454 287",
            "01234567",
            "0461234567",
            "00501",
            "01234",
            "012-34-5678",
            "0046123",
            "00012345",
        ];
        let phones_matched = phones.iter().filter(|s| redacts(s)).count();
        let non_phones_matched = non_phones.iter().filter(|s| redacts(s)).count();
        assert_eq!(
            phones_matched,
            phones.len(),
            "every phone string must surface at least one PHONE_NUMBER hit"
        );
        assert_eq!(
            non_phones_matched, 0,
            "no leading-zero ID may surface a PHONE_NUMBER hit"
        );
    }
}
