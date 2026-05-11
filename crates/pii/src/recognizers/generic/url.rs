//! `URL` recognizer.

use super::Recognizer;
use crate::pattern::Pattern;
use crate::score::Score;
use crate::{Category, Entity};

/// Build the `URL` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn url() -> Recognizer {
    let pattern = Pattern::new(
        "URL (http/https)",
        r"\bhttps?://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+\b",
        Score::from_static(0.5),
    )
    .expect("static URL pattern compiles");
    Recognizer::new(Entity::Url, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("UrlRecognizer")
        .with_category(Category::Network)
}

#[cfg(test)]
mod tests {
    use super::url;

    fn matches(text: &str) -> Vec<(usize, usize)> {
        url().analyze(text).into_iter().map(|r| (r.start, r.end)).collect()
    }

    #[test]
    fn recognizes_url() {
        let cases: &[(&str, &[(usize, usize)])] = &[
            ("https://www.haymon.ai/", &[(0, 21)]),
            ("http://www.haymon.ai/", &[(0, 20)]),
            ("http://www.haymon.ai", &[(0, 20)]),
            ("http://haymon.ai", &[(0, 16)]),
            ("http://haymon.site", &[(0, 18)]),
            ("http://haymon.webcam", &[(0, 20)]),
            ("http://haymon.vlaanderen", &[(0, 24)]),
            (
                "https://webhook.site/a8eedfd6-9d8a-44e0-b0fc-cc7d517db5dc?q=1&b=2",
                &[(0, 65)],
            ),
            ("https://www.haymon.ai/store/abc/", &[(0, 31)]),
            ("Visit https://www.haymon.ai/ today", &[(6, 27)]),
            (
                "see https://www.haymon.ai/ and http://docs.haymon.ai/",
                &[(4, 25), (31, 52)],
            ),
            ("haymon.ai", &[]),
            ("www.haymon.ai", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(matches(input), expected.to_vec(), "input {input:?}: span mismatch");
        }
    }
}
