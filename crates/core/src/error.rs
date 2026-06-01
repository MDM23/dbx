#[derive(Debug)]
pub enum Error<DriverError> {
    FromRow(FromRowError),
    Driver(DriverError),

    #[cfg(feature = "migrate")]
    Migration(MigrationError),
}

impl<T: std::fmt::Display> std::fmt::Display for Error<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FromRow(e) => write!(f, "{e}"),
            Self::Driver(e) => write!(f, "{e}"),
            #[cfg(feature = "migrate")]
            Self::Migration(e) => write!(f, "migration error in {}: {:?}", e.filename, e.error),
        }
    }
}

impl<T: std::fmt::Debug + std::fmt::Display> std::error::Error for Error<T> {}

#[derive(Debug)]
pub enum FromRowError {
    ColumnNotFound(String),
    TypeMismatch { expected: &'static str, got: String },
    NoRows,
    NullValue(String),
    Utf8Error,
}

impl std::fmt::Display for FromRowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ColumnNotFound(col) => write!(f, "column not found: {col}"),
            Self::TypeMismatch { expected, got } => {
                write!(f, "type mismatch: expected {expected}, got {got}")
            }
            Self::NoRows => write!(f, "query returned no rows"),
            Self::NullValue(col) => write!(f, "unexpected null value in column: {col}"),
            Self::Utf8Error => write!(f, "invalid UTF-8 in byte column"),
        }
    }
}

impl std::error::Error for FromRowError {}

impl<T> From<FromRowError> for Error<T> {
    fn from(value: FromRowError) -> Self {
        Self::FromRow(value)
    }
}

#[cfg(feature = "migrate")]
pub use __migrate::{MigrationError, MigrationErrorKind};

#[cfg(feature = "migrate")]
mod __migrate {
    use super::Error;
    use std::num::ParseIntError;

    impl<T> From<MigrationError> for Error<T> {
        fn from(value: MigrationError) -> Self {
            Self::Migration(value)
        }
    }

    #[derive(Debug)]
    pub enum MigrationErrorKind {
        FilenameError,
        ChecksumError,
        ParseIntError(std::num::ParseIntError),
        IOError(std::io::Error),
    }

    impl From<ParseIntError> for MigrationErrorKind {
        fn from(value: ParseIntError) -> Self {
            Self::ParseIntError(value)
        }
    }

    impl From<std::io::Error> for MigrationErrorKind {
        fn from(value: std::io::Error) -> Self {
            Self::IOError(value)
        }
    }

    #[derive(Debug)]
    pub struct MigrationError {
        pub error: MigrationErrorKind,
        pub filename: String,
    }
}
