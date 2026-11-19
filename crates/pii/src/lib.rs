//! PII analyzer and anonymizer for `dbmcp`.
//!
//! Library-only crate. Regex/pattern recognition with optional checksum
//! validation, plus four built-in operators (`Replace`, `Mask`, `Redact`,
//! `Hash`). `Hash` emits a keyed token `<{TYPE}_{16 hex}>` (HMAC-SHA-256),
//! and is what [`Redactor::from_config`] uses by default. An optional ML/NER pass detects person, location, organization,
//! NRP, and facility spans (which entities depend on the model's labels); it
//! is always compiled and enabled at runtime via `PiiConfig`. Inference runs
//! locally via ONNX Runtime with no network. Wired into the MCP server's
//! query tool output via [`Redactor`].
//!
//! # Quickstart
//!
//! ```
//! use dbmcp_pii::{AnalyzeOptions, Analyzer, Operator, anonymize};
//!
//! let analyzer = Analyzer::with_defaults();
//! let text = "ping me at jane.doe@example.com";
//! let results = analyzer.analyze(text, &AnalyzeOptions::default());
//! let out = anonymize(text, results, &Operator::Replace);
//! assert_eq!(out.text, "ping me at <EMAIL_ADDRESS>");
//! ```

#![deny(missing_docs)]

pub mod analyzer;
pub mod anonymizer;
pub mod category;
pub mod cells;
pub mod context;
pub mod entity;
pub mod error;
pub mod ner;
pub mod operators;
pub mod overlap;
pub mod pattern;
pub mod recognizers;
pub mod redact;
pub mod result;
pub mod score;
pub mod validation;
pub mod validators;
mod words;

pub use crate::analyzer::{AnalyzeOptions, Analyzer};
pub use crate::anonymizer::anonymize;
pub use crate::category::{Category, ParseCategoryError};
pub use crate::context::ContextSettings;
pub use crate::entity::{Entity, ParseEntityError};
pub use crate::ner::{NerEngine, NerError};
pub use crate::operators::{ChunkCount, Operator};
pub use crate::redact::{MaybeRedact, RedactionError, RedactionStats, Redactor, RedactorInitError};
pub use crate::result::{AnalysisExplanation, RecognizerResult};
pub use crate::score::{MAX_SCORE, Score};
pub use crate::validation::ValidationOutcome;
