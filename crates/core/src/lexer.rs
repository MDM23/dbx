//! Where SQL text is structural and where it is opaque.
//!
//! Two callers need the same answer: the query builder, to find placeholders,
//! and the migrator, to find statement boundaries. Both have to skip string
//! literals, comments and dollar-quoted bodies, so both walk the same
//! primitives.

use std::ops::Range;

/// A piece of a parsed statement.
///
/// Raw pieces are ranges into the source rather than slices, so that the same
/// scan serves a borrowed `&'static str` and an owned `String` without either
/// paying for the other.
pub(crate) enum Piece {
    Raw(Range<usize>),
    /// A run that contained `??`, already collapsed to a single `?`. Splicing
    /// it here rather than splitting the run keeps the escaped `?` glued to
    /// what follows, which is what `?|` and `?&` need.
    Escaped(String),
    Param,
}

/// Splits SQL at its unquoted `?` placeholders.
///
/// `??` is an escaped literal `?`, which is how the Postgres jsonb existence
/// operators `?`, `?|` and `?&` are written. Comments are dropped: they end
/// the current run, and runs rejoin with a single space, which is precisely
/// what a comment means to the server.
pub(crate) fn scan(sql: &str) -> Vec<Piece> {
    let bytes = sql.as_bytes();
    let mut pieces = Vec::new();
    let mut escaped = None;
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_quoted(bytes, i),
            b'-' if bytes.get(i + 1) == Some(&b'-') => {
                flush(&mut pieces, &mut escaped, sql, start..i);
                i = skip_line_comment(bytes, i);
                start = i;
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                flush(&mut pieces, &mut escaped, sql, start..i);
                i = skip_block_comment(bytes, i);
                start = i;
            }
            b'$' => i = skip_dollar_quoted(bytes, i).unwrap_or(i + 1),
            b'?' if bytes.get(i + 1) == Some(&b'?') => {
                escaped
                    .get_or_insert_with(String::new)
                    .push_str(&sql[start..=i]);

                i += 2;
                start = i;
            }
            b'?' => {
                flush(&mut pieces, &mut escaped, sql, start..i);
                pieces.push(Piece::Param);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }

    flush(&mut pieces, &mut escaped, sql, start..bytes.len());
    pieces
}

/// Splits a script at its top-level semicolons, so that a `;` inside a string
/// literal, a comment or a `$$` body does not end a statement.
#[cfg(feature = "migrate")]
pub fn split_statements(sql: &str) -> Vec<&str> {
    let bytes = sql.as_bytes();
    let mut statements = Vec::new();
    let mut start = 0;
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b'\'' | b'"' | b'`' => i = skip_quoted(bytes, i),
            b'-' if bytes.get(i + 1) == Some(&b'-') => i = skip_line_comment(bytes, i),
            b'/' if bytes.get(i + 1) == Some(&b'*') => i = skip_block_comment(bytes, i),
            b'$' => i = skip_dollar_quoted(bytes, i).unwrap_or(i + 1),
            b';' => {
                push_statement(&mut statements, &sql[start..i]);
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }

    push_statement(&mut statements, &sql[start..]);
    statements
}

#[cfg(feature = "migrate")]
fn push_statement<'a>(statements: &mut Vec<&'a str>, statement: &'a str) {
    let statement = statement.trim();

    if !statement.is_empty() {
        statements.push(statement);
    }
}

fn flush(pieces: &mut Vec<Piece>, escaped: &mut Option<String>, sql: &str, run: Range<usize>) {
    match escaped.take() {
        Some(mut text) => {
            text.push_str(&sql[run]);
            text.truncate(text.trim_end().len());
            text.replace_range(..text.len() - text.trim_start().len(), "");

            if !text.is_empty() {
                pieces.push(Piece::Escaped(text));
            }
        }
        None => {
            let text = &sql[run.clone()];
            let start = run.start + (text.len() - text.trim_start().len());
            let text = text.trim();

            if !text.is_empty() {
                pieces.push(Piece::Raw(start..start + text.len()));
            }
        }
    }
}

// Every delimiter below is ASCII, and no byte of a multi-byte UTF-8 sequence
// is, so scanning bytes never lands inside a character. Each function takes
// the index of the opening delimiter and returns the index just past the
// closing one, or the end of input where the construct is unterminated.

fn skip_quoted(bytes: &[u8], open: usize) -> usize {
    let quote = bytes[open];
    let mut i = open + 1;

    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            c if c == quote => {
                // A doubled quote is an escaped quote, not the end.
                if bytes.get(i + 1) == Some(&quote) {
                    i += 2;
                } else {
                    return i + 1;
                }
            }
            _ => i += 1,
        }
    }

    bytes.len()
}

fn skip_line_comment(bytes: &[u8], open: usize) -> usize {
    match bytes[open..].iter().position(|&c| c == b'\n') {
        Some(offset) => open + offset + 1,
        None => bytes.len(),
    }
}

fn skip_block_comment(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;

    while i + 1 < bytes.len() {
        match (bytes[i], bytes[i + 1]) {
            // Postgres nests block comments; MySQL does not, but a nested one
            // there is malformed either way.
            (b'/', b'*') => {
                depth += 1;
                i += 2;
            }
            (b'*', b'/') => {
                depth -= 1;
                i += 2;

                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }

    bytes.len()
}

/// Returns [None] when the `$` opens no dollar-quoted body, which is the case
/// for a Postgres `$1` placeholder written out by hand.
fn skip_dollar_quoted(bytes: &[u8], open: usize) -> Option<usize> {
    let mut i = open + 1;

    while bytes
        .get(i)
        .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
    {
        // The tag follows the rules for an unquoted identifier.
        if i == open + 1 && bytes[i].is_ascii_digit() {
            return None;
        }

        i += 1;
    }

    if bytes.get(i) != Some(&b'$') {
        return None;
    }

    let tag = &bytes[open..=i];
    let body = &bytes[i + 1..];

    Some(
        body.windows(tag.len())
            .position(|window| window == tag)
            .map_or(bytes.len(), |offset| i + 1 + offset + tag.len()),
    )
}

// -----------------------------------------------------------------------------
//                                    TESTS
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Renders a scan the way `Query::build` would, with `!` standing in for a
    /// placeholder, so a test reads as the SQL that reaches the server.
    fn rebuild(sql: &str) -> String {
        let mut out = String::new();

        for piece in scan(sql) {
            if !out.is_empty() {
                out.push(' ');
            }

            match piece {
                Piece::Param => out.push('!'),
                Piece::Raw(range) => out.push_str(&sql[range]),
                Piece::Escaped(text) => out.push_str(&text),
            }
        }

        out
    }

    #[test]
    fn placeholders_split_outside_quotes() {
        assert_eq!(rebuild("a = ? AND b = ?"), "a = ! AND b = !");
        assert_eq!(rebuild("a = '?' AND b = ?"), "a = '?' AND b = !");
        assert_eq!(
            rebuild(r#"a = "he said ?" AND b = ?"#),
            r#"a = "he said ?" AND b = !"#
        );
        assert_eq!(rebuild("a = `x?` AND b = ?"), "a = `x?` AND b = !");
        assert_eq!(
            rebuild("a = 'it''s ?' AND b = ?"),
            "a = 'it''s ?' AND b = !"
        );
    }

    /// The four rewrites recorded as broken in the roadmap.
    #[test]
    fn jsonb_operators_and_comments_survive() {
        assert_eq!(rebuild("WHERE data ?? 'key'"), "WHERE data ? 'key'");
        assert_eq!(rebuild("tags ??| array['x']"), "tags ?| array['x']");
        assert_eq!(
            rebuild("data ??& array['x'] AND b = ?"),
            "data ?& array['x'] AND b = !"
        );
        assert_eq!(rebuild("-- is this ok?\n AND a = ?"), "AND a = !");
        assert_eq!(rebuild("$$ body with ? $$"), "$$ body with ? $$");
    }

    #[test]
    fn comments_become_whitespace() {
        assert_eq!(rebuild("SELECT a /* note */ FROM t"), "SELECT a FROM t");
        assert_eq!(rebuild("SELECT a/*/* deep */*/b"), "SELECT a b");
        assert_eq!(rebuild("SELECT a -- trailing"), "SELECT a");
        assert_eq!(
            rebuild("SELECT ? -- ?\nWHERE b = ?"),
            "SELECT ! WHERE b = !"
        );
    }

    #[test]
    fn hand_written_postgres_placeholders_are_not_dollar_quotes() {
        assert_eq!(rebuild("a = $1 AND b = ?"), "a = $1 AND b = !");
        assert_eq!(
            rebuild("$tag$ ? $tag$ AND b = ?"),
            "$tag$ ? $tag$ AND b = !"
        );
    }

    #[test]
    fn unterminated_constructs_do_not_panic() {
        rebuild("SELECT 'unclosed");
        rebuild("SELECT /* unclosed");
        rebuild("SELECT $$ unclosed");
        rebuild("SELECT 'trailing backslash \\");
    }

    #[test]
    fn multibyte_text_is_not_split_mid_character() {
        assert_eq!(
            rebuild("SELECT 'grüße 🎉' WHERE a = ?"),
            "SELECT 'grüße 🎉' WHERE a = !"
        );
    }

    #[cfg(feature = "migrate")]
    #[test]
    fn statements_split_on_top_level_semicolons_only() {
        assert_eq!(
            split_statements("CREATE TABLE a (x INT); INSERT INTO a VALUES (1);"),
            ["CREATE TABLE a (x INT)", "INSERT INTO a VALUES (1)"]
        );

        assert_eq!(
            split_statements("INSERT INTO a VALUES ('semi; colon'); SELECT 1"),
            ["INSERT INTO a VALUES ('semi; colon')", "SELECT 1"]
        );

        assert_eq!(
            split_statements(
                "CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql;"
            ),
            ["CREATE FUNCTION f() RETURNS int AS $$ BEGIN RETURN 1; END; $$ LANGUAGE plpgsql"]
        );

        assert_eq!(
            split_statements("SELECT 1 -- ; not a split\n"),
            ["SELECT 1 -- ; not a split"]
        );
        assert!(split_statements("   \n  ").is_empty());
    }
}
