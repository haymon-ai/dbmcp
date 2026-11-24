//! Privacy vocabulary (entities, categories) and privacy settings.

mod category;
mod config;
mod entity;

pub use category::Category;
pub use config::{ColumnRule, Config, Mode, Operator};
pub use entity::{Entity, ParseEntityError};
