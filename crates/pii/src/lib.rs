//! PII analyzer and anonymizer for `dbmcp`.
//!
//! Library-only crate. Ports Presidio's language-agnostic recognizer and
//! anonymizer pipeline to Rust: regex/pattern recognition with optional
//! checksum validation, plus four built-in operators (`Replace`, `Mask`,
//! `Redact`, `Hash`). No NLP, no LLM, no network. Not wired into the MCP
//! server in this iteration.
//!
//! # Quickstart
//!
//! ```
//! use dbmcp_pii::{AnalyzeOptions, Analyzer, OperatorConfig, anonymize};
//!
//! let analyzer = Analyzer::with_defaults();
//! let text = "ping me at jane.doe@example.com";
//! let results = analyzer.analyze(text, &AnalyzeOptions::default());
//! let out = anonymize(text, results, &OperatorConfig::default());
//! assert_eq!(out.text, "ping me at <EMAIL_ADDRESS>");
//! ```

#![deny(missing_docs)]

pub mod analyzer;
pub mod anonymizer;
pub mod error;
pub mod operator;
pub mod overlap;
pub mod recognizer;
pub mod redact;
pub mod regex;
pub mod result;
pub mod score;

pub use crate::analyzer::{AnalyzeOptions, Analyzer};
pub use crate::anonymizer::{AnonymizedText, OperatorConfig, anonymize};
pub use crate::error::{AnalyzerBuildError, OperatorError, PatternError, RecognizerError};
pub use crate::operator::{ChunkCount, HashAlgorithm, Operator, OperatorKind};
pub use crate::recognizer::{
    AndValidator, Category, EntityType, KeywordValidator, ParseCategoryError, Pattern, Recognizer, ValidationOutcome,
    Validator,
};
pub use crate::redact::{RedactionError, RedactionStats, Redactor};
pub use crate::regex::Regex;
pub use crate::result::{AnalysisExplanation, OperatorResult, RecognizerResult};
pub use crate::score::{MAX_SCORE, MIN_SCORE, Score};

pub use crate::recognizer::entity;
