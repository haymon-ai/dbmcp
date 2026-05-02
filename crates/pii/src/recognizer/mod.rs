//! Recognizer abstraction, entity-type newtype, validator hook, and built-in registry.

mod deny_list;
mod pattern_recognizer;
mod types;
mod validators;

pub mod builtin;
pub mod entity;

pub use deny_list::deny_list_recognizer;
pub use pattern_recognizer::PatternRecognizer;
pub use types::{EntityType, Recognizer, ValidationOutcome, Validator};
pub use validators::{IbanValidator, IpAddressValidator, LuhnValidator, NoopValidator, UsSsnValidator};
