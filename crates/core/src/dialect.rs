use std::fmt::Write as _;

/// How a database spells parameter placeholders.
///
/// This is carried as an associated type on [crate::Esql] rather than being
/// read from a cargo feature, so which database a [crate::Query] is built for
/// is a property of the connection it runs on. Enabling both drivers in one
/// binary is therefore fine.
pub trait Dialect {
    /// Append the placeholder for a parameter at the given 1-based position.
    fn placeholder(index: usize, out: &mut String);
}

/// Positional `$1`, `$2`, ... placeholders.
pub struct Postgres;

impl Dialect for Postgres {
    fn placeholder(index: usize, out: &mut String) {
        let _ = write!(out, "${index}");
    }
}

/// Positional `?` placeholders.
pub struct MySql;

impl Dialect for MySql {
    fn placeholder(_: usize, out: &mut String) {
        out.push('?');
    }
}
