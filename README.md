# esql

A thin, composable SQL query builder for
[tokio-postgres](https://docs.rs/tokio-postgres) and
[mysql_async](https://docs.rs/mysql_async).

esql does not own your connection. Call `.esql()` on whatever you already have,
be it a client, a transaction or a pooled connection, and you get `execute`,
`query`, `first` and `first_optional` on it. There is no entity layer, no
relation mapping and no compile-time schema verification.

```rust
use esql::{Esql, FromRow, Query};

#[derive(FromRow)]
struct User {
    id: i64,
    name: String,
}

// `?` is the placeholder in the source regardless of the database. Which one
// reaches the server as `$1` and which as `?` is decided by the connection.
let users: Vec<User> = client
    .esql()
    .query(("SELECT id, name FROM users WHERE active = ?", true))
    .await?;

// Fragments compose, and an empty one takes its prefix with it, so a filter
// that is not set adds nothing.
let mut query = Query::from("SELECT id, name FROM users");
query.push_where(Query::in_("id", [1i64, 2, 3]).and(("name LIKE ?", "a%")));
query.push("ORDER BY name");

let users: Vec<User> = client.esql().query(query).await?;
```

## Safety

Anything that becomes SQL text has to be a `&'static str`, or be wrapped in
`Trusted::unchecked` at the call site. Everything else is a parameter. That is
the whole rule, and it is enforced by the type system rather than by review.

```rust
// Will not compile: the value is not a static string.
client.esql().query(format!("SELECT * FROM {table}")).await?;

// Compiles, and says at the call site who vouched for the string.
client.esql().query(Trusted::unchecked(format!("SELECT * FROM {table}"))).await?;
```

## Placeholders and the `?` operators

Since `?` marks a parameter, a literal `?` is written `??`. This is how the
Postgres jsonb operators are spelled:

```sql
SELECT * FROM docs WHERE data ?? 'key'
SELECT * FROM docs WHERE tags ??| array['a', 'b']
```

Comments and `$tag$` bodies are left alone, and a query whose placeholder count
does not match its parameter count fails before it is sent.

## Migrations

`embed_migrations!` bakes a directory of `<version>_<name>.sql` files into the
binary. `run` takes a session lock first, so concurrent boots apply them one at
a time, which means it needs a client or a transaction rather than a pool.

```rust
esql::migrate::embed_migrations!("migrations")
    .run(&mut client.esql())
    .await?;
```

## Features

| Feature | Adds |
| --- | --- |
| `postgres` | The `tokio-postgres` driver |
| `mysql` | The `mysql_async` driver |
| `derive` | `#[derive(FromRow)]` |
| `migrate` | `embed_migrations!` and the migrator |
| `with-rust_decimal-1` | `NUMERIC` and `DECIMAL` as `rust_decimal::Decimal` |
| `with-serde_json-1` | `JSON` and `JSONB` as `serde_json::Value` |
| `with-time-0_3` | Date and time columns as `time` types |
| `with-uuid-1` | `UUID` as `uuid::Uuid` |

Features are additive: both drivers can be enabled in one binary, and a `Query`
takes its placeholder style from the connection it runs on.

## Status

Pre-1.0. The read path still routes every column through an owned `Value`,
which is the one structural change still ahead: it costs an allocation per
column and rules out zero-copy reads.

## License

MIT
