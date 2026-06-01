use crate::{FromRow, FromRowError, driver::FromValue};

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Bool(bool),
    Bytes(Vec<u8>),
    F32(f32),
    F64(f64),
    I32(i32),
    I64(i64),
    #[cfg(feature = "with-time-0_3")]
    OffsetDateTime(time::OffsetDateTime),
    String(String),
    #[cfg(feature = "with-uuid-1")]
    Uuid(uuid::Uuid),
}

impl Value {
    /// Return a human-readable name for the variant, used in error messages.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Null => "null",
            Self::Bool(_) => "bool",
            Self::Bytes(_) => "bytes",
            Self::F32(_) => "f32",
            Self::F64(_) => "f64",
            Self::I32(_) => "i32",
            Self::I64(_) => "i64",
            #[cfg(feature = "with-time-0_3")]
            Self::OffsetDateTime(_) => "OffsetDateTime",
            Self::String(_) => "string",
            #[cfg(feature = "with-uuid-1")]
            Self::Uuid(_) => "Uuid",
        }
    }
}

// -----------------------------------------------------------------------------
//                           Into<Value> IMPLS
// -----------------------------------------------------------------------------

impl From<bool> for Value {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<f32> for Value {
    fn from(value: f32) -> Self {
        Self::F32(value)
    }
}

impl From<f64> for Value {
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}

impl From<i32> for Value {
    fn from(value: i32) -> Self {
        Self::I32(value)
    }
}

impl From<i64> for Value {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

#[cfg(feature = "with-time-0_3")]
impl From<time::OffsetDateTime> for Value {
    fn from(value: time::OffsetDateTime) -> Self {
        Self::OffsetDateTime(value)
    }
}

impl From<String> for Value {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl<'a> From<&'a str> for Value {
    fn from(value: &'a str) -> Self {
        Self::String(value.into())
    }
}

#[cfg(feature = "with-uuid-1")]
impl From<uuid::Uuid> for Value {
    fn from(value: uuid::Uuid) -> Self {
        Self::Uuid(value)
    }
}

impl From<Vec<u8>> for Value {
    fn from(value: Vec<u8>) -> Self {
        Self::Bytes(value)
    }
}

impl<T: Into<Value>> From<Option<T>> for Value {
    fn from(value: Option<T>) -> Self {
        match value {
            Some(v) => v.into(),
            None => Self::Null,
        }
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

impl FromValue for bool {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Bool(b) => Ok(b),
            Value::I32(i) => Ok(i != 0),
            Value::I64(i) => Ok(i != 0),
            other => Err(type_mismatch("bool", &other)),
        }
    }
}

impl FromValue for f32 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::F32(f) => Ok(f),
            Value::I32(i) => Ok(i as f32),
            other => Err(type_mismatch("f32", &other)),
        }
    }
}

impl FromValue for f64 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::F64(f) => Ok(f),
            Value::F32(f) => Ok(f as f64),
            Value::I32(i) => Ok(i as f64),
            Value::I64(i) => Ok(i as f64),
            other => Err(type_mismatch("f64", &other)),
        }
    }
}

impl FromValue for i32 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Bool(b) => Ok(if b { 1 } else { 0 }),
            Value::I32(i) => Ok(i),
            other => Err(type_mismatch("i32", &other)),
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Bool(b) => Ok(if b { 1 } else { 0 }),
            Value::I32(i) => Ok(i as i64),
            Value::I64(i) => Ok(i),
            other => Err(type_mismatch("i64", &other)),
        }
    }
}

#[cfg(feature = "with-time-0_3")]
impl FromValue for time::OffsetDateTime {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::OffsetDateTime(dt) => Ok(dt),
            other => Err(type_mismatch("OffsetDateTime", &other)),
        }
    }
}

impl FromValue for String {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Bool(b) => Ok(if b { "true" } else { "false" }.to_string()),
            Value::F32(f) => Ok(f.to_string()),
            Value::F64(f) => Ok(f.to_string()),
            Value::I32(i) => Ok(i.to_string()),
            Value::I64(i) => Ok(i.to_string()),
            Value::String(s) => Ok(s),
            Value::Bytes(b) => String::from_utf8(b).map_err(|_| FromRowError::Utf8Error),
            Value::Null => Err(type_mismatch("string", &Value::Null)),
            #[cfg(feature = "with-time-0_3")]
            other @ Value::OffsetDateTime(_) => Err(type_mismatch("string", &other)),
            #[cfg(feature = "with-uuid-1")]
            other @ Value::Uuid(_) => Err(type_mismatch("string", &other)),
        }
    }
}

#[cfg(feature = "with-uuid-1")]
impl FromValue for uuid::Uuid {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Uuid(u) => Ok(u),
            other => Err(type_mismatch("Uuid", &other)),
        }
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

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: Value) -> Result<Self, FromRowError> {
        match value {
            Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

impl<T: FromValue> FromRow for T {
    fn from_row<R: crate::Row>(row: &R) -> Result<Self, FromRowError> {
        row.try_get(0)
    }
}
