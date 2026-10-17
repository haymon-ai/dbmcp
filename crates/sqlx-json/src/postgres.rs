//! [`RowExt`](crate::RowExt) implementation for `PostgreSQL` rows.
//!
//! Maps each column's type name to a [`Cell`]; JSON shapes are owned by
//! [`encode`](crate::cell::encode). Type names are normalized to uppercase
//! because sqlx may return either case depending on the query context.
//! Integer types use size-specific Rust types (`i16`, `i32`, `i64`) because
//! sqlx enforces strict type matching for `PostgreSQL`. `TIMESTAMPTZ`
//! decodes to `Cell::DateTimeUtc`. `MONEY` arrives as text (sqlx uses
//! simple-query for parameterless statements) and is parsed locale-aware
//! into `Cell::Decimal`, like `NUMERIC`.

use std::str::FromStr;

use bigdecimal::BigDecimal;
use sqlx::postgres::PgRow;

use crate::prelude::*;

/// Parses a locale-formatted Postgres `MONEY` text value into a `BigDecimal`.
///
/// Strips currency symbol and grouping separators (everything except digits,
/// `.`, and `-`), then parses what remains. Tuned for the en_US.UTF-8
/// `lc_monetary` default — locales using `,` as decimal separator are not
/// supported.
fn parse_pg_money_text(text: &str) -> Option<BigDecimal> {
    let cleaned: String = text.chars().filter(|c| matches!(c, '0'..='9' | '.' | '-')).collect();
    BigDecimal::from_str(&cleaned).ok()
}

impl RowExt for PgRow {
    fn to_json(&self) -> Value {
        row_to_json(self)
    }
}

impl DecodeCell for PgRow {
    fn decode_cell(&self, idx: usize, type_name: &str) -> Cell {
        match type_name.to_ascii_uppercase().as_str() {
            "BOOL" => self.try_get(idx).map_or(Cell::Null, Cell::Bool),

            "INT8" => self.try_get(idx).map_or(Cell::Null, Cell::Int),

            "INT4" | "OID" => self.try_get(idx).map_or(Cell::Null, |v: i32| Cell::Int(v.into())),

            "INT2" => self.try_get(idx).map_or(Cell::Null, |v: i16| Cell::Int(v.into())),

            "NUMERIC" => self.try_get(idx).map_or(Cell::Null, Cell::Decimal),

            // dbmcp passes raw `&str` queries → sqlx uses Postgres' simple-query
            // (text) protocol, where `PgMoney` errors out (binary-only). Parse the
            // locale-formatted text form ($1,234.56, -$99.99) directly — assumes the
            // en_US.UTF-8 lc_monetary default (`$` symbol, `.` decimal).
            "MONEY" => self
                .try_get_raw(idx)
                .ok()
                .and_then(|v| v.as_str().ok())
                .and_then(parse_pg_money_text)
                .map_or(Cell::Null, Cell::Decimal),

            "FLOAT4" => self.try_get(idx).map_or(Cell::Null, Cell::Float32),

            "FLOAT8" => self.try_get(idx).map_or(Cell::Null, Cell::Float64),

            "BYTEA" => self.try_get(idx).map_or(Cell::Null, Cell::Bytes),

            "JSON" | "JSONB" => self.try_get(idx).map_or(Cell::Null, Cell::Json),

            "DATE" => self.try_get(idx).map_or(Cell::Null, Cell::Date),

            "TIME" => self.try_get(idx).map_or(Cell::Null, Cell::Time),

            "TIMESTAMP" => self.try_get(idx).map_or(Cell::Null, Cell::DateTime),

            "TIMESTAMPTZ" => self.try_get(idx).map_or(Cell::Null, Cell::DateTimeUtc),

            _ => self.try_get(idx).map_or(Cell::Null, Cell::Text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_pg_money_text;
    use bigdecimal::BigDecimal;
    use std::str::FromStr;

    fn dec(s: &str) -> BigDecimal {
        BigDecimal::from_str(s).expect("valid decimal literal")
    }

    #[test]
    fn parses_plain_money() {
        assert_eq!(parse_pg_money_text("$123.45"), Some(dec("123.45")));
    }

    #[test]
    fn parses_money_with_thousand_separators() {
        // Postgres MONEY in en_US.UTF-8 emits grouping commas in output:
        // `$1,234,567.89`. The filter drops everything but digits/`.`/`-`,
        // so commas vanish before parsing.
        assert_eq!(parse_pg_money_text("$1,234.56"), Some(dec("1234.56")));
        assert_eq!(parse_pg_money_text("$1,234,567.89"), Some(dec("1234567.89")));
    }

    #[test]
    fn parses_zero_money() {
        assert_eq!(parse_pg_money_text("$0.00"), Some(dec("0")));
    }

    #[test]
    fn parses_negative_money_leading_minus_outside_symbol() {
        // Default en_US.UTF-8 form: `-$99.99`.
        assert_eq!(parse_pg_money_text("-$99.99"), Some(dec("-99.99")));
    }

    #[test]
    fn parses_negative_money_with_minus_after_symbol() {
        // Some locales render as `$-99.99`; filter retains `-` so the parse
        // still produces a negative value.
        assert_eq!(parse_pg_money_text("$-99.99"), Some(dec("-99.99")));
    }

    #[test]
    fn empty_string_returns_none() {
        assert!(parse_pg_money_text("").is_none());
    }

    #[test]
    fn unparseable_returns_none() {
        // After filtering: `..` — bigdecimal rejects this.
        assert!(parse_pg_money_text("$.").is_none());
        assert!(parse_pg_money_text("abc").is_none());
    }

    #[test]
    fn accounting_parens_misparsed_as_positive() {
        // Documents a known limitation: locales that wrap negatives in
        // parentheses ($99.99) lose the negative sign because the filter
        // strips `(` and `)`. Postgres en_US.UTF-8 default does not use
        // this form; if a deployment customises lc_monetary to one that
        // does, the wire form will be wrong. Test pins behaviour so any
        // future fix surfaces as an obvious diff.
        assert_eq!(parse_pg_money_text("($99.99)"), Some(dec("99.99")));
    }

    #[test]
    fn large_money_at_i64_max_cents() {
        // $92,233,720,368,547,758.07 — the maximum positive Postgres MONEY,
        // beyond f64's 15-digit safe range.
        assert_eq!(
            parse_pg_money_text("$92,233,720,368,547,758.07"),
            Some(dec("92233720368547758.07"))
        );
    }
}
