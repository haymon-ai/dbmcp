//! Config-file deserialization for [`clap::ValueEnum`] types.
//!
//! File values use the exact kebab-case tokens the CLI accepts, matched
//! case-sensitively. The CLI is at least as lenient (`--log-level` also
//! ignores case), so a value valid in the file is valid in every layer.

use clap::ValueEnum;
use serde::de::{Deserialize, Deserializer, Error};

/// Parses `value` as one of `T`'s possible values, case-sensitively.
fn parse<T: ValueEnum, E: Error>(value: &str) -> Result<T, E> {
    T::from_str(value, false).map_err(|_| {
        let names: Vec<String> = T::value_variants()
            .iter()
            .filter_map(ValueEnum::to_possible_value)
            .map(|value| format!("`{}`", value.get_name()))
            .collect();
        E::custom(format!("expected one of {}", names.join(", ")))
    })
}

/// Implements [`Deserialize`] for each listed [`ValueEnum`] type.
macro_rules! deserialize_value_enum {
    ($($ty:ty),* $(,)?) => {$(
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                parse(&String::deserialize(deserializer)?)
            }
        }
    )*};
}

deserialize_value_enum!(
    crate::DatabaseBackend,
    crate::LogLevel,
    crate::PiiCategory,
    crate::PiiOperator,
    crate::StrictHostKeyChecking,
);
