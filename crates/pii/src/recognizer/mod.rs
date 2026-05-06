//! Recognizer abstraction, entity-type newtype, validator hook, and built-in registry.

mod category;
mod types;
mod validator;

pub mod entity;
pub mod pattern;

pub use category::{Category, ParseCategoryError};
pub use pattern::Pattern;
pub use types::{EntityType, Recognizer, ValidationOutcome, Validator};
pub use validator::{
    AndValidator, IbanValidator, IpAddressValidator, KeywordValidator, LuhnValidator, NoopValidator, UsSsnValidator,
};
