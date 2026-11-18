//! Shared row-to-JSON driver over the per-backend cell decoders.

use serde_json::{Map, Value};
use sqlx::{Column, ColumnIndex, Row, TypeInfo, ValueRef};

use crate::cell::{Cell, encode};

/// Decodes one non-`NULL` column of a backend row into a [`Cell`].
pub(crate) trait DecodeCell: Row {
    /// Decodes column `idx`, whose driver-reported type name is `type_name`.
    ///
    /// Never called for SQL `NULL` values.
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell;
}

/// Converts a row into its `(name, value)` cells, in column order.
///
/// Keeps every column, duplicate names included.
pub(crate) fn row_to_cells<R>(row: &R) -> Vec<(String, Value)>
where
    R: DecodeCell,
    usize: ColumnIndex<R>,
{
    row.columns()
        .iter()
        .map(|column| (column.name().to_string(), column_value(row, column)))
        .collect()
}

/// Converts a row into a JSON object keyed by column name.
///
/// Matches [`cells_to_object`] over [`row_to_cells`] without the intermediate cells.
pub(crate) fn row_to_json<R>(row: &R) -> Value
where
    R: DecodeCell,
    usize: ColumnIndex<R>,
{
    let columns = row.columns();
    let mut map = Map::with_capacity(columns.len());

    for column in columns {
        map.insert(column.name().to_string(), column_value(row, column));
    }

    Value::Object(map)
}

/// Decodes one column of `row` to its JSON shape; SQL `NULL` is [`Value::Null`].
fn column_value<R>(row: &R, column: &<R::Database as sqlx::Database>::Column) -> Value
where
    R: DecodeCell,
    usize: ColumnIndex<R>,
{
    let idx = column.ordinal();

    let cell = if row.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
        Cell::Null
    } else {
        row.decode_cell(idx, column.type_info().name())
    };

    encode(cell)
}

/// Builds a JSON object from positional cells, as [`RowExt::to_json`](crate::RowExt::to_json) does.
///
/// A later cell overwrites an earlier one with the same name.
#[must_use]
pub fn cells_to_object(cells: Vec<(String, Value)>) -> Value {
    let mut map = Map::with_capacity(cells.len());

    for (name, value) in cells {
        map.insert(name, value);
    }

    Value::Object(map)
}
