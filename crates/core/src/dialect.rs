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

    /// Take the session lock that serialises migrations across processes.
    #[cfg(feature = "migrate")]
    const LOCK: &'static str;

    /// Release [Dialect::LOCK].
    #[cfg(feature = "migrate")]
    const UNLOCK: &'static str;
}

/// Positional `$1`, `$2`, ... placeholders.
pub struct Postgres;

impl Dialect for Postgres {
    fn placeholder(index: usize, out: &mut String) {
        let _ = write!(out, "${index}");
    }

    // The key is arbitrary but has to be stable: it is the whole agreement
    // between two processes that they are waiting on the same thing.
    #[cfg(feature = "migrate")]
    const LOCK: &'static str = "SELECT pg_advisory_lock(4359270142058781)";

    #[cfg(feature = "migrate")]
    const UNLOCK: &'static str = "SELECT pg_advisory_unlock(4359270142058781)";
}

/// Positional `?` placeholders.
pub struct MySql;

impl Dialect for MySql {
    fn placeholder(_: usize, out: &mut String) {
        out.push('?');
    }

    // A negative timeout waits indefinitely, so the lock either is held or the
    // statement is still running.
    #[cfg(feature = "migrate")]
    const LOCK: &'static str = "SELECT GET_LOCK('esql_migrations', -1)";

    #[cfg(feature = "migrate")]
    const UNLOCK: &'static str = "SELECT RELEASE_LOCK('esql_migrations')";
}
