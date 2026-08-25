use std::net::IpAddr;

use crate::{FromRow, FromRowError, driver::FromValue};

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Array(Vec<Value>),
    Bool(bool),
    Bytes(Vec<u8>),
    F32(f32),
    F64(f64),
    I16(i16),
    I32(i32),
    I64(i64),
    IpAddr(IpAddr),
    String(String),
    U64(u64),
    #[cfg(feature = "with-rust_decimal-1")]
    Decimal(rust_decimal::Decimal),
    #[cfg(feature = "with-serde_json-1")]
    Json(serde_json::Value),
    #[cfg(feature = "with-time-0_3")]
    Date(time::Date),
    #[cfg(feature = "with-time-0_3")]
    DateTime(time::PrimitiveDateTime),
    #[cfg(feature = "with-time-0_3")]
    OffsetDateTime(time::OffsetDateTime),
    #[cfg(feature = "with-time-0_3")]
    Time(time::Time),
    #[cfg(feature = "with-uuid-1")]
    Uuid(uuid::Uuid),
}

impl Value {
    /// Return a human-readable name for the variant, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Array(_) => "array",
            Self::Bool(_) => "bool",
            Self::Bytes(_) => "bytes",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
            Self::I16(_) => "i16",
            Self::I32(_) => "i32",
            Self::I64(_) => "i64",
            Self::IpAddr(_) => "IpAddr",
            Self::String(_) => "string",
            Self::U64(_) => "u64",
            #[cfg(feature = "with-rust_decimal-1")]
            Self::Decimal(_) => "Decimal",
            #[cfg(feature = "with-serde_json-1")]
            Self::Json(_) => "Json",
            #[cfg(feature = "with-time-0_3")]
            Self::Date(_) => "Date",
            #[cfg(feature = "with-time-0_3")]
            Self::DateTime(_) => "PrimitiveDateTime",
            #[cfg(feature = "with-time-0_3")]
            Self::OffsetDateTime(_) => "OffsetDateTime",
            #[cfg(feature = "with-time-0_3")]
            Self::Time(_) => "Time",
            #[cfg(feature = "with-uuid-1")]
            Self::Uuid(_) => "Uuid",
        }
    }
}

// -----------------------------------------------------------------------------
//                           Into<Value> IMPLS
// -----------------------------------------------------------------------------

macro_rules! impl_from {
    ($($t:ty => |$v:ident| $body:expr),+ $(,)?) => {$(
        impl From<$t> for Value {
            fn from($v: $t) -> Self {
                $body
            }
        }
    )+};
}

// Note: `u8` is deliberately absent. `Vec<u8>` maps to `Value::Bytes`, and the
// blanket `From<Vec<T>>` below only coexists with it while `u8: !Into<Value>`.
impl_from! {
    bool => |v| Self::Bool(v),
    f32 => |v| Self::F32(v),
    f64 => |v| Self::F64(v),
    i8 => |v| Self::I16(v as i16),
    i16 => |v| Self::I16(v),
    i32 => |v| Self::I32(v),
    i64 => |v| Self::I64(v),
    u16 => |v| Self::I32(v as i32),
    u32 => |v| Self::I64(v as i64),
    u64 => |v| Self::U64(v),
    IpAddr => |v| Self::IpAddr(v),
    String => |v| Self::String(v),
    &str => |v| Self::String(v.to_owned()),
    Vec<u8> => |v| Self::Bytes(v),
}

#[cfg(feature = "with-rust_decimal-1")]
impl_from!(rust_decimal::Decimal => |v| Self::Decimal(v));

#[cfg(feature = "with-serde_json-1")]
impl_from!(serde_json::Value => |v| Self::Json(v));

#[cfg(feature = "with-time-0_3")]
impl_from! {
    time::Date => |v| Self::Date(v),
    time::PrimitiveDateTime => |v| Self::DateTime(v),
    time::OffsetDateTime => |v| Self::OffsetDateTime(v),
    time::Time => |v| Self::Time(v),
}

#[cfg(feature = "with-uuid-1")]
impl_from!(uuid::Uuid => |v| Self::Uuid(v));

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => v.into(),
            None => Self::Null,
        }
    }
}

impl<T: Into<Value>> From<Vec<T>> for Value {
    fn from(value: Vec<T>) -> Self {
        Self::Array(value.into_iter().map(Into::into).collect())
    }
}

// -----------------------------------------------------------------------------
//                           FromValue IMPLS
// -----------------------------------------------------------------------------

fn type_mismatch(expected: &'static str, got: &Value) -> FromRowError {
    FromRowError::TypeMismatch {
        expected,
        got: got.type_name().to_string(),
    }
}

/// Widen any integral variant to `i128` so the concrete impls only have to
/// range-check. Drivers disagree on how wide an integer column comes back
/// (MySQL reports every width as `Int(i64)`), so conversions have to be by
/// value rather than by variant.
fn as_int(value: &Value) -> Option<i128> {
    Some(match value {
        Value::Bool(b) => *b as i128,
        Value::I16(i) => *i as i128,
        Value::I32(i) => *i as i128,
        Value::I64(i) => *i as i128,
        Value::U64(u) => *u as i128,
        _ => return None,
    })
}

fn as_float(value: &Value) -> Option<f64> {
    Some(match value {
        Value::F32(f) => *f as f64,
        Value::F64(f) => *f,
        #[cfg(feature = "with-rust_decimal-1")]
        Value::Decimal(d) => rust_decimal::prelude::ToPrimitive::to_f64(d)?,
        other => as_int(other)? as f64,
    })
}

macro_rules! impl_int_from_value {
    ($($t:ty),+ $(,)?) => {$(
        impl FromValue for $t {
            fn from_value(value: Value) -> Result<Self, FromRowError> {
                as_int(&value)
                    .and_then(|i| <$t>::try_from(i).ok())
                    .ok_or_else(|| type_mismatch(stringify!($t), &value))
            }
        }
    )+};
}

// `u8` is omitted on purpose; see the note on the `From` impls above.
impl_int_from_value!(i8, i16, i32, i64, i128, u16, u32, u64);

impl FromValue for bool {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        as_int(&value)
            .map(|i| i != 0)
            .ok_or_else(|| type_mismatch("bool", &value))
    }
}

impl FromValue for f32 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        as_float(&value)
            .map(|f| f as f32)
            .ok_or_else(|| type_mismatch("f32", &value))
    }
}

impl FromValue for f64 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        as_float(&value).ok_or_else(|| type_mismatch("f64", &value))
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        Ok(match value {
            Value::Bool(b) => if b { "true" } else { "false" }.to_string(),
            Value::Bytes(b) => String::from_utf8(b).map_err(|_| FromRowError::Utf8Error)?,
            Value::F32(f) => f.to_string(),
            Value::F64(f) => f.to_string(),
            Value::I16(i) => i.to_string(),
            Value::I32(i) => i.to_string(),
            Value::I64(i) => i.to_string(),
            Value::IpAddr(a) => a.to_string(),
            Value::String(s) => s,
            Value::U64(u) => u.to_string(),
            #[cfg(feature = "with-rust_decimal-1")]
            Value::Decimal(d) => d.to_string(),
            #[cfg(feature = "with-serde_json-1")]
            Value::Json(j) => j.to_string(),
            #[cfg(feature = "with-uuid-1")]
            Value::Uuid(u) => u.to_string(),
            other => return Err(type_mismatch("string", &other)),
        })
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Bytes(b) => Ok(b),
            Value::String(s) => Ok(s.into_bytes()),
            other => Err(type_mismatch("bytes", &other)),
        }
    }
}

impl FromValue for IpAddr {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::IpAddr(a) => Ok(a),
            // MySQL has no inet type; VARCHAR is the usual stand-in.
            Value::Bytes(_) | Value::String(_) => {
                let s = String::from_value(value)?;
                s.parse()
                    .map_err(|_| type_mismatch("IpAddr", &Value::String(s)))
            }
            other => Err(type_mismatch("IpAddr", &other)),
        }
    }
}

/// Coexists with the `Vec<u8>` impl above only because no `FromValue for u8`
/// exists. Adding one turns both into overlapping impls.
impl<T: FromValue> FromValue for Vec<T> {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Array(items) => items.into_iter().map(T::from_value).collect(),
            other => Err(type_mismatch("array", &other)),
        }
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

#[cfg(feature = "with-rust_decimal-1")]
impl FromValue for rust_decimal::Decimal {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Decimal(d) => Ok(d),
            // MySQL returns DECIMAL as text.
            Value::Bytes(_) | Value::String(_) => {
                let s = String::from_value(value)?;
                s.parse()
                    .map_err(|_| type_mismatch("Decimal", &Value::String(s)))
            }
            other => Err(type_mismatch("Decimal", &other)),
        }
    }
}

#[cfg(feature = "with-serde_json-1")]
impl FromValue for serde_json::Value {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Json(j) => Ok(j),
            // MySQL returns JSON as text.
            Value::Bytes(_) | Value::String(_) => {
                let s = String::from_value(value)?;
                serde_json::from_str(&s).map_err(|_| type_mismatch("Json", &Value::String(s)))
            }
            other => Err(type_mismatch("Json", &other)),
        }
    }
}

#[cfg(feature = "with-time-0_3")]
mod time_impls {
    use super::{FromRowError, FromValue, Value, type_mismatch};

    impl FromValue for time::Date {
        fn from_value(value: Value) -> Result<Self, FromRowError> {
            match value {
                Value::Date(d) => Ok(d),
                Value::DateTime(dt) => Ok(dt.date()),
                Value::OffsetDateTime(dt) => Ok(dt.date()),
                other => Err(type_mismatch("Date", &other)),
            }
        }
    }

    impl FromValue for time::Time {
        fn from_value(value: Value) -> Result<Self, FromRowError> {
            match value {
                Value::Time(t) => Ok(t),
                Value::DateTime(dt) => Ok(dt.time()),
                Value::OffsetDateTime(dt) => Ok(dt.time()),
                other => Err(type_mismatch("Time", &other)),
            }
        }
    }

    impl FromValue for time::PrimitiveDateTime {
        fn from_value(value: Value) -> Result<Self, FromRowError> {
            match value {
                Value::DateTime(dt) => Ok(dt),
                Value::Date(d) => Ok(d.midnight()),
                Value::OffsetDateTime(dt) => Ok(time::PrimitiveDateTime::new(dt.date(), dt.time())),
                other => Err(type_mismatch("PrimitiveDateTime", &other)),
            }
        }
    }

    impl FromValue for time::OffsetDateTime {
        fn from_value(value: Value) -> Result<Self, FromRowError> {
            match value {
                Value::OffsetDateTime(dt) => Ok(dt),
                // A naive timestamp column carries no zone; UTC is the only
                // defensible reading.
                Value::DateTime(dt) => Ok(dt.assume_utc()),
                Value::Date(d) => Ok(d.midnight().assume_utc()),
                other => Err(type_mismatch("OffsetDateTime", &other)),
            }
        }
    }
}

#[cfg(feature = "with-uuid-1")]
impl FromValue for uuid::Uuid {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Uuid(u) => Ok(u),
            // MySQL has no UUID type; CHAR(36) and BINARY(16) are the
            // conventional storage choices.
            Value::Bytes(b) if b.len() == 16 => Ok(uuid::Uuid::from_slice(&b).unwrap()),
            Value::Bytes(_) | Value::String(_) => {
                let s = String::from_value(value)?;
                s.parse()
                    .map_err(|_| type_mismatch("Uuid", &Value::String(s)))
            }
            other => Err(type_mismatch("Uuid", &other)),
        }
    }
}

impl<T: FromValue> FromRow for T {
    fn from_row<R: crate::Row>(row: &R) -> Result<Self, FromRowError> {
        row.try_get(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get<T: FromValue>(v: Value) -> Result<T, FromRowError> {
        T::from_value(v)
    }

    #[test]
    fn integers_convert_across_widths() {
        // MySQL reports every integer width as Int(i64); Postgres reports int4
        // as I32. The same target type has to accept both.
        assert_eq!(get::<i32>(Value::I64(7)).unwrap(), 7);
        assert_eq!(get::<i32>(Value::I32(7)).unwrap(), 7);
        assert_eq!(get::<i32>(Value::I16(7)).unwrap(), 7);
        assert_eq!(get::<i64>(Value::I32(7)).unwrap(), 7);
        assert_eq!(get::<u32>(Value::I64(7)).unwrap(), 7);
    }

    #[test]
    fn out_of_range_integers_are_errors_not_wraps() {
        assert!(get::<i32>(Value::I64(i64::from(i32::MAX) + 1)).is_err());
        assert!(get::<u32>(Value::I64(-1)).is_err());
        assert!(get::<i64>(Value::U64(u64::MAX)).is_err());
        assert_eq!(get::<u64>(Value::U64(u64::MAX)).unwrap(), u64::MAX);
    }

    #[test]
    fn bools_and_ints_interoperate() {
        assert!(get::<bool>(Value::I64(1)).unwrap());
        assert!(!get::<bool>(Value::I64(0)).unwrap());
        assert_eq!(get::<i32>(Value::Bool(true)).unwrap(), 1);
    }

    #[test]
    fn floats_widen_from_integers() {
        assert_eq!(get::<f64>(Value::I32(3)).unwrap(), 3.0);
        assert_eq!(get::<f64>(Value::F32(0.5)).unwrap(), 0.5);
        assert_eq!(get::<f32>(Value::F64(0.5)).unwrap(), 0.5);
    }

    #[test]
    fn arrays_round_trip() {
        let value: Value = vec![1i32, 2, 3].into();
        assert!(matches!(value, Value::Array(ref v) if v.len() == 3));
        assert_eq!(get::<Vec<i32>>(value).unwrap(), vec![1, 2, 3]);

        let nested: Value = vec![vec!["a".to_string()], vec!["b".to_string()]].into();
        assert_eq!(
            get::<Vec<Vec<String>>>(nested).unwrap(),
            vec![vec!["a".to_string()], vec!["b".to_string()]]
        );
    }

    #[test]
    fn bytes_stay_bytes_not_arrays() {
        // `Vec<u8>` must keep routing to bytea rather than the generic array
        // path.
        let value: Value = vec![1u8, 2, 3].into();
        assert!(matches!(value, Value::Bytes(_)));
        assert_eq!(get::<Vec<u8>>(value).unwrap(), vec![1u8, 2, 3]);
    }

    #[test]
    fn nulls_only_land_in_options() {
        assert_eq!(get::<Option<i32>>(Value::Null).unwrap(), None);
        assert_eq!(get::<Option<i32>>(Value::I32(1)).unwrap(), Some(1));
        assert!(get::<i32>(Value::Null).is_err());
    }

    #[test]
    fn ip_addresses_parse_from_text() {
        let addr: IpAddr = "192.168.1.1".parse().unwrap();
        assert_eq!(get::<IpAddr>(Value::IpAddr(addr)).unwrap(), addr);
        // MySQL stores inet as text.
        assert_eq!(
            get::<IpAddr>(Value::String("192.168.1.1".into())).unwrap(),
            addr
        );
    }

    #[cfg(feature = "with-rust_decimal-1")]
    #[test]
    fn decimals_parse_from_text() {
        let d: rust_decimal::Decimal = "10.25".parse().unwrap();
        assert_eq!(get::<rust_decimal::Decimal>(Value::Decimal(d)).unwrap(), d);
        // MySQL returns DECIMAL as bytes.
        assert_eq!(
            get::<rust_decimal::Decimal>(Value::Bytes(b"10.25".to_vec())).unwrap(),
            d
        );
        assert_eq!(get::<f64>(Value::Decimal(d)).unwrap(), 10.25);
    }

    #[cfg(feature = "with-serde_json-1")]
    #[test]
    fn json_parses_from_text() {
        let j = serde_json::json!({ "a": 1 });
        assert_eq!(get::<serde_json::Value>(Value::Json(j.clone())).unwrap(), j);
        // MySQL returns JSON as bytes.
        assert_eq!(
            get::<serde_json::Value>(Value::Bytes(br#"{"a":1}"#.to_vec())).unwrap(),
            j
        );
    }

    #[cfg(feature = "with-uuid-1")]
    #[test]
    fn uuids_parse_from_both_mysql_storage_conventions() {
        let u = uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
        assert_eq!(get::<uuid::Uuid>(Value::Uuid(u)).unwrap(), u);
        // CHAR(36)
        assert_eq!(get::<uuid::Uuid>(Value::String(u.to_string())).unwrap(), u);
        // BINARY(16)
        assert_eq!(
            get::<uuid::Uuid>(Value::Bytes(u.as_bytes().to_vec())).unwrap(),
            u
        );
    }

    #[cfg(feature = "with-time-0_3")]
    #[test]
    fn temporal_types_narrow_and_widen() {
        let date = time::Date::from_calendar_date(2026, time::Month::August, 24).unwrap();
        let time = time::Time::from_hms(13, 30, 0).unwrap();
        let dt = time::PrimitiveDateTime::new(date, time);

        // MySQL hands DATE, DATETIME and TIMESTAMP back through one variant, so
        // a DateTime must satisfy a Date or Time target.
        assert_eq!(get::<time::Date>(Value::DateTime(dt)).unwrap(), date);
        assert_eq!(get::<time::Time>(Value::DateTime(dt)).unwrap(), time);
        assert_eq!(
            get::<time::PrimitiveDateTime>(Value::Date(date)).unwrap(),
            date.midnight()
        );
        assert_eq!(
            get::<time::OffsetDateTime>(Value::DateTime(dt)).unwrap(),
            dt.assume_utc()
        );
    }
}
