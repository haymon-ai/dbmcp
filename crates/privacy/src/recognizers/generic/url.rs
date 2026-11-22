//! `URL` recognizer.

use crate::recognizers::prelude::*;

/// Context keywords for URLs.
const CONTEXT: &[&str] = &["url", "website", "link"];

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
        .with_context(CONTEXT)
}

#[cfg(test)]
mod tests {
    use super::{CONTEXT, url};

    #[test]
    fn carries_context_list() {
        assert_eq!(url().context(), CONTEXT);
    }

    fn results(text: &str) -> Vec<(&str, f32)> {
        url()
            .analyze(text)
            .into_iter()
            .map(|r| (&text[r.start..r.end], r.score.as_f32()))
            .collect()
    }

    #[test]
    fn recognizes_url() {
        let cases: &[(&str, &[(&str, f32)])] = &[
            ("https://www.haymon.ai/", &[("https://www.haymon.ai", 0.5)]),
            ("http://www.haymon.ai/", &[("http://www.haymon.ai", 0.5)]),
            ("http://www.haymon.ai", &[("http://www.haymon.ai", 0.5)]),
            ("http://haymon.ai", &[("http://haymon.ai", 0.5)]),
            ("http://haymon.site", &[("http://haymon.site", 0.5)]),
            ("http://haymon.webcam", &[("http://haymon.webcam", 0.5)]),
            ("http://haymon.vlaanderen", &[("http://haymon.vlaanderen", 0.5)]),
            (
                "https://webhook.site/a8eedfd6-9d8a-44e0-b0fc-cc7d517db5dc?q=1&b=2",
                &[("https://webhook.site/a8eedfd6-9d8a-44e0-b0fc-cc7d517db5dc?q=1&b=2", 0.5)],
            ),
            (
                "https://www.haymon.ai/store/abc/",
                &[("https://www.haymon.ai/store/abc", 0.5)],
            ),
            ("Visit https://www.haymon.ai/ today", &[("https://www.haymon.ai", 0.5)]),
            (
                "see https://www.haymon.ai/ and http://docs.haymon.ai/",
                &[("https://www.haymon.ai", 0.5), ("http://docs.haymon.ai", 0.5)],
            ),
            ("haymon.ai", &[]),
            ("www.haymon.ai", &[]),
            ("", &[]),
        ];
        for (input, expected) in cases {
            assert_eq!(results(input), expected.to_vec(), "input {input:?}");
        }
    }
}
