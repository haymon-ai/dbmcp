//! `EMAIL_ADDRESS` recognizer.
//!
//! Pattern adapted from Presidio's `EmailRecognizer.PATTERNS["Email (Medium)"]`.

use crate::recognizer::{Category, Pattern, entity};
use crate::regex::Regex;
use crate::score::Score;

/// Build the `EMAIL_ADDRESS` recognizer.
///
/// # Panics
///
/// Panics only if the bundled regex source or score literal is rejected at construction;
/// both are unit-tested.
#[must_use]
pub fn email() -> Pattern {
    let pattern = Regex::new(
        "Email (Medium)",
        r"\b[A-Za-z0-9!#$%&'*+\-/=?^_`{|}~]+(?:\.[A-Za-z0-9!#$%&'*+\-/=?^_`{|}~]+)*@[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?)+\b",
        Score::from_static(0.5),
    )
    .expect("static email pattern compiles");
    Pattern::new(entity::EMAIL_ADDRESS, vec![pattern])
        .expect("non-empty pattern list")
        .with_name("EmailRecognizer")
        .with_category(Category::Personal)
}
