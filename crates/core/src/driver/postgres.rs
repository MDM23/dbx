use futures_util::TryStreamExt;
use tokio_postgres::{
    Client, Transaction,
    types::{FromSql, IsNull, Kind, ToSql, Type, to_sql_checked},
};

use crate::{
    Error, FromRow, FromRowError, Row, Value,
    driver::{Esql, EsqlDriver, FromValue, RowIndex},
};

type BoxError = Box<dyn std::error::Error + Sync + Send>;

impl From<tokio_postgres::Error> for Error<tokio_postgres::Error> {
    fn from(value: tokio_postgres::Error) -> Self {
        Self::Driver(value)
    }
}

/// Maps postgres type names onto the Rust type used to decode them. The
/// resulting value is lifted into [Value] through its `From` impls, so this
/// table only has to name types that are already convertible.
///
/// Arrays are not listed: postgres-types provides `Vec<T>` for any `T: FromSql`,
/// so `Vec<Value>` recurses through this same table for the member type.
macro_rules! pg_types {
    ($($(#[$attr:meta])* $($name:literal)|+ => $ty:ty),+ $(,)?) => {
        fn decode(ty: &Type, raw: &[u8]) -> Result<Value, BoxError> {
            Ok(match ty.name() {
                $($(#[$attr])* $($name)|+ => <$ty as FromSql>::from_sql(ty, raw)?.into(),)+
                _ if matches!(ty.kind(), Kind::Array(_)) => {
                    <Vec<Value> as FromSql>::from_sql(ty, raw)?.into()
                }
                other => return Err(format!("unsupported postgres type: {other}").into()),
            })
        }

        fn decodable(ty: &Type) -> bool {
            match ty.name() {
                $($(#[$attr])* $($name)|+ => true,)+
                _ => match ty.kind() {
                    Kind::Array(member) => decodable(member),
                    _ => false,
                },
            }
        }
    };
}

pg_types! {
    "bool" => bool,
    "bytea" => Vec<u8>,
    "bpchar" | "citext" | "name" | "text" | "unknown" | "varchar" => String,
    "float4" => f32,
    "float8" => f64,
    "inet" => std::net::IpAddr,
    "int2" => i16,
    "int4" => i32,
    "int8" => i64,
    "oid" => u32,
    #[cfg(feature = "with-rust_decimal-1")]
    "numeric" => rust_decimal::Decimal,
    #[cfg(feature = "with-serde_json-1")]
    "json" | "jsonb" => serde_json::Value,
    #[cfg(feature = "with-time-0_3")]
    "date" => time::Date,
    #[cfg(feature = "with-time-0_3")]
    "time" => time::Time,
    #[cfg(feature = "with-time-0_3")]
    "timestamp" => time::PrimitiveDateTime,
    #[cfg(feature = "with-time-0_3")]
    "timestamptz" => time::OffsetDateTime,
    #[cfg(feature = "with-uuid-1")]
    "uuid" => uuid::Uuid,
}

impl<'a> FromSql<'a> for Value {
    fn from_sql(ty: &Type, raw: &'a [u8]) -> Result<Self, BoxError> {
        decode(ty, raw)
    }

    fn from_sql_null(_: &Type) -> Result<Self, BoxError> {
        Ok(Value::Null)
    }

    fn accepts(ty: &Type) -> bool {
        decodable(ty)
    }
}

impl ToSql for Value {
    fn to_sql(&self, ty: &Type, out: &mut tokio_postgres::types::private::BytesMut) -> Result<IsNull, BoxError> {
        // Delegating to the inner type's `to_sql_checked` rather than `to_sql`
        // keeps the variant/column check where the good error message lives,
        // and lets `accepts` stay permissive.
        match self {
            Value::Null => Ok(IsNull::Yes),
            Value::Array(v) => v.to_sql_checked(ty, out),
            Value::Bool(v) => v.to_sql_checked(ty, out),
            Value::Bytes(v) => v.to_sql_checked(ty, out),
            Value::F32(v) => v.to_sql_checked(ty, out),
            Value::F64(v) => v.to_sql_checked(ty, out),
            Value::I16(v) => v.to_sql_checked(ty, out),
            Value::I32(v) => v.to_sql_checked(ty, out),
            Value::I64(v) => v.to_sql_checked(ty, out),
            Value::IpAddr(v) => v.to_sql_checked(ty, out),
            Value::String(v) => v.to_sql_checked(ty, out),
            // Postgres has no unsigned integers; int8 is the only lossless
            // target and it cannot hold the top half of the range.
            Value::U64(v) => i64::try_from(*v)?.to_sql_checked(ty, out),
            #[cfg(feature = "with-rust_decimal-1")]
            Value::Decimal(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-serde_json-1")]
            Value::Json(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-time-0_3")]
            Value::Date(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-time-0_3")]
            Value::DateTime(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-time-0_3")]
            Value::OffsetDateTime(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-time-0_3")]
            Value::Time(v) => v.to_sql_checked(ty, out),
            #[cfg(feature = "with-uuid-1")]
            Value::Uuid(v) => v.to_sql_checked(ty, out),
        }
    }

    fn accepts(_: &Type) -> bool {
        true
    }

    to_sql_checked! {}
}

impl Row for tokio_postgres::Row {
    fn try_get<'a, I, T>(&'a self, index: I) -> Result<T, FromRowError>
    where
        I: Into<RowIndex<'a>>,
        T: FromValue,
    {
        let position = match index.into() {
            RowIndex::Pos(i) if i < self.columns().len() => i,
            RowIndex::Pos(i) => return Err(FromRowError::ColumnNotFound(i.to_string())),
            RowIndex::Name(n) => self
                .columns()
                .iter()
                .position(|col| col.name() == n)
                .ok_or_else(|| FromRowError::ColumnNotFound(n.to_string()))?,
        };

        let value: Value =
            tokio_postgres::Row::try_get(self, position).map_err(|e| FromRowError::TypeMismatch {
                expected: "supported type",
                got: e.to_string(),
            })?;

        FromValue::from_value(value)
    }
}

fn slice_iter<'a>(s: &'a [Value]) -> impl ExactSizeIterator<Item = &'a dyn ToSql> + 'a {
    s.iter().map(|t| t as &dyn ToSql)
}

macro_rules! impl_pg_esql {
    ($ty:ty) => {
        impl Esql for $ty {
            type Error = tokio_postgres::Error;
            type Dialect = crate::dialect::Postgres;

            fn esql(&mut self) -> EsqlDriver<'_, Self> {
                EsqlDriver(self)
            }

            fn _esql_execute(
                &mut self,
                sql: String,
                params: Vec<Value>,
            ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_ {
                async move {
                    self.execute_raw(&sql, slice_iter(&params))
                        .await
                        .map_err(Error::Driver)
                }
            }

            fn _esql_query<T>(
                &mut self,
                sql: String,
                params: Vec<Value>,
            ) -> impl Future<Output = Result<Vec<T>, Error<Self::Error>>> + '_
            where
                T: FromRow,
            {
                async move {
                    self.query_raw(&sql, slice_iter(&params))
                        .await
                        .map_err(Error::Driver)?
                        .map_ok(|row| <T as FromRow>::from_row(&row).map_err(Error::FromRow))
                        .try_collect::<Vec<_>>()
                        .await
                        .map_err(Error::Driver)?
                        .into_iter()
                        .collect::<Result<Vec<_>, _>>()
                }
            }
        }
    };
}

impl_pg_esql!(Client);
impl_pg_esql!(Transaction<'_>);
