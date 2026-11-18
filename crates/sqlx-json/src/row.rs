//! Shared row-to-JSON driver over the per-backend cell decoders.

use serde_json::Value;
use sqlx::{Column, ColumnIndex, Row, TypeInfo, ValueRef};

use crate::cell::{Cell, encode};

/// Decodes one non-`NULL` column of a backend row into a [`Cell`].
pub trait DecodeCell: Row {
    /// Decodes column `idx`, whose driver-reported type name is `type_name`.
    ///
    /// Never called for SQL `NULL` values. Returns `None` when the value
    /// cannot be decoded.
    fn decode_cell(&self, idx: usize, type_name: &str) -> Option<Cell>;
}

/// Walks a row's columns in order as `(name, value)` entries.
///
/// SQL `NULL` and undecodable values are [`Value::Null`].
pub(crate) fn entries<R>(row: &R) -> impl Iterator<Item = (String, Value)> + '_
where
    R: DecodeCell,
    usize: ColumnIndex<R>,
{
    row.columns().iter().map(move |column| {
        let idx = column.ordinal();
        let value = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
            Value::Null
        } else {
            row.decode_cell(idx, column.type_info().name())
                .map_or(Value::Null, encode)
        };
        (column.name().to_string(), value)
    })
}
