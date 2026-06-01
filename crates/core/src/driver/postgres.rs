use futures_util::TryStreamExt;
use tokio_postgres::{
    Client, Transaction,
    types::{ToSql, to_sql_checked},
};

use crate::{
    Error, FromRow, FromRowError, Row, Value,
    driver::{Esql, EsqlDriver, FromValue, RowIndex},
};

impl From<tokio_postgres::Error> for Error<tokio_postgres::Error> {
    fn from(value: tokio_postgres::Error) -> Self {
        Self::Driver(value)
    }
}

impl Row for tokio_postgres::Row {
    fn try_get<'a, I, T>(&'a self, index: I) -> Result<T, FromRowError>
    where
        I: Into<RowIndex<'a>>,
        T: FromValue,
    {
        let subject_index = index.into();

        let (column_index, column) = self
            .columns()
            .iter()
            .enumerate()
            .find(|(index, col)| match subject_index {
                RowIndex::Pos(i) => *index == i,
                RowIndex::Name(n) => col.name() == n,
            })
            .ok_or_else(|| match subject_index {
                RowIndex::Pos(i) => FromRowError::ColumnNotFound(i.to_string()),
                RowIndex::Name(n) => FromRowError::ColumnNotFound(n.to_string()),
            })?;

        let value = match column.type_().name() {
            "bool" => match self.try_get::<_, Option<bool>>(column_index) {
                Ok(Some(v)) => Value::Bool(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "bool",
                        got: e.to_string(),
                    });
                }
            },
            "int4" => match self.try_get::<_, Option<i32>>(column_index) {
                Ok(Some(v)) => Value::I32(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "i32",
                        got: e.to_string(),
                    });
                }
            },
            "int8" => match self.try_get::<_, Option<i64>>(column_index) {
                Ok(Some(v)) => Value::I64(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "i64",
                        got: e.to_string(),
                    });
                }
            },
            "float4" => match self.try_get::<_, Option<f32>>(column_index) {
                Ok(Some(v)) => Value::F32(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "f32",
                        got: e.to_string(),
                    });
                }
            },
            "float8" => match self.try_get::<_, Option<f64>>(column_index) {
                Ok(Some(v)) => Value::F64(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "f64",
                        got: e.to_string(),
                    });
                }
            },
            "text" | "varchar" => match self.try_get::<_, Option<String>>(column_index) {
                Ok(Some(v)) => Value::String(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "string",
                        got: e.to_string(),
                    });
                }
            },
            "bytea" => match self.try_get::<_, Option<Vec<u8>>>(column_index) {
                Ok(Some(v)) => Value::Bytes(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "bytes",
                        got: e.to_string(),
                    });
                }
            },
            #[cfg(feature = "with-time-0_3")]
            "timestamptz" => match self.try_get::<_, Option<time::OffsetDateTime>>(column_index) {
                Ok(Some(v)) => Value::OffsetDateTime(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "timestamptz",
                        got: e.to_string(),
                    });
                }
            },
            #[cfg(feature = "with-uuid-1")]
            "uuid" => match self.try_get::<_, Option<uuid::Uuid>>(column_index) {
                Ok(Some(v)) => Value::Uuid(v),
                Ok(None) => Value::Null,
                Err(e) => {
                    return Err(FromRowError::TypeMismatch {
                        expected: "uuid",
                        got: e.to_string(),
                    });
                }
            },
            other => {
                return Err(FromRowError::TypeMismatch {
                    expected: "supported type",
                    got: other.to_string(),
                });
            }
        };

        FromValue::from_value(value)
    }
}

impl ToSql for Value {
    fn to_sql(
        &self,
        ty: &tokio_postgres::types::Type,
        out: &mut tokio_postgres::types::private::BytesMut,
    ) -> Result<tokio_postgres::types::IsNull, Box<dyn std::error::Error + Sync + Send>>
    where
        Self: Sized,
    {
        match self {
            Value::Null => Ok(tokio_postgres::types::IsNull::Yes),
            Value::Bool(b) => b.to_sql(ty, out),
            Value::Bytes(b) => b.to_sql(ty, out),
            Value::F32(f) => f.to_sql(ty, out),
            Value::F64(f) => f.to_sql(ty, out),
            Value::I32(i) => i.to_sql(ty, out),
            Value::I64(i) => i.to_sql(ty, out),
            #[cfg(feature = "with-time-0_3")]
            Value::OffsetDateTime(dt) => dt.to_sql(ty, out),
            Value::String(s) => s.to_sql(ty, out),
            #[cfg(feature = "with-uuid-1")]
            Value::Uuid(u) => u.to_sql(ty, out),
        }
    }

    fn accepts(ty: &tokio_postgres::types::Type) -> bool
    where
        Self: Sized,
    {
        let name = ty.name();
        if matches!(
            name,
            "bool" | "bytea" | "float4" | "float8" | "int4" | "int8" | "text" | "varchar"
        ) {
            return true;
        }
        #[cfg(feature = "with-time-0_3")]
        if name == "timestamptz" {
            return true;
        }
        #[cfg(feature = "with-uuid-1")]
        if name == "uuid" {
            return true;
        }
        false
    }

    to_sql_checked! {}
}

fn slice_iter<'a>(s: &'a [Value]) -> impl ExactSizeIterator<Item = &'a dyn ToSql> + 'a {
    s.iter().map(|t| t as &dyn ToSql)
}

macro_rules! impl_pg_esql {
    ($ty:ty) => {
        impl Esql for $ty {
            type Error = tokio_postgres::Error;

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
