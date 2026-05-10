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
