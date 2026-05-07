//! Error types for the `dbmcp-pii` crate.

use thiserror::Error;

/// Errors that surface when constructing a [`crate::Regex`] or [`crate::Score`].
#[derive(Debug, Error)]
pub enum PatternError {
    /// `regex`-engine compile error.
    #[error("invalid regex: {0}")]
    InvalidRegex(Box<regex::Error>),
    /// Score was non-finite or outside `[0.0, 1.0]`.
    #[error("invalid score: {value} (must be a finite value in [0.0, 1.0])")]
    InvalidScore {
        /// Offending input value.
        value: f32,
    },
}

/// Errors that surface when constructing a [`crate::Rule`].
#[derive(Debug, Error)]
pub enum RecognizerError {
    /// Caller supplied no regexes.
    #[error("recognizer requires at least one regex")]
    EmptyPatternList,
}

/// Errors that surface from [`crate::analyzer::Builder::build`].
#[derive(Debug, Error)]
pub enum AnalyzerBuildError {
    /// A requested category resolved to zero recognizers in the merged registry.
    #[error("category {0:?} requested but no recognizer in registry tags it")]
    EmptyCategory(crate::recognizer::Category),
}
