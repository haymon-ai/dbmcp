//! PII analyzer and anonymizer for `dbmcp`.
//!
//! Library-only crate. Ports Presidio's language-agnostic recognizer and
//! anonymizer pipeline to Rust: regex/pattern recognition with optional
//! checksum validation, plus four built-in operators (`Replace`, `Mask`,
//! `Redact`, `Hash`). No NLP, no LLM, no network. Wired into the MCP
//! server's query tool output via [`Redactor`] behind `PiiConfig`.
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
#[cfg(feature = "test-support")]
pub mod corpus;
pub mod error;
pub mod operator;
pub mod overlap;
pub mod recognizer;
pub mod redact;
pub mod regex;
pub mod result;
pub mod score;

pub use crate::analyzer::{AnalyzeOptions, Analyzer};
pub use crate::anonymizer::{OperatorConfig, anonymize};
pub use crate::operator::{ChunkCount, HashAlgorithm, Operator};
pub use crate::recognizer::{Category, EntityType, ValidationOutcome, entity};
pub use crate::redact::{RedactionError, RedactionStats, Redactor};
pub use crate::result::{AnalysisExplanation, RecognizerResult};
pub use crate::score::{MAX_SCORE, Score};
