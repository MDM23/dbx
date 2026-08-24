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
            mysql_async::Value::Bytes(b) => Value::Bytes(b.to_vec()),
            mysql_async::Value::Int(i) => Value::I64(*i),
            mysql_async::Value::UInt(u) => Value::I64(*u as i64),
            mysql_async::Value::Float(f) => Value::F32(*f),
            mysql_async::Value::Double(d) => Value::F64(*d),
            _ => {
                return Err(FromRowError::TypeMismatch {
                    expected: "supported type",
                    got: format!("{raw:?}"),
                });
            }
        };

        FromValue::from_value(value)
    }
}

impl Esql for Pool {
    type Error = mysql_async::Error;

    fn esql(&mut self) -> EsqlDriver<'_, Self> {
        EsqlDriver(self)
    }

    fn _esql_execute(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_ {
        async move { Ok(build_query(sql, params).run(&*self).await?.affected_rows()) }
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
            build_query(sql, params)
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

    fn esql(&mut self) -> EsqlDriver<'_, Self> {
        EsqlDriver(self)
    }

    fn _esql_execute(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_ {
        async move {
            Ok(build_query(sql, params)
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
            build_query(sql, params)
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

fn build_params(input: Vec<Value>) -> Params {
    let positional: Vec<_> = input
        .into_iter()
        .map(|val| match val {
            Value::Null => mysql_async::Value::NULL,
            Value::Bool(b) => mysql_async::Value::Int(if b { 1 } else { 0 }),
            Value::F32(f) => mysql_async::Value::Float(f),
            Value::F64(f) => mysql_async::Value::Double(f),
            Value::I32(i) => mysql_async::Value::Int(i as i64),
            Value::I64(i) => mysql_async::Value::Int(i),
            Value::String(s) => mysql_async::Value::Bytes(s.into_bytes()),
            Value::Bytes(b) => mysql_async::Value::Bytes(b),
        })
        .collect();

    if positional.is_empty() {
        Params::Empty
    } else {
        Params::Positional(positional)
    }
}

fn build_query(sql: String, params: Vec<Value>) -> QueryWithParams<String, Params> {
    QueryWithParams {
        query: sql,
        params: build_params(params),
    }
}
