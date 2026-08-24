use crate::{Error, FromRowError, Query, Value, dialect::Dialect};

#[cfg(feature = "mysql")]
mod mysql;

#[cfg(feature = "postgres")]
mod postgres;

/// Thin wrapper that provides the esql query interface on database clients and
/// transactions. Obtain one via the [Esql::esql] method.
pub struct EsqlDriver<'c, C: ?Sized>(pub(crate) &'c mut C);

/// Extension trait implemented on database clients and transactions.
/// Call `.esql()` to obtain an [EsqlDriver] handle that provides
/// `execute`, `query`, and `first` methods.
pub trait Esql {
    type Error;

    /// How this connection spells parameter placeholders.
    type Dialect: Dialect;

    /// Returns an [EsqlDriver] handle for this connection.
    fn esql(&mut self) -> EsqlDriver<'_, Self>;

    #[doc(hidden)]
    fn _esql_execute(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<u64, Error<Self::Error>>> + '_;

    #[doc(hidden)]
    fn _esql_query<T>(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<Vec<T>, Error<Self::Error>>> + '_
    where
        T: FromRow;

    #[doc(hidden)]
    fn _esql_first<T>(
        &mut self,
        sql: String,
        params: Vec<Value>,
    ) -> impl Future<Output = Result<T, Error<Self::Error>>> + '_
    where
        T: FromRow,
    {
        async {
            self._esql_query(sql, params)
                .await?
                .into_iter()
                .next()
                .ok_or(FromRowError::NoRows.into())
        }
    }
}

impl<'c, C: Esql + ?Sized> EsqlDriver<'c, C> {
    /// Execute a statement and return the number of affected rows.
    pub fn execute<'q>(
        &mut self,
        query: impl Into<Query<'q>>,
    ) -> impl Future<Output = Result<u64, Error<C::Error>>> + '_ {
        let (sql, params) = query.into().build::<C::Dialect>();
        self.0._esql_execute(sql, params)
    }

    /// Execute a query and return all result rows.
    pub fn query<'q, T>(
        &mut self,
        query: impl Into<Query<'q>>,
    ) -> impl Future<Output = Result<Vec<T>, Error<C::Error>>> + '_
    where
        T: FromRow,
    {
        let (sql, params) = query.into().build::<C::Dialect>();
        self.0._esql_query(sql, params)
    }

    /// Execute a query and return the first row, or error with
    /// [FromRowError::NoRows] if the result set is empty.
    pub fn first<'q, T>(
        &mut self,
        query: impl Into<Query<'q>>,
    ) -> impl Future<Output = Result<T, Error<C::Error>>> + '_
    where
        T: FromRow,
    {
        let (sql, params) = query.into().build::<C::Dialect>();
        self.0._esql_first(sql, params)
    }
}

pub trait FromRow
where
    Self: Sized,
{
    fn from_row<R: Row>(row: &R) -> Result<Self, FromRowError>;
}

pub trait Row {
    fn try_get<'a, I, T>(&'a self, index: I) -> Result<T, FromRowError>
    where
        I: Into<RowIndex<'a>>,
        T: FromValue;
}

pub trait FromValue: Sized {
    fn from_value(value: Value) -> Result<Self, FromRowError>;
}

pub enum RowIndex<'a> {
    Pos(usize),
    Name(&'a str),
}

impl From<usize> for RowIndex<'_> {
    fn from(value: usize) -> Self {
        Self::Pos(value)
    }
}

impl<'a> From<&'a str> for RowIndex<'a> {
    fn from(value: &'a str) -> Self {
        Self::Name(value)
    }
}
