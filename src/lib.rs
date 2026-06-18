#[cfg(all(feature = "mysql", feature = "postgres"))]
compile_error!("Features `mysql` and `postgres` are mutually exclusive. Enable only one.");

#[cfg(not(any(feature = "mysql", feature = "postgres")))]
compile_error!("Either feature `mysql` or `postgres` must be enabled.");

pub use esql_core::{
    Error, Esql, EsqlDriver, FromRow, FromRowError, FromValue, Query, Row, Trusted, Value,
};

#[cfg(feature = "derive")]
pub use esql_macros::FromRow;

#[cfg(feature = "migrate")]
pub mod migrate {
    pub use esql_macros::embed_migrations;
    pub use esql_migrate::{Migration, Migrator};
}
