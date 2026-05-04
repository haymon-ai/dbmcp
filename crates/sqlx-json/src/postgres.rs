//! [`RowExt`](crate::RowExt) implementation for `PostgreSQL` rows.
//!
//! Type names are normalized to uppercase because sqlx may return either case
//! depending on the query context. Integer types use size-specific Rust types
//! (`i16`, `i32`, `i64`) because sqlx enforces strict type matching for
//! `PostgreSQL`. Temporal types (`DATE`, `TIME`, `TIMESTAMP`, `TIMESTAMPTZ`)
//! are decoded via sqlx's `chrono` integration and serialized as RFC 3339
//! strings; `TIMESTAMPTZ` is normalized to UTC and emitted with a `Z` suffix.
//! `NUMERIC` is decoded via `BigDecimal` to preserve precision; `MONEY`
//! arrives as text (sqlx uses simple-query for parameterless statements)
//! and is parsed locale-aware then routed through the same shape rule.

use std::str::FromStr;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use bigdecimal::BigDecimal;
use serde_json::{Map, Value};
use sqlx::postgres::PgRow;
use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};
use sqlx::{Column, Row, TypeInfo, ValueRef};

use crate::RowExt;
use crate::numeric::bigdecimal_to_json;

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
        let columns = self.columns();
        let mut map = Map::with_capacity(columns.len());

        for column in columns {
            let idx = column.ordinal();
            let type_name = column.type_info().name().to_ascii_uppercase();

            let value = if self.try_get_raw(idx).is_ok_and(|v| v.is_null()) {
                Value::Null
            } else {
                match type_name.as_str() {
                    "BOOL" => self.try_get::<bool, _>(idx).map_or(Value::Null, Value::Bool),

                    "INT8" => self
                        .try_get::<i64, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(v.into())),

                    "INT4" | "OID" => self
                        .try_get::<i32, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(i64::from(v).into())),

                    "INT2" => self
                        .try_get::<i16, _>(idx)
                        .map_or(Value::Null, |v| Value::Number(i64::from(v).into())),

                    "NUMERIC" => self
                        .try_get::<BigDecimal, _>(idx)
                        .map_or(Value::Null, |v| bigdecimal_to_json(&v)),

                    // dbmcp passes raw `&str` queries → sqlx uses Postgres' simple-query
                    // (text) protocol, where `PgMoney` errors out (binary-only). Parse the
                    // locale-formatted text form ($1,234.56, -$99.99) directly — assumes the
                    // en_US.UTF-8 lc_monetary default (`$` symbol, `.` decimal).
                    "MONEY" => self
                        .try_get_raw(idx)
                        .ok()
                        .and_then(|v| v.as_str().ok())
                        .and_then(parse_pg_money_text)
                        .map_or(Value::Null, |bd| bigdecimal_to_json(&bd)),

                    "FLOAT4" => self.try_get::<f32, _>(idx).map_or(Value::Null, Value::from),

                    "FLOAT8" => self.try_get::<f64, _>(idx).map_or(Value::Null, Value::from),

                    "BYTEA" => self
                        .try_get::<Vec<u8>, _>(idx)
                        .map_or(Value::Null, |bytes| Value::String(BASE64.encode(&bytes))),

                    "JSON" | "JSONB" => self.try_get::<Value, _>(idx).unwrap_or(Value::Null),

                    "DATE" => self
                        .try_get::<NaiveDate, _>(idx)
                        .map_or(Value::Null, |v| Value::String(v.to_string())),

                    "TIME" => self
                        .try_get::<NaiveTime, _>(idx)
                        .map_or(Value::Null, |v| Value::String(v.to_string())),

                    "TIMESTAMP" => self
                        .try_get::<NaiveDateTime, _>(idx)
                        .map_or(Value::Null, |v| Value::String(format!("{}T{}", v.date(), v.time()))),

                    "TIMESTAMPTZ" => self.try_get::<DateTime<Utc>, _>(idx).map_or(Value::Null, |v| {
                        let n = v.naive_utc();
                        Value::String(format!("{}T{}Z", n.date(), n.time()))
                    }),

                    _ => self.try_get::<String, _>(idx).map_or(Value::Null, Value::String),
                }
            };

            map.insert(column.name().to_string(), value);
        }

        Value::Object(map)
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
