//! Row-to-JSON conversion trait.

use serde_json::Value;

/// Converts a single database row into a JSON object.
pub trait RowExt {
    /// Converts this row's columns to a JSON object.
    ///
    /// Each column becomes a key in the returned object, with values
    /// converted to the most appropriate JSON type. `NULL` columns
    /// produce [`Value::Null`].
    ///
    /// # Returns
    ///
    /// A [`Value::Object`] where keys are column names and values are
    /// type-appropriate JSON values.
    fn to_json(&self) -> Value;
}
