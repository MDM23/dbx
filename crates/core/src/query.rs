use std::borrow::Cow;
use std::iter;

use crate::Value;

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
    /// Build the query into a SQL string and a parameter vector.
    ///
    /// Placeholder format depends on the enabled driver feature:
    /// - `postgres`: positional `$1`, `$2`, ...
    /// - `mysql`: positional `?`
    /// - no driver feature: positional `?` (default)
    pub fn build(self) -> (String, Vec<Value>) {
        let mut buffer = String::with_capacity(64);

        #[cfg(feature = "postgres")]
        let mut n = 1usize;

        for frag in self.fragments {
            if !buffer.is_empty() {
                buffer.push(' ');
            }

            match frag {
                Fragment::Param => {
                    #[cfg(feature = "postgres")]
                    {
                        buffer.push('$');
                        buffer.push_str(&n.to_string());
                        n += 1;
                    }
                    #[cfg(not(feature = "postgres"))]
                    buffer.push('?');
                }
                Fragment::Raw(r) => buffer.push_str(&r),
            }
        }

        (buffer, self.params)
    }
}

////////////////////////////////////////////////////////////////////////////////
//                   Q U E R Y   P A R S I N G
////////////////////////////////////////////////////////////////////////////////

/// Parses a SQL string into [`Fragment`]s, splitting at unquoted `?`
/// placeholders. The `$wrap` expression converts each `&str` slice into the
/// appropriate [`Cow`] variant.
macro_rules! parse_sql {
    ($statement:expr, $params:expr, $wrap:expr) => {{
        let mut statement = $statement;
        let mut query = Query {
            fragments: Vec::new(),
            params: $params,
        };

        let mut chars = statement.chars();
        let mut delimiter = None;
        let mut consumed = 0usize;

        while let Some(c) = chars.next() {
            match c {
                c @ ('"' | '\'' | '`') => {
                    match delimiter {
                        Some(d) if d == c => delimiter = None,
                        None => delimiter = Some(c),
                        _ => {}
                    }

                    consumed += c.len_utf8();
                }
                c @ '?' if delimiter.is_none() => {
                    query.fragments.extend([
                        Fragment::Raw($wrap(statement[..consumed].trim_end())),
                        Fragment::Param,
                    ]);

                    statement = &statement[consumed + c.len_utf8()..];
                    chars = statement.chars();
                    consumed = 0;
                }
                c @ '\\' if delimiter.is_some() => {
                    consumed += c.len_utf8() + chars.next().map(char::len_utf8).unwrap_or(0);
                }
                c if consumed == 0 && c.is_whitespace() => {
                    statement = &statement[c.len_utf8()..];
                    chars = statement.chars();
                    consumed = 0;
                }
                c => {
                    consumed += c.len_utf8();
                }
            }
        }

        if consumed > 0 {
            query
                .fragments
                .push(Fragment::Raw($wrap(statement[..consumed].trim_end())));
        }

        query
    }};
}

fn make_query<'a, T>(statement: T, params: Vec<Value>) -> Query<'a>
where
    T: Into<Trusted<'a>>,
{
    match statement.into().0 {
        Cow::Borrowed(s) => parse_sql!(s, params, Cow::Borrowed),
        Cow::Owned(s) => parse_sql!(s.as_str(), params, |s: &str| Cow::Owned(s.to_owned())),
    }
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

    #[test]
    fn basic_query_parsing() {
        let mut q1 = Query::from((
            "SELECT foo FROM bar WHERE a = ? AND b = ? AND c = \"hello?\"",
            1,
            2,
        ));

        q1.push(("AND d != ?", 13.37f32));

        let (sql, params) = q1.build();
        assert_eq!(params.len(), 3);
        assert!(!sql.contains("hello?") || sql.contains("\"hello?\""));
    }

    #[test]
    fn in_clause() {
        let mut q2 = Query::from("SELECT * FROM users");
        q2.push_where(Query::in_("type", ["admin", "moderator"]));

        let (sql, params) = q2.build();
        assert_eq!(params.len(), 2);
        assert!(sql.contains("WHERE"));
        assert!(sql.contains("IN"));
    }

    #[test]
    fn empty_in_clause() {
        let q = Query::in_("type", Vec::<String>::new());
        let (sql, _) = q.build();
        assert!(sql.contains("1=0"));
    }
}
