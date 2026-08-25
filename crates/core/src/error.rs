use thiserror::Error;

/// Everything a query can fail with, parameterised by the driver's own error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error<DriverError> {
    #[error(transparent)]
    Driver(DriverError),

    #[error(transparent)]
    FromRow(#[from] FromRowError),

    #[error(transparent)]
    ParamCount(#[from] ParamCountError),

    /// A parameter whose type the target database has no representation for,
    /// such as an array bound against MySQL.
    #[error("driver cannot bind a parameter of type {0}")]
    UnsupportedParam(&'static str),

    #[cfg(feature = "migrate")]
    #[error(transparent)]
    Migration(#[from] MigrationError),
}

/// A query carrying a different number of placeholders than parameters, which
/// the server would reject.
#[derive(Debug, Error)]
#[error("query has {placeholders} placeholders but {params} parameters were bound")]
#[non_exhaustive]
pub struct ParamCountError {
    pub placeholders: usize,
    pub params: usize,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FromRowError {
    #[error("column not found: {0}")]
    ColumnNotFound(String),

    #[error("type mismatch: expected {expected}, got {got}")]
    TypeMismatch { expected: &'static str, got: String },

    #[error("query returned no rows")]
    NoRows,

    #[error("invalid UTF-8 in byte column")]
    Utf8Error,
}

#[cfg(feature = "migrate")]
pub use __migrate::{MigrationError, MigrationErrorKind};

#[cfg(feature = "migrate")]
mod __migrate {
    use std::num::ParseIntError;
    use thiserror::Error;

    #[derive(Debug, Error)]
    #[error("in migration {filename}")]
    pub struct MigrationError {
        #[source]
        pub error: MigrationErrorKind,
        pub filename: String,
    }

    #[derive(Debug, Error)]
    #[non_exhaustive]
    pub enum MigrationErrorKind {
        #[error("filename is not <version>_<name>.sql")]
        FilenameError,

        #[error("checksum differs from the migration that was applied")]
        ChecksumError,

        #[error("version is not a number")]
        ParseIntError(#[from] ParseIntError),

        #[error("cannot be read")]
        IOError(#[from] std::io::Error),
    }
}
