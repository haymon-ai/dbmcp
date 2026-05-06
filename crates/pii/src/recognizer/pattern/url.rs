//! `URL` recognizer.

use crate::recognizer::{Category, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `URL` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score constant is rejected at construction.
#[must_use]
pub fn url() -> Pattern {
    let pattern = Regex::new(
        "URL (http/https)",
        r"\bhttps?://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+\b",
        Score::from_static(0.5),
    )
    .expect("static URL pattern compiles");
    Pattern::new(entity::URL, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("UrlRecognizer")
        .with_category(Category::Network)
}
