// The trait returns `impl Future + '_`, which `async fn` cannot express for
// the generic query without a `T: '_` bound.
#![allow(clippy::manual_async_fn)]

use futures_util::{StreamExt, TryStreamExt};
use mysql_async::{Params, Pool, QueryWithParams, Transaction, prelude::Query as _};

use crate::{
    Error, FromRow, FromRowError, Row, Value,
    driver::{Esql, EsqlDriver, FromValue, RowIndex},
};

impl From<mysql_async::Error> for Error<mysql_async::Error> {
    fn from(value: mysql_async::Error) -> Self {
        Self::Driver(value)
    }
}

impl Row for mysql_async::Row {
    fn try_get<'a, I, T>(&'a self, index: I) -> Result<T, FromRowError>
    where
        I: Into<RowIndex<'a>>,
        T: FromValue,
    {
        let subject_index = index.into();

        let columns = self.columns();
        let (col_idx, col) = columns
            .iter()
            .enumerate()
            .find(|(index, col)| match subject_index {
                RowIndex::Pos(i) => *index == i,
                RowIndex::Name(n) => col.name_str() == n,
            })
            .ok_or_else(|| match subject_index {
                RowIndex::Pos(i) => FromRowError::ColumnNotFound(i.to_string()),
                RowIndex::Name(n) => FromRowError::ColumnNotFound(n.to_string()),
            })?;

        let col_name = col.name_str().to_string();
        let raw = self
            .as_ref(col_idx)
            .ok_or_else(|| FromRowError::ColumnNotFound(col_name.clone()))?;

        let value = match raw {
            mysql_async::Value::NULL => Value::Null,
            // DECIMAL and JSON arrive as text; `FromValue` parses them on
            // request rather than guessing from the column type here.
            mysql_async::Value::Bytes(b) => Value::Bytes(b.to_vec()),
            mysql_async::Value::Int(i) => Value::I64(*i),
            mysql_async::Value::UInt(u) => Value::U64(*u),
            mysql_async::Value::Float(f) => Value::F32(*f),
            mysql_async::Value::Double(d) => Value::F64(*d),
            raw => temporal(raw)?,
        };

        FromValue::from_value(value)
    }
}

#[cfg(feature = "with-time-0_3")]
fn temporal(raw: &mysql_async::Value) -> Result<Value, FromRowError> {
    let unsupported = || FromRowError::TypeMismatch {
        expected: "date or time",
        got: format!("{raw:?}"),
    };

    match *raw {
        // DATE, DATETIME and TIMESTAMP all arrive through this one variant. A
        // bare DATE carries a zeroed time, which `FromValue` narrows back down
        // when the target type asks for it.
        mysql_async::Value::Date(y, mo, d, h, mi, s, us) => {
            let date = time::Date::from_calendar_date(
                y as i32,
                time::Month::try_from(mo).map_err(|_| unsupported())?,
                d,
            )
            .map_err(|_| unsupported())?;

            time::Time::from_hms_micro(h, mi, s, us)
                .map(|time| Value::DateTime(time::PrimitiveDateTime::new(date, time)))
                .map_err(|_| unsupported())
        }
        // MySQL TIME is a signed duration of up to ~838 hours. It only
        // coincides with a wall clock time inside a single positive day.
        mysql_async::Value::Time(false, 0, h, mi, s, us) => {
            time::Time::from_hms_micro(h, mi, s, us)
                .map(Value::Time)
                .map_err(|_| unsupported())
        }
        _ => Err(unsupported()),
    }
}

#[cfg(not(feature = "with-time-0_3"))]
fn temporal(raw: &mysql_async::Value) -> Result<Value, FromRowError> {
    Err(FromRowError::TypeMismatch {
        expected: "supported type (enable `with-time-0_3` for date and time columns)",
        got: format!("{raw:?}"),
    })
}

impl Esql for Pool {
    type Error = mysql_async::Error;
    type Dialect = crate::dialect::MySql;

    fn esql(&mut self) -> EsqlDriver<'_, Self> {
        EsqlDriver(self)
    }

    fn _esql_execute(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_ {
        async move { Ok(build_query(sql, params)?.run(&*self).await?.affected_rows()) }
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
            build_query(sql, params)?
                .stream::<mysql_async::Row, _>(&*self)
                .await
                .map_err(Error::Driver)?
                .map(|res| {
                    res.map_err(Error::Driver)
                        .and_then(|row| <T as FromRow>::from_row(&row).map_err(Error::FromRow))
                })
                .try_collect::<Vec<_>>()
                .await
        }
    }
}

impl Esql for Transaction<'_> {
    type Error = mysql_async::Error;
    type Dialect = crate::dialect::MySql;

    fn esql(&mut self) -> EsqlDriver<'_, Self> {
        EsqlDriver(self)
    }

    fn _esql_execute(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_ {
        async move {
            Ok(build_query(sql, params)?
                .run(&mut *self)
                .await?
                .affected_rows())
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
            build_query(sql, params)?
                .stream::<mysql_async::Row, _>(&mut *self)
                .await
                .map_err(Error::Driver)?
                .map(|res| {
                    res.map_err(Error::Driver)
                        .and_then(|row| <T as FromRow>::from_row(&row).map_err(Error::FromRow))
                })
                .try_collect::<Vec<_>>()
                .await
        }
    }
}

#[cfg(feature = "with-time-0_3")]
fn encode_datetime(dt: time::PrimitiveDateTime) -> mysql_async::Value {
    mysql_async::Value::Date(
        dt.year() as u16,
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second(),
        dt.microsecond(),
    )
}

fn encode<E>(value: Value) -> Result<mysql_async::Value, Error<E>> {
    use mysql_async::Value as My;

    Ok(match value {
        Value::Null => My::NULL,
        Value::Bool(b) => My::Int(b as i64),
        Value::Bytes(b) => My::Bytes(b),
        Value::F32(f) => My::Float(f),
        Value::F64(f) => My::Double(f),
        Value::I16(i) => My::Int(i as i64),
        Value::I32(i) => My::Int(i as i64),
        Value::I64(i) => My::Int(i),
        Value::String(s) => My::Bytes(s.into_bytes()),
        Value::U64(u) => My::UInt(u),
        // MySQL has no native type for these. Text is what the conventional
        // column definitions (VARCHAR, CHAR(36), JSON, DECIMAL) accept.
        Value::IpAddr(a) => My::Bytes(a.to_string().into_bytes()),
        #[cfg(feature = "with-rust_decimal-1")]
        Value::Decimal(d) => My::Bytes(d.to_string().into_bytes()),
        #[cfg(feature = "with-serde_json-1")]
        Value::Json(j) => My::Bytes(j.to_string().into_bytes()),
        #[cfg(feature = "with-uuid-1")]
        Value::Uuid(u) => My::Bytes(u.to_string().into_bytes()),
        #[cfg(feature = "with-time-0_3")]
        Value::Date(d) => My::Date(d.year() as u16, d.month() as u8, d.day(), 0, 0, 0, 0),
        #[cfg(feature = "with-time-0_3")]
        Value::DateTime(dt) => encode_datetime(dt),
        // A MySQL DATETIME column carries no zone, so the offset has to be
        // normalised away rather than silently dropped.
        #[cfg(feature = "with-time-0_3")]
        Value::OffsetDateTime(dt) => {
            let utc = dt.to_offset(time::UtcOffset::UTC);
            encode_datetime(time::PrimitiveDateTime::new(utc.date(), utc.time()))
        }
        #[cfg(feature = "with-time-0_3")]
        Value::Time(t) => My::Time(false, 0, t.hour(), t.minute(), t.second(), t.microsecond()),
        Value::Array(_) => return Err(Error::UnsupportedParam("array")),
    })
}

fn build_query<E>(
    sql: String,
    params: Vec<Value>,
) -> Result<QueryWithParams<String, Params>, Error<E>> {
    let params = if params.is_empty() {
        Params::Empty
    } else {
        Params::Positional(
            params
                .into_iter()
                .map(encode)
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    Ok(QueryWithParams { query: sql, params })
}
