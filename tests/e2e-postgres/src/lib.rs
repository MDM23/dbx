#[cfg(test)]
mod tests {
    use esql::{Esql, FromRow, Query};
    use tokio_postgres::NoTls;

    #[allow(dead_code)]
    #[derive(FromRow)]
    struct User {
        id: i64,
        name: String,
        active: bool,
    }

    fn base_url() -> String {
        std::env::var("POSTGRES_URL").unwrap_or_else(|_| "host=localhost user=postgres".to_string())
    }

    async fn admin_client() -> tokio_postgres::Client {
        let (client, conn) = tokio_postgres::connect(&base_url(), NoTls).await.unwrap();
        tokio::spawn(conn);
        client
    }

    struct TestDb {
        name: String,
        client: tokio_postgres::Client,
    }

    impl TestDb {
        async fn new(test_name: &str) -> Self {
            let name = format!("esql_test_{test_name}");
            let admin = admin_client().await;
            let _ = admin
                .execute(&format!("DROP DATABASE IF EXISTS {name}"), &[])
                .await;
            admin
                .execute(&format!("CREATE DATABASE {name}"), &[])
                .await
                .unwrap();

            let url = format!("{} dbname={name}", base_url());
            let (client, conn) = tokio_postgres::connect(&url, NoTls).await.unwrap();
            tokio::spawn(conn);

            Self { name, client }
        }

        async fn with_users_table(test_name: &str) -> Self {
            let db = Self::new(test_name).await;
            db.client
                .execute(
                    "CREATE TABLE IF NOT EXISTS users (
                        id      BIGINT PRIMARY KEY,
                        name    TEXT NOT NULL,
                        active  BOOLEAN NOT NULL DEFAULT TRUE
                    )",
                    &[],
                )
                .await
                .unwrap();
            db
        }
    }

    impl Drop for TestDb {
        fn drop(&mut self) {
            let name = self.name.clone();
            let base = base_url();
            std::thread::spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async {
                        let (c, conn) = tokio_postgres::connect(&base, NoTls).await.unwrap();
                        tokio::spawn(conn);
                        let _ = c
                            .execute(&format!("DROP DATABASE IF EXISTS {name}"), &[])
                            .await;
                    });
            })
            .join()
            .ok();
        }
    }

    #[tokio::test]
    async fn migrations() {
        let mut db = TestDb::new("migrations").await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut db.client.esql()).await.unwrap();

        let tables: Vec<String> = db
            .client
            .esql()
            .query(
                "SELECT table_name::text FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name IN ('migrations', 'users') \
                 ORDER BY table_name",
            )
            .await
            .unwrap();

        assert_eq!(tables, vec!["migrations", "users"]);
    }

    #[tokio::test]
    async fn insert_and_query() {
        let mut db = TestDb::with_users_table("insert_and_query").await;

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Alice",
                true,
            ))
            .await
            .unwrap();

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                2i64,
                "Bob",
                false,
            ))
            .await
            .unwrap();

        let users: Vec<User> = db
            .client
            .esql()
            .query("SELECT id, name, active FROM users ORDER BY id")
            .await
            .unwrap();

        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "Alice");
        assert!(users[0].active);
        assert_eq!(users[1].name, "Bob");
        assert!(!users[1].active);
    }

    #[tokio::test]
    async fn query_builder_with_params() {
        let mut db = TestDb::with_users_table("query_builder").await;

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Charlie",
                true,
            ))
            .await
            .unwrap();

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                2i64,
                "Diana",
                true,
            ))
            .await
            .unwrap();

        let mut q = Query::from("SELECT name FROM users");
        q.push_where(Query::in_("id", [1i64, 2]));
        q.push("ORDER BY name");

        let names: Vec<String> = db.client.esql().query(q).await.unwrap();
        assert_eq!(names, vec!["Charlie", "Diana"]);
    }

    #[tokio::test]
    async fn first_returns_single_row() {
        let mut db = TestDb::with_users_table("first").await;

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Eve",
                true,
            ))
            .await
            .unwrap();

        let name: String = db
            .client
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(name, "Eve");
    }

    #[tokio::test]
    async fn first_optional_returns_none_on_empty_result() {
        let mut db = TestDb::with_users_table("first_optional").await;

        let missing: Option<String> = db
            .client
            .esql()
            .first_optional(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(missing, None);

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Iris",
                true,
            ))
            .await
            .unwrap();

        let found: Option<String> = db
            .client
            .esql()
            .first_optional(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(found.as_deref(), Some("Iris"));
    }

    /// The jsonb existence operators are spelled `??`, `??|` and `??&`, since a
    /// bare `?` is a placeholder. Nothing else can tell them apart.
    #[tokio::test]
    async fn jsonb_existence_operators() {
        let mut db = TestDb::new("jsonb_ops").await;

        db.client
            .esql()
            .execute("CREATE TABLE d (id INTEGER, data JSONB)")
            .await
            .unwrap();

        db.client
            .esql()
            .execute((
                "INSERT INTO d VALUES (?, ?)",
                1i32,
                serde_json::json!({ "a": 1, "b": 2 }),
            ))
            .await
            .unwrap();

        let by_key: i64 = db
            .client
            .esql()
            .first(("SELECT COUNT(*) FROM d WHERE data ?? 'a' AND id = ?", 1i32))
            .await
            .unwrap();

        let by_any: i64 = db
            .client
            .esql()
            .first("SELECT COUNT(*) FROM d WHERE data ??| array['b', 'zz']")
            .await
            .unwrap();

        let by_all: i64 = db
            .client
            .esql()
            .first("SELECT COUNT(*) FROM d WHERE data ??& array['a', 'b']")
            .await
            .unwrap();

        assert_eq!((by_key, by_any, by_all), (1, 1, 1));
    }

    /// A comment used to eat the rest of the statement, because fragments
    /// rejoin with a space and the newline was lost with it.
    #[tokio::test]
    async fn comments_do_not_swallow_the_statement() {
        let mut db = TestDb::with_users_table("comments").await;

        db.client
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) -- is this ok?\n VALUES (?, ?, ?)",
                1i64,
                "Hank",
                true,
            ))
            .await
            .unwrap();

        let name: String = db
            .client
            .esql()
            .first("SELECT name /* the column */ FROM users")
            .await
            .unwrap();

        assert_eq!(name, "Hank");
    }

    #[tokio::test]
    async fn transaction_commit() {
        let mut db = TestDb::with_users_table("tx_commit").await;

        let mut tx = db.client.transaction().await.unwrap();
        tx.esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Frank",
                true,
            ))
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let name: String = db
            .client
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(name, "Frank");
    }

    #[tokio::test]
    async fn transaction_rollback() {
        let mut db = TestDb::with_users_table("tx_rollback").await;

        let mut tx = db.client.transaction().await.unwrap();
        tx.esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Ghost",
                true,
            ))
            .await
            .unwrap();
        tx.rollback().await.unwrap();

        let count: i64 = db
            .client
            .esql()
            .first(("SELECT COUNT(*) FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(count, 0);
    }

    #[derive(FromRow, PartialEq, Debug)]
    struct Scalars {
        c_bool: bool,
        c_int2: i16,
        c_int4: i32,
        c_int8: i64,
        c_float4: f32,
        c_float8: f64,
        c_numeric: rust_decimal::Decimal,
        c_text: String,
        c_varchar: String,
        c_bytea: Vec<u8>,
    }

    #[derive(FromRow, PartialEq, Debug)]
    struct Extended {
        c_uuid: uuid::Uuid,
        c_inet: std::net::IpAddr,
        c_json: serde_json::Value,
        c_jsonb: serde_json::Value,
        c_date: time::Date,
        c_time: time::Time,
        c_ts: time::PrimitiveDateTime,
        c_tstz: time::OffsetDateTime,
    }

    /// Binds each type as a parameter and reads it back out of the same
    /// column, so both the `ToSql` and `FromSql` halves are covered.
    #[tokio::test]
    async fn scalar_types_round_trip() {
        let mut db = TestDb::new("scalars").await;

        db.client
            .esql()
            .execute(
                "CREATE TABLE t (
                    c_bool      BOOLEAN,
                    c_int2      SMALLINT,
                    c_int4      INTEGER,
                    c_int8      BIGINT,
                    c_float4    REAL,
                    c_float8    DOUBLE PRECISION,
                    c_numeric   NUMERIC(10,2),
                    c_text      TEXT,
                    c_varchar   VARCHAR(32),
                    c_bytea     BYTEA
                )",
            )
            .await
            .unwrap();

        let expected = Scalars {
            c_bool: true,
            c_int2: 7,
            c_int4: 8,
            c_int8: 9,
            c_float4: 1.5,
            c_float8: 2.5,
            c_numeric: "10.25".parse().unwrap(),
            c_text: "text".into(),
            c_varchar: "varchar".into(),
            c_bytea: vec![1, 2, 3],
        };

        db.client
            .esql()
            .execute((
                "INSERT INTO t VALUES (?,?,?,?,?,?,?,?,?,?)",
                expected.c_bool,
                expected.c_int2,
                expected.c_int4,
                expected.c_int8,
                expected.c_float4,
                expected.c_float8,
                expected.c_numeric,
                expected.c_text.clone(),
                expected.c_varchar.clone(),
                expected.c_bytea.clone(),
            ))
            .await
            .unwrap();

        let actual: Scalars = db.client.esql().first("SELECT * FROM t").await.unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn extended_types_round_trip() {
        let mut db = TestDb::new("extended").await;

        db.client
            .esql()
            .execute(
                "CREATE TABLE t (
                    c_uuid      UUID,
                    c_inet      INET,
                    c_json      JSON,
                    c_jsonb     JSONB,
                    c_date      DATE,
                    c_time      TIME,
                    c_ts        TIMESTAMP,
                    c_tstz      TIMESTAMPTZ
                )",
            )
            .await
            .unwrap();

        let date = time::Date::from_calendar_date(2026, time::Month::August, 24).unwrap();
        let naive = time::PrimitiveDateTime::new(date, time::Time::from_hms(13, 30, 0).unwrap());

        let expected = Extended {
            c_uuid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            c_inet: "192.168.1.1".parse().unwrap(),
            c_json: serde_json::json!({ "a": 1 }),
            c_jsonb: serde_json::json!({ "a": 1 }),
            c_date: date,
            c_time: time::Time::from_hms(13, 30, 0).unwrap(),
            c_ts: naive,
            c_tstz: naive.assume_utc(),
        };

        db.client
            .esql()
            .execute((
                "INSERT INTO t VALUES (?,?,?,?,?,?,?,?)",
                expected.c_uuid,
                expected.c_inet,
                expected.c_json.clone(),
                expected.c_jsonb.clone(),
                expected.c_date,
                expected.c_time,
                expected.c_ts,
                expected.c_tstz,
            ))
            .await
            .unwrap();

        let actual: Extended = db.client.esql().first("SELECT * FROM t").await.unwrap();
        assert_eq!(actual, expected);
    }

    #[tokio::test]
    async fn arrays_round_trip() {
        let mut db = TestDb::new("arrays").await;

        db.client
            .esql()
            .execute("CREATE TABLE a (ints INTEGER[], texts TEXT[], nested INTEGER[][])")
            .await
            .unwrap();

        db.client
            .esql()
            .execute((
                "INSERT INTO a VALUES (?, ?, ?)",
                vec![1i32, 2, 3],
                vec!["x".to_string(), "y".to_string()],
                Vec::<i32>::new(),
            ))
            .await
            .unwrap();

        let ints: Vec<i32> = db.client.esql().first("SELECT ints FROM a").await.unwrap();
        assert_eq!(ints, vec![1, 2, 3]);

        let texts: Vec<String> = db.client.esql().first("SELECT texts FROM a").await.unwrap();
        assert_eq!(texts, vec!["x", "y"]);

        // `= ANY($1)` is the idiomatic alternative to building an IN list.
        let matched: Vec<i64> = db
            .client
            .esql()
            .query((
                "SELECT unnest(ints)::bigint FROM a WHERE 2 = ANY(ints) AND ints && ?",
                vec![2i32],
            ))
            .await
            .unwrap();
        assert_eq!(matched, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn null_reads_into_option() {
        let mut db = TestDb::new("nulls").await;

        db.client
            .esql()
            .execute("CREATE TABLE n (a INTEGER, b TEXT, c TIMESTAMPTZ)")
            .await
            .unwrap();

        db.client
            .esql()
            .execute((
                "INSERT INTO n VALUES (?, ?, ?)",
                None::<i32>,
                None::<String>,
                None::<time::OffsetDateTime>,
            ))
            .await
            .unwrap();

        let a: Option<i32> = db.client.esql().first("SELECT a FROM n").await.unwrap();
        let b: Option<String> = db.client.esql().first("SELECT b FROM n").await.unwrap();
        assert_eq!(a, None);
        assert_eq!(b, None);
    }
}
