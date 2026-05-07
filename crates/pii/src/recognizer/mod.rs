//! Rule-driven recognizer, entity-type newtype, validator hook, and built-in registry.

mod category;
mod types;
mod validator;

pub mod entity;
pub mod rule;

pub use category::{Category, ParseCategoryError};
pub use rule::Rule;
pub use types::{EntityType, ValidationOutcome};
pub use validator::{KeywordValidator, Validator};
