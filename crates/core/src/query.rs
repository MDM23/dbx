use std::borrow::Cow;
use std::iter;

use crate::{
    ParamCountError, Value,
    dialect::Dialect,
    lexer::{self, Piece},
};

/// A wrapper certifying that the contained SQL string is safe to embed
/// directly (i.e. not user-supplied input that needs parameterisation).
#[repr(transparent)]
pub struct Trusted<'a>(Cow<'a, str>);

impl<'a> From<&'static str> for Trusted<'a> {
    fn from(value: &'static str) -> Self {
        Self(Cow::Borrowed(value))
    }
}

impl<'a> Trusted<'a> {
    /// Wraps a dynamic string as trusted SQL. Use this only for SQL that
    /// originates from a safe source (e.g. migration files read from disk at
    /// compile time, or column/table names from internal constants). For
    /// static string literals, the `From<&'static str>` conversion is
    /// preferred.
    ///
    /// Accepts both `&str` (borrowed) and `String` (owned).
    pub fn unchecked(value: impl Into<Cow<'a, str>>) -> Self {
        Trusted(value.into())
    }
}

#[derive(Debug)]
#[allow(dead_code)]
pub(crate) enum Fragment<'a> {
    Raw(Cow<'a, str>),
    Param,
}

impl Fragment<'_> {
    fn into_owned(self) -> Fragment<'static> {
        match self {
            Self::Param => Fragment::Param,
            Self::Raw(text) => Fragment::Raw(Cow::Owned(text.into_owned())),
        }
    }
}

#[derive(Debug, Default)]
pub struct Query<'a> {
    pub(crate) fragments: Vec<Fragment<'a>>,
    pub(crate) params: Vec<Value>,
}

////////////////////////////////////////////////////////////////////////////////
//                   M U T A T I N G   F U N C T I O N S
////////////////////////////////////////////////////////////////////////////////

impl<'a> Query<'a> {
    /// Modifies the current query instance by appending the given fragment. You
    /// may pass anything that can be turned into a valid query.
    pub fn push(&mut self, query: impl Into<Query<'a>>) -> &mut Self {
        let Query { fragments, params } = query.into();

        self.fragments.extend(fragments);
        self.params.extend(params);

        self
    }

    /// Modifies the current query by appending a given fragment with a
    /// specified prefix. The prefix only gets appended if the provided fragment
    /// is not empty.
    pub fn push_with_prefix(
        &mut self,
        prefix: &'static str,
        query: impl Into<Query<'a>>,
    ) -> &mut Self {
        let Query { fragments, params } = query.into();

        if !fragments.is_empty() {
            self.fragments.push(Fragment::Raw(Cow::Borrowed(prefix)));
            self.fragments.extend(fragments);
            self.params.extend(params);
        }

        self
    }

    /// Shortcut for [Query::push_with_prefix] with a prefix of "HAVING".
    pub fn push_having(&mut self, query: impl Into<Query<'a>>) -> &mut Self {
        self.push_with_prefix("HAVING", query)
    }

    /// Shortcut for [Query::push_with_prefix] with a prefix of "WHERE".
    pub fn push_where(&mut self, query: impl Into<Query<'a>>) -> &mut Self {
        self.push_with_prefix("WHERE", query)
    }
}

////////////////////////////////////////////////////////////////////////////////
//                   B U I L D E R   F U N C T I O N S
////////////////////////////////////////////////////////////////////////////////

impl<'a> Query<'a> {
    /// Creates a new and empty [Query] instance.
    pub fn new() -> Self {
        Self {
            fragments: Vec::new(),
            params: Vec::new(),
        }
    }

    /// Creates a new [Query] instance from the result of the provided closure.
    /// If the resulting query is not empty, it gets encapsulated using
    /// parentheses.
    pub fn group(query: impl Into<Query<'a>>) -> Self {
        let query = query.into();

        if query.fragments.is_empty() {
            return query;
        }

        Query {
            params: query.params,
            fragments: iter::once(Fragment::Raw(Cow::Borrowed("(")))
                .chain(query.fragments)
                .chain(iter::once(Fragment::Raw(Cow::Borrowed(")"))))
                .collect(),
        }
    }

    /// Creates a new subquery that just selects the specified column from the
    /// current query.
    pub fn pluck(self, column: &'static str) -> Query<'a> {
        let mut outer = Query::from("SELECT");
        outer.push(column);
        outer.push("FROM");
        outer.push(Query::group(self));
        outer
    }

    pub fn in_(subject: &'static str, params: impl IntoIterator<Item = impl Into<Value>>) -> Self {
        let params: Vec<_> = params.into_iter().map(Into::into).collect();

        if params.is_empty() {
            return Query::from("1=0");
        }

        let mut fragments = vec![
            Fragment::Raw(Cow::Borrowed(subject)),
            Fragment::Raw(Cow::Borrowed("IN (")),
        ];

        for i in 0..params.len() {
            if i > 0 {
                fragments.push(Fragment::Raw(Cow::Borrowed(",")));
            }

            fragments.push(Fragment::Param);
        }

        fragments.push(Fragment::Raw(Cow::Borrowed(")")));

        Query { params, fragments }
    }

    pub fn and(mut self, query: impl Into<Query<'a>>) -> Self {
        let query = query.into();

        if !self.fragments.is_empty() {
            self.push_with_prefix("AND", query);
        } else {
            self.push(query);
        }

        self
    }

    pub fn or(mut self, query: impl Into<Query<'a>>) -> Self {
        let query = query.into();

        if !self.fragments.is_empty() {
            self.push_with_prefix("OR", query);
        } else {
            self.push(query);
        }

        self
    }
}

////////////////////////////////////////////////////////////////////////////////
//                   I N T E R N A L   B U I L D
////////////////////////////////////////////////////////////////////////////////

impl<'a> Query<'a> {
    /// Build the query into a SQL string and a parameter vector, spelling
    /// placeholders the way `D` requires.
    ///
    /// The driver methods on [crate::EsqlDriver] pick `D` from the connection,
    /// so this only needs naming directly when building SQL by hand.
    ///
    /// # Errors
    ///
    /// [ParamCountError] if the query holds a different number of placeholders
    /// than it does parameters. Every caller routes through here, so a
    /// statement the server would reject never leaves the process.
    pub fn build<D: Dialect>(self) -> Result<(String, Vec<Value>), ParamCountError> {
        let mut sql = String::with_capacity(64);
        let mut placeholders = 0usize;

        for fragment in self.fragments {
            if !sql.is_empty() {
                sql.push(' ');
            }

            match fragment {
                Fragment::Param => {
                    placeholders += 1;
                    D::placeholder(placeholders, &mut sql);
                }
                Fragment::Raw(text) => sql.push_str(&text),
            }
        }

        if placeholders != self.params.len() {
            return Err(ParamCountError {
                placeholders,
                params: self.params.len(),
            });
        }

        Ok((sql, self.params))
    }
}

////////////////////////////////////////////////////////////////////////////////
//                   Q U E R Y   P A R S I N G
////////////////////////////////////////////////////////////////////////////////

fn parse(sql: &str) -> Vec<Fragment<'_>> {
    lexer::scan(sql)
        .into_iter()
        .map(|piece| match piece {
            Piece::Param => Fragment::Param,
            Piece::Raw(range) => Fragment::Raw(Cow::Borrowed(&sql[range])),
            Piece::Escaped(text) => Fragment::Raw(Cow::Owned(text)),
        })
        .collect()
}

fn make_query<'a, T>(statement: T, params: Vec<Value>) -> Query<'a>
where
    T: Into<Trusted<'a>>,
{
    let fragments = match statement.into().0 {
        Cow::Borrowed(sql) => parse(sql),
        // The source string dies with this function, so its fragments have to
        // take their text with them.
        Cow::Owned(sql) => parse(&sql).into_iter().map(Fragment::into_owned).collect(),
    };

    Query { fragments, params }
}

impl<'a, T> From<T> for Query<'a>
where
    T: Into<Trusted<'a>>,
{
    fn from(value: T) -> Self {
        make_query(value, Vec::new())
    }
}

macro_rules! impl_from_tuple {
    ($($idx:tt: $T:ident),+) => {
        impl<'a, S, $($T),+> From<(S, $($T),+)> for Query<'a>
        where
            S: Into<Trusted<'a>>,
            $($T: Into<Value>),+
        {
            fn from(value: (S, $($T),+)) -> Self {
                make_query(value.0, vec![$(value.$idx.into()),+])
            }
        }
    };
}

impl_from_tuple!(1: A1);
impl_from_tuple!(1: A1, 2: A2);
impl_from_tuple!(1: A1, 2: A2, 3: A3);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7, 8: A8);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7, 8: A8, 9: A9);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7, 8: A8, 9: A9, 10: A10);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7, 8: A8, 9: A9, 10: A10, 11: A11);
impl_from_tuple!(1: A1, 2: A2, 3: A3, 4: A4, 5: A5, 6: A6, 7: A7, 8: A8, 9: A9, 10: A10, 11: A11, 12: A12);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialect::{MySql, Postgres};

    #[test]
    fn basic_query_parsing() {
        let mut q1 = Query::from((
            "SELECT foo FROM bar WHERE a = ? AND b = ? AND c = \"hello?\"",
            1,
            2,
        ));

        q1.push(("AND d != ?", 13.37f32));

        let (sql, params) = q1.build::<Postgres>().unwrap();
        assert_eq!(params.len(), 3);
        assert!(sql.contains("\"hello?\""));
    }

    #[test]
    fn in_clause() {
        let mut q2 = Query::from("SELECT * FROM users");
        q2.push_where(Query::in_("type", ["admin", "moderator"]));

        let (sql, params) = q2.build::<Postgres>().unwrap();
        assert_eq!(params.len(), 2);
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("IN"));
    }

    #[test]
    fn empty_in_clause() {
        let q = Query::in_("type", Vec::<String>::new());
        let (sql, _) = q.build::<Postgres>().unwrap();
        assert!(sql.contains("1=0"));
    }

    /// The same query builds for either database. Before the dialect became a
    /// value this was a cargo feature, so one binary could only ever produce
    /// one of these two strings.
    #[test]
    fn one_query_builds_for_both_dialects() {
        let build = || Query::from(("SELECT a WHERE b = ? AND c = ?", 1, 2));

        let (pg, pg_params) = build().build::<Postgres>().unwrap();
        let (my, my_params) = build().build::<MySql>().unwrap();

        assert_eq!(pg, "SELECT a WHERE b = $1 AND c = $2");
        assert_eq!(my, "SELECT a WHERE b = ? AND c = ?");
        assert_eq!(pg_params.len(), 2);
        assert_eq!(my_params.len(), 2);
    }

    #[test]
    fn postgres_placeholders_count_from_one_across_pushes() {
        let mut q = Query::from(("SELECT ?", 1));
        q.push(("AND a = ?", 2));
        q.push(("AND b = ?", 3));

        let (sql, _) = q.build::<Postgres>().unwrap();
        assert_eq!(sql, "SELECT $1 AND a = $2 AND b = $3");
    }

    #[test]
    fn param_count_mismatch_does_not_build() {
        assert!(Query::from(("SELECT ?, ?", 1)).build::<Postgres>().is_err());
        assert!(Query::from(("SELECT ?", 1, 2)).build::<Postgres>().is_err());
    }

    /// An escaped `?` is text, so it must not be counted as a placeholder.
    #[test]
    fn escaped_placeholders_do_not_count_as_params() {
        let (sql, params) = Query::from(("SELECT * FROM d WHERE data ?? ? AND id = ?", "key", 1))
            .build::<Postgres>()
            .unwrap();

        assert_eq!(sql, "SELECT * FROM d WHERE data ? $1 AND id = $2");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn owned_sql_keeps_its_text() {
        let sql = format!("SELECT {} WHERE a = ?", "name");
        let (sql, _) = Query::from((Trusted::unchecked(sql), 1))
            .build::<Postgres>()
            .unwrap();

        assert_eq!(sql, "SELECT name WHERE a = $1");
    }
}
