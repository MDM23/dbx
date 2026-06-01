#[cfg(all(feature = "mysql", feature = "postgres"))]
compile_error!("Features `mysql` and `postgres` are mutually exclusive. Enable only one.");

pub use crate::driver::{Esql, EsqlDriver, FromRow, FromValue, Row, RowIndex};
pub use crate::error::{Error, FromRowError};
pub use crate::query::{Query, Trusted};
pub use crate::value::Value;

#[cfg(feature = "migrate")]
pub use crate::error::{MigrationError, MigrationErrorKind};

mod driver;
mod error;
mod query;
mod value;
