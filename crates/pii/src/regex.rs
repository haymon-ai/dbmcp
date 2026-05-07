//! Named regex with confidence score; eagerly compiled at construction.

use std::borrow::Cow;
use std::fmt;

use crate::error::PatternError;
use crate::score::Score;

/// Named regex pattern with a base confidence score.
///
/// Backed by the linear-time `regex` crate (RE2 semantics). Compiled eagerly
/// so a bad pattern is rejected at construction, not at match time.
#[derive(Clone)]
pub struct Regex {
    name: Cow<'static, str>,
    score: Score,
    pub(crate) compiled: regex::Regex,
}

impl fmt::Debug for Regex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Regex")
            .field("name", &self.name)
            .field("regex", &self.compiled.as_str())
            .field("score", &self.score)
            .finish_non_exhaustive()
    }
}

impl Regex {
    /// Build a pattern.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError::InvalidRegex`] when the source fails to compile.
    pub fn new(
        name: impl Into<Cow<'static, str>>,
        regex_src: impl AsRef<str>,
        score: Score,
    ) -> Result<Self, PatternError> {
        let compiled = regex::Regex::new(regex_src.as_ref()).map_err(|e| PatternError::InvalidRegex(Box::new(e)))?;
        Ok(Self {
            name: name.into(),
            score,
            compiled,
        })
    }

    /// Regex's human-readable name; surfaced in [`crate::AnalysisExplanation`].
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Regex name as a cheap-to-clone [`Cow`]; static literals stay borrowed.
    pub(crate) fn name_cow(&self) -> Cow<'static, str> {
        self.name.clone()
    }

    /// Regex source (the string the pattern was constructed with).
    #[must_use]
    pub fn regex(&self) -> &str {
        self.compiled.as_str()
    }

    /// Base confidence score, before any validator promotion.
    #[must_use]
    pub fn score(&self) -> Score {
        self.score
    }
}

#[cfg(test)]
mod tests {
    use super::Regex;
    use crate::error::PatternError;
    use crate::score::Score;

    fn s(v: f32) -> Score {
        Score::new(v).expect("valid score")
    }

    #[test]
    fn rejects_invalid_regex() {
        let err = Regex::new("bad", "(unclosed", s(0.5)).unwrap_err();
        assert!(matches!(err, PatternError::InvalidRegex(_)));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn accepts_valid_regex() {
        let p = Regex::new("digits", r"\b\d+\b", s(0.5)).unwrap();
        assert_eq!(p.score().as_f32(), 0.5);
    }

    #[test]
    fn rejects_lookbehind() {
        // The `regex` crate does not support lookbehind; the pattern is rejected.
        let err = Regex::new("bad_lb", r"(?<!a)b", s(0.5)).unwrap_err();
        assert!(matches!(err, PatternError::InvalidRegex(_)));
    }
}
