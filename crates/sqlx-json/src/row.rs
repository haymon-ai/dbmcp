//! Shared row-to-JSON driver over the per-backend cell decoders.

use serde_json::{Map, Value};
use sqlx::{Column, ColumnIndex, Row, TypeInfo, ValueRef};

use crate::cell::{Cell, encode};

/// Decodes one non-`NULL` column of a backend row into a [`Cell`].
pub(crate) trait DecodeCell: Row {
    /// Decodes column `idx`, whose driver-reported type name is `type_name`.
    ///
    /// Never called for SQL `NULL` values; decode failures yield [`Cell::Null`].
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell;
}

/// Converts a row into a JSON object keyed by column name.
pub(crate) fn row_to_json<R>(row: &R) -> Value
where
    R: DecodeCell,
    usize: ColumnIndex<R>,
{
    let columns = row.columns();
    let mut map = Map::with_capacity(columns.len());

    for column in columns {
        let idx = column.ordinal();

        let cell = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
            Cell::Null
        } else {
            row.decode_cell(idx, column.type_info().name())
        };

        map.insert(column.name().to_string(), encode(cell));
    }

    Value::Object(map)
}
