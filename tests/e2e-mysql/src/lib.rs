#[cfg(test)]
mod tests {
    use esql::{Esql, FromRow, Query};
    use mysql_async::prelude::Queryable;

    #[allow(dead_code)]
    #[derive(FromRow)]
    struct User {
        id: i64,
        name: String,
        active: bool,
    }

    fn base_url() -> String {
        let url =
            std::env::var("MYSQL_URL").unwrap_or_else(|_| "mysql://root@localhost".to_string());
        url.trim_end_matches('/').to_string()
    }

    fn strip_database(url: &str) -> String {
        let scheme_end = url.find("://").map(|i| i + 3).unwrap_or(0);
        let rest = &url[scheme_end..];
        match rest.rfind('/') {
            Some(i) => url[..scheme_end + i].to_string(),
            None => url.to_string(),
        }
    }

    struct TestDb {
        name: String,
        pool: mysql_async::Pool,
    }

    impl TestDb {
        async fn new(test_name: &str) -> Self {
            let name = format!("esql_test_{test_name}");
            let admin_url = strip_database(&base_url());
            let admin = mysql_async::Pool::new(admin_url.as_str());
            let mut conn = admin.get_conn().await.unwrap();
            conn.query_drop(format!("DROP DATABASE IF EXISTS {name}"))
                .await
                .unwrap();
            conn.query_drop(format!("CREATE DATABASE {name}"))
                .await
                .unwrap();
            drop(conn);
            admin.disconnect().await.unwrap();

            let url = format!("{admin_url}/{name}");
            let pool = mysql_async::Pool::new(url.as_str());

            Self { name, pool }
        }

        async fn with_users_table(test_name: &str) -> Self {
            let db = Self::new(test_name).await;
            let mut conn = db.pool.get_conn().await.unwrap();
            conn.query_drop(
                "CREATE TABLE users (
                    id      BIGINT PRIMARY KEY,
                    name    TEXT NOT NULL,
                    active  BOOLEAN NOT NULL DEFAULT TRUE
                )",
            )
            .await
            .unwrap();
            db
        }

        async fn cleanup(self) {
            let name = self.name.clone();
            self.pool.disconnect().await.unwrap();
            let admin_url = strip_database(&base_url());
            let admin = mysql_async::Pool::new(admin_url.as_str());
            let mut conn = admin.get_conn().await.unwrap();
            let _ = conn
                .query_drop(format!("DROP DATABASE IF EXISTS {name}"))
                .await;
            drop(conn);
            admin.disconnect().await.unwrap();
        }
    }

    #[tokio::test]
    async fn migrations() {
        let mut db = TestDb::new("migrations").await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut db.pool.esql()).await.unwrap();

        let tables: Vec<String> = db
            .pool
            .esql()
            .query(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = DATABASE() AND table_name IN ('migrations', 'users') \
                 ORDER BY table_name",
            )
            .await
            .unwrap();

        assert_eq!(tables, vec!["migrations", "users"]);
        db.cleanup().await;
    }

    #[tokio::test]
    async fn insert_and_query() {
        let mut db = TestDb::with_users_table("insert_and_query").await;

        db.pool
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Alice",
                true,
            ))
            .await
            .unwrap();

        db.pool
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
            .pool
            .esql()
            .query("SELECT id, name, active FROM users ORDER BY id")
            .await
            .unwrap();

        assert_eq!(users.len(), 2);
        assert_eq!(users[0].name, "Alice");
        assert!(users[0].active);
        assert_eq!(users[1].name, "Bob");
        assert!(!users[1].active);
        db.cleanup().await;
    }

    #[tokio::test]
    async fn query_builder_with_params() {
        let mut db = TestDb::with_users_table("query_builder").await;

        db.pool
            .esql()
            .execute((
                "INSERT INTO users (id, name, active) VALUES (?, ?, ?)",
                1i64,
                "Charlie",
                true,
            ))
            .await
            .unwrap();

        db.pool
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

        let names: Vec<String> = db.pool.esql().query(q).await.unwrap();
        assert_eq!(names, vec!["Charlie", "Diana"]);
        db.cleanup().await;
    }

    #[tokio::test]
    async fn first_returns_single_row() {
        let mut db = TestDb::with_users_table("first").await;

        db.pool
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
            .pool
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(name, "Eve");
        db.cleanup().await;
    }

    #[tokio::test]
    async fn transaction_commit() {
        let mut db = TestDb::with_users_table("tx_commit").await;

        let mut conn = db.pool.get_conn().await.unwrap();
        let mut tx = conn
            .start_transaction(mysql_async::TxOpts::new())
            .await
            .unwrap();

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
        drop(conn);

        let name: String = db
            .pool
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(name, "Frank");
        db.cleanup().await;
    }

    #[tokio::test]
    async fn transaction_rollback() {
        let mut db = TestDb::with_users_table("tx_rollback").await;

        let mut conn = db.pool.get_conn().await.unwrap();
        let mut tx = conn
            .start_transaction(mysql_async::TxOpts::new())
            .await
            .unwrap();

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
        drop(conn);

        let count: i64 = db
            .pool
            .esql()
            .first(("SELECT COUNT(*) FROM users WHERE id = ?", 1i64))
            .await
            .unwrap();

        assert_eq!(count, 0);
        db.cleanup().await;
    }

    #[derive(FromRow, PartialEq, Debug)]
    struct Scalars {
        c_bool: bool,
        c_smallint: i16,
        c_int: i32,
        c_bigint: i64,
        c_unsigned: u64,
        c_float: f32,
        c_double: f64,
        c_decimal: rust_decimal::Decimal,
        c_text: String,
        c_blob: Vec<u8>,
    }

    #[derive(FromRow, PartialEq, Debug)]
    struct Extended {
        c_uuid: uuid::Uuid,
        c_inet: std::net::IpAddr,
        c_json: serde_json::Value,
        c_date: time::Date,
        c_time: time::Time,
        c_datetime: time::PrimitiveDateTime,
    }

    /// MySQL reports every integer width as `Int(i64)`, so the narrow targets
    /// here only work if conversion is by value rather than by variant.
    #[tokio::test]
    async fn scalar_types_round_trip() {
        let db = TestDb::new("scalars").await;
        let mut pool = db.pool.clone();

        pool.esql()
            .execute(
                "CREATE TABLE t (
                    c_bool      BOOLEAN,
                    c_smallint  SMALLINT,
                    c_int       INT,
                    c_bigint    BIGINT,
                    c_unsigned  BIGINT UNSIGNED,
                    c_float     FLOAT,
                    c_double    DOUBLE,
                    c_decimal   DECIMAL(10,2),
                    c_text      TEXT,
                    c_blob      BLOB
                )",
            )
            .await
            .unwrap();

        let expected = Scalars {
            c_bool: true,
            c_smallint: 7,
            c_int: 8,
            c_bigint: 9,
            c_unsigned: u64::MAX,
            c_float: 1.5,
            c_double: 2.5,
            c_decimal: "10.25".parse().unwrap(),
            c_text: "text".into(),
            c_blob: vec![1, 2, 3],
        };

        pool.esql()
            .execute((
                "INSERT INTO t VALUES (?,?,?,?,?,?,?,?,?,?)",
                expected.c_bool,
                expected.c_smallint,
                expected.c_int,
                expected.c_bigint,
                expected.c_unsigned,
                expected.c_float,
                expected.c_double,
                expected.c_decimal,
                expected.c_text.clone(),
                expected.c_blob.clone(),
            ))
            .await
            .unwrap();

        let actual: Scalars = pool.esql().first("SELECT * FROM t").await.unwrap();
        assert_eq!(actual, expected);
        db.cleanup().await;
    }

    /// MySQL has no native uuid or inet type, and hands DECIMAL and JSON back
    /// as text, so these all round-trip through their conventional columns.
    #[tokio::test]
    async fn extended_types_round_trip() {
        let db = TestDb::new("extended").await;
        let mut pool = db.pool.clone();

        pool.esql()
            .execute(
                "CREATE TABLE t (
                    c_uuid      CHAR(36),
                    c_inet      VARCHAR(45),
                    c_json      JSON,
                    c_date      DATE,
                    c_time      TIME,
                    c_datetime  DATETIME
                )",
            )
            .await
            .unwrap();

        let date = time::Date::from_calendar_date(2026, time::Month::August, 24).unwrap();
        let expected = Extended {
            c_uuid: uuid::Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap(),
            c_inet: "192.168.1.1".parse().unwrap(),
            c_json: serde_json::json!({ "a": 1 }),
            c_date: date,
            c_time: time::Time::from_hms(13, 30, 0).unwrap(),
            c_datetime: time::PrimitiveDateTime::new(date, time::Time::from_hms(13, 30, 0).unwrap()),
        };

        pool.esql()
            .execute((
                "INSERT INTO t VALUES (?,?,?,?,?,?)",
                expected.c_uuid,
                expected.c_inet,
                expected.c_json.clone(),
                expected.c_date,
                expected.c_time,
                expected.c_datetime,
            ))
            .await
            .unwrap();

        let actual: Extended = pool.esql().first("SELECT * FROM t").await.unwrap();
        assert_eq!(actual, expected);
        db.cleanup().await;
    }

    /// MySQL has no array type, so binding one has to fail with a clear error
    /// rather than encoding something arbitrary.
    #[tokio::test]
    async fn arrays_are_rejected() {
        let db = TestDb::new("arrays").await;
        let mut pool = db.pool.clone();

        let result = pool
            .esql()
            .execute(("SELECT ?", vec![1i32, 2, 3]))
            .await;

        assert!(matches!(
            result,
            Err(esql::Error::UnsupportedParam("array"))
        ));
        db.cleanup().await;
    }
}
