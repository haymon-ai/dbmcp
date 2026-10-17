//! Backend-neutral column values and their JSON shapes.
//!
//! Each backend adapter decodes a column into a [`Cell`]; [`encode`] then
//! applies the one JSON shape rule for that cell kind, so the same kind of
//! value produces the same JSON on every backend.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use bigdecimal::{BigDecimal, ToPrimitive};
use serde_json::Value;
use sqlx::types::chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, Utc};

/// A single decoded column value, independent of the backend it came from.
#[derive(Debug)]
pub(crate) enum Cell {
    /// SQL `NULL`, or a value the adapter could not decode.
    Null,
    /// Boolean.
    Bool(bool),
    /// Signed integer.
    Int(i64),
    /// Unsigned integer that may exceed `i64::MAX`.
    UInt(u64),
    /// Single-precision float.
    Float32(f32),
    /// Double-precision float.
    Float64(f64),
    /// Exact fixed-point or arbitrary-precision numeric.
    Decimal(BigDecimal),
    /// Text.
    Text(String),
    /// Raw bytes.
    Bytes(Vec<u8>),
    /// Already-parsed JSON document.
    Json(Value),
    /// Calendar date.
    Date(NaiveDate),
    /// Time of day without a zone.
    Time(NaiveTime),
    /// Date and time without a zone.
    DateTime(NaiveDateTime),
    /// Date and time in UTC.
    DateTimeUtc(DateTime<Utc>),
}

/// Converts a [`Cell`] to its JSON shape.
///
/// Unsigned integers beyond `i64::MAX` become strings; non-finite floats
/// become `null`; decimals follow `bigdecimal_to_json`; bytes are
/// base64-encoded; temporal values are ISO 8601 strings, with a `Z` suffix
/// only for UTC date-times.
pub(crate) fn encode(cell: Cell) -> Value {
    match cell {
        Cell::Null => Value::Null,
        Cell::Bool(v) => Value::Bool(v),
        Cell::Int(v) => Value::from(v),
        Cell::UInt(v) => i64::try_from(v).map_or_else(|_| Value::String(v.to_string()), Value::from),
        Cell::Float32(v) => Value::from(v),
        Cell::Float64(v) => Value::from(v),
        Cell::Decimal(v) => bigdecimal_to_json(&v),
        Cell::Text(v) => Value::String(v),
        Cell::Bytes(v) => Value::String(BASE64.encode(v)),
        Cell::Json(v) => v,
        Cell::Date(v) => Value::String(v.to_string()),
        Cell::Time(v) => Value::String(v.to_string()),
        Cell::DateTime(v) => Value::String(format!("{}T{}", v.date(), v.time())),
        Cell::DateTimeUtc(v) => {
            let n = v.naive_utc();
            Value::String(format!("{}T{}Z", n.date(), n.time()))
        }
    }
}

/// Maximum significant decimal digits that round-trip through `f64`.
///
/// IEEE 754 binary64 holds 15–17 decimal digits depending on value; 15 is
/// the conservative bound that guarantees a clean round-trip for every
/// value. Beyond this, the canonical decimal string is the only lossless
/// JSON shape.
const F64_SAFE_DIGITS: u64 = 15;

/// Converts a `BigDecimal` to the canonical JSON shape for numeric values.
///
/// Integer-valued decimals that fit in `i64` emit as integer JSON numbers.
/// Other values emit as `Value::Number` when their canonical decimal form
/// has at most `F64_SAFE_DIGITS` digits, else as `Value::String` carrying
/// the decimal text.
fn bigdecimal_to_json(value: &BigDecimal) -> Value {
    let normalized = value.normalized();

    if normalized.is_integer()
        && let Some(as_i64) = normalized.to_i64()
    {
        return Value::from(as_i64);
    }

    // `digits()` counts mantissa only — for negative scale (e.g. `1e30` =
    // mantissa 1, scale -29) add the trailing zeros so huge values don't
    // slip past the gate and emit as lossy JSON numbers.
    let scale = normalized.fractional_digit_count();
    let canonical_digits = normalized.digits() + scale.min(0).unsigned_abs();
    if canonical_digits <= F64_SAFE_DIGITS
        && let Some(as_f64) = normalized.to_f64()
        // Underflow guard: tiny non-zero values (≈ |v| < 5e-324) clamp to
        // 0.0 in f64. The integer fast-path above already returned for
        // true zero, so reaching this branch with `as_f64 == 0.0` means
        // the value was non-zero and silently lost — fall through to the
        // canonical-string form instead of emitting JSON `0`.
        && as_f64 != 0.0
        && let Some(num) = serde_json::Number::from_f64(as_f64)
    {
        return Value::from(num);
    }

    // `Display` (uses scientific form past internal thresholds) keeps the
    // output bounded for pathological NUMERICs; `to_plain_string` would
    // expand 1e131072 to a 131-KB string per row.
    Value::String(normalized.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn dec(s: &str) -> BigDecimal {
        BigDecimal::from_str(s).expect("valid decimal literal")
    }

    #[test]
    fn small_fixed_point_is_number() {
        assert_eq!(bigdecimal_to_json(&dec("0.5")), Value::from(0.5));
        assert_eq!(bigdecimal_to_json(&dec("42")), Value::from(42));
        assert_eq!(bigdecimal_to_json(&dec("-1.25")), Value::from(-1.25));
    }

    #[test]
    fn trailing_zeros_normalize_consistently() {
        let a = bigdecimal_to_json(&dec("1.20"));
        let b = bigdecimal_to_json(&dec("1.2"));
        assert_eq!(a, b);
    }

    #[test]
    fn value_beyond_f64_precision_is_string() {
        let v = bigdecimal_to_json(&dec("12345678901234567890.1234567890"));
        assert_eq!(v, Value::String("12345678901234567890.123456789".to_string()));
    }

    #[test]
    fn small_decimal_with_many_significant_digits_is_string() {
        let v = bigdecimal_to_json(&dec("0.123456789012345678901234567890"));
        let Value::String(s) = v else {
            panic!("expected string for high-precision small decimal");
        };
        assert!(s.starts_with("0.12345678901234567890"));
    }

    #[test]
    fn shape_is_deterministic_per_value() {
        let v1 = bigdecimal_to_json(&dec("99999999999999999999.99"));
        let v2 = bigdecimal_to_json(&dec("99999999999999999999.99"));
        assert_eq!(v1, v2);
        assert!(matches!(v1, Value::String(_)));
    }

    #[test]
    fn integer_value_uses_integer_branch_regardless_of_digit_count() {
        // i64::MAX has 19 digits but is still an integer that fits, so the
        // integer fast-path wins over the digit gate.
        let v = bigdecimal_to_json(&dec(&i64::MAX.to_string()));
        assert_eq!(v, Value::Number(i64::MAX.into()));
    }

    #[test]
    fn boundary_15_digit_fraction_is_number() {
        let v = bigdecimal_to_json(&dec("12345678901234.5"));
        assert!(matches!(v, Value::Number(_)));
    }

    #[test]
    fn boundary_16_digit_fraction_is_string() {
        let v = bigdecimal_to_json(&dec("12345678901234.56"));
        assert!(matches!(v, Value::String(_)));
    }

    #[test]
    fn small_fraction_with_few_digits_is_number() {
        // 0.1 has 1 significant digit; safe in f64 even though the binary
        // representation is approximate. JSON serialization is shortest
        // round-trip via ryu, so the wire form matches the database value.
        assert_eq!(bigdecimal_to_json(&dec("0.1")), Value::from(0.1));
        assert_eq!(bigdecimal_to_json(&dec("0.10")), Value::from(0.1));
        assert_eq!(bigdecimal_to_json(&dec("0.30")), Value::from(0.3));
    }

    #[test]
    fn high_precision_fraction_is_string() {
        let v = bigdecimal_to_json(&dec("0.123456789012345678"));
        assert!(matches!(v, Value::String(_)));
    }

    #[test]
    fn huge_magnitude_with_few_significant_digits_is_string() {
        // 1 followed by 30 zeros (DECIMAL(38,0)). Mantissa has 1 digit but
        // magnitude exceeds 2^53, so canonical form must emit as string —
        // `Display` writes scientific form past its zero-threshold.
        let v = bigdecimal_to_json(&dec("1000000000000000000000000000000"));
        assert!(matches!(v, Value::String(_)));
    }

    #[test]
    fn zero_is_integer_zero() {
        assert_eq!(bigdecimal_to_json(&dec("0")), Value::from(0));
        assert_eq!(bigdecimal_to_json(&dec("0.0")), Value::from(0));
        assert_eq!(bigdecimal_to_json(&dec("0.0000")), Value::from(0));
    }

    #[test]
    fn negative_zero_normalizes_to_zero() {
        // BigDecimal normalises -0 to 0; emit as integer zero so the wire
        // form does not leak a meaningless minus sign.
        let v = bigdecimal_to_json(&dec("-0"));
        assert_eq!(v, Value::from(0));
        let v = bigdecimal_to_json(&dec("-0.000"));
        assert_eq!(v, Value::from(0));
    }

    #[test]
    fn i64_min_uses_integer_branch() {
        let v = bigdecimal_to_json(&dec(&i64::MIN.to_string()));
        assert_eq!(v, Value::Number(i64::MIN.into()));
    }

    #[test]
    fn integer_one_past_i64_max_is_string() {
        // 9223372036854775808 = i64::MAX + 1. is_integer() is true but
        // to_i64() returns None; the digit gate (19 > 15) routes to string.
        let v = bigdecimal_to_json(&dec("9223372036854775808"));
        assert_eq!(v, Value::String("9223372036854775808".to_string()));
    }

    #[test]
    fn very_large_integer_is_string() {
        // 25-digit integer beyond i64 and beyond f64-safe digit count.
        let v = bigdecimal_to_json(&dec("1234567890123456789012345"));
        let Value::String(s) = v else {
            panic!("expected string for huge integer");
        };
        assert_eq!(s, "1234567890123456789012345");
    }

    #[test]
    fn tiny_fraction_within_digit_budget_is_number() {
        // 1e-30 has mantissa "1" (1 digit). f64 can represent this magnitude
        // (denormal range is ~1e-308), so the value-driven rule emits a
        // JSON number — not a string — even though it looks "extreme".
        let v = bigdecimal_to_json(&dec("1e-30"));
        let Value::Number(n) = v else {
            panic!("expected JSON number for tiny fraction");
        };
        let f = n.as_f64().expect("f64 representable");
        assert!((f - 1e-30).abs() < 1e-40);
    }

    #[test]
    fn integer_15_digits_uses_integer_branch() {
        // 15-digit integer fits in i64 easily; integer fast-path wins.
        let v = bigdecimal_to_json(&dec("123456789012345"));
        assert_eq!(v, Value::Number(123_456_789_012_345_i64.into()));
    }

    #[test]
    fn f64_underflow_falls_back_to_string() {
        // 1e-1000 is below the f64 denormal floor (~5e-324); BigDecimal's
        // `to_f64` rounds it to 0.0. Without the underflow guard, the
        // shape rule would emit JSON `0` and silently lose the value.
        let v = bigdecimal_to_json(&dec("1e-1000"));
        let Value::String(s) = v else {
            panic!("expected string for f64-underflow tiny fraction, got {v:?}");
        };
        // BigDecimal::Display emits scientific form for extreme exponents.
        let lower = s.to_ascii_lowercase();
        assert!(
            lower.contains("e-1000") || lower.starts_with("0.0"),
            "must preserve magnitude: {s}"
        );
    }

    #[test]
    fn negative_f64_underflow_falls_back_to_string() {
        let v = bigdecimal_to_json(&dec("-1e-500"));
        let Value::String(s) = v else {
            panic!("expected string for negative-tiny underflow, got {v:?}");
        };
        assert!(s.starts_with('-'), "negative sign preserved: {s}");
    }

    #[test]
    fn near_underflow_within_f64_range_is_number() {
        // 1e-300 is comfortably within f64's normal range (~2.2e-308 min);
        // must emit as JSON number — guard must not over-trigger.
        let v = bigdecimal_to_json(&dec("1e-300"));
        let Value::Number(n) = v else {
            panic!("1e-300 must emit as JSON number, got {v:?}");
        };
        let f = n.as_f64().expect("number is f64");
        assert!((f / 1e-300 - 1.0).abs() < 1e-10, "round-trip preserved: {f}");
    }

    #[test]
    fn non_integer_with_negative_scale_is_handled() {
        // 1.23e5 = 123000 — integer-valued after normalisation, so the
        // integer fast-path applies; verifies that the digit-gate fallback
        // does not double-count digits for negative-scale representations
        // that normalise to whole numbers.
        let v = bigdecimal_to_json(&dec("1.23e5"));
        assert_eq!(v, Value::from(123_000));
    }

    #[test]
    fn encode_null_bool_int() {
        assert_eq!(encode(Cell::Null), Value::Null);
        assert_eq!(encode(Cell::Bool(true)), Value::Bool(true));
        assert_eq!(encode(Cell::Int(-7)), Value::from(-7));
    }

    #[test]
    fn encode_uint_within_i64_is_number() {
        assert_eq!(encode(Cell::UInt(42)), Value::from(42));
        assert_eq!(encode(Cell::UInt(i64::MAX as u64)), Value::from(i64::MAX));
    }

    #[test]
    fn encode_uint_beyond_i64_is_string() {
        assert_eq!(encode(Cell::UInt(u64::MAX)), Value::String(u64::MAX.to_string()));
    }

    #[test]
    fn encode_floats() {
        assert_eq!(encode(Cell::Float32(1.5)), Value::from(1.5_f32));
        assert_eq!(encode(Cell::Float64(2.25)), Value::from(2.25));
    }

    #[test]
    fn encode_non_finite_float_is_null() {
        assert_eq!(encode(Cell::Float32(f32::NAN)), Value::Null);
        assert_eq!(encode(Cell::Float64(f64::INFINITY)), Value::Null);
        assert_eq!(encode(Cell::Float64(f64::NAN)), Value::Null);
    }

    #[test]
    fn encode_decimal_uses_shape_rule() {
        assert_eq!(encode(Cell::Decimal(dec("1.20"))), Value::from(1.2));
        assert_eq!(
            encode(Cell::Decimal(dec("12345678901234567890.5"))),
            Value::String("12345678901234567890.5".to_string())
        );
    }

    #[test]
    fn encode_text_and_json() {
        assert_eq!(encode(Cell::Text("abc".to_string())), Value::from("abc"));
        let doc = serde_json::json!({"a": [1, null]});
        assert_eq!(encode(Cell::Json(doc.clone())), doc);
    }

    #[test]
    fn encode_bytes_is_base64() {
        assert_eq!(encode(Cell::Bytes(b"hello".to_vec())), Value::from("aGVsbG8="));
        assert_eq!(encode(Cell::Bytes(vec![0xff, 0x00])), Value::from("/wA="));
    }

    #[test]
    fn encode_temporal() {
        let date = NaiveDate::from_ymd_opt(2026, 1, 2).expect("valid date");
        let time = NaiveTime::from_hms_micro_opt(3, 4, 5, 6).expect("valid time");
        let datetime = NaiveDateTime::new(date, time);

        assert_eq!(encode(Cell::Date(date)), Value::from("2026-01-02"));
        assert_eq!(encode(Cell::Time(time)), Value::from("03:04:05.000006"));
        assert_eq!(
            encode(Cell::DateTime(datetime)),
            Value::from("2026-01-02T03:04:05.000006")
        );
        assert_eq!(
            encode(Cell::DateTimeUtc(datetime.and_utc())),
            Value::from("2026-01-02T03:04:05.000006Z")
        );
    }
}
