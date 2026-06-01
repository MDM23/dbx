#[cfg(test)]
mod tests {
    use esql::{Esql, FromRow, FromRowError, Query, Row};
    use mysql_async::prelude::Queryable;

    #[allow(dead_code)]
    struct User {
        id: i64,
        name: String,
        active: bool,
    }

    impl FromRow for User {
        fn from_row<R: Row>(row: &R) -> Result<Self, FromRowError> {
            Ok(User {
                id: row.try_get("id")?,
                name: row.try_get("name")?,
                active: row.try_get("active")?,
            })
        }
    }

    fn base_url() -> String {
        let url = std::env::var("MYSQL_URL")
            .unwrap_or_else(|_| "mysql://root@localhost".to_string());
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Alice", true))
            .await
            .unwrap();

        db.pool
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 2i64, "Bob", false))
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Charlie", true))
            .await
            .unwrap();

        db.pool
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 2i64, "Diana", true))
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Eve", true))
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
        let mut tx = conn.start_transaction(mysql_async::TxOpts::new()).await.unwrap();

        tx.esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Frank", true))
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
        let mut tx = conn.start_transaction(mysql_async::TxOpts::new()).await.unwrap();

        tx.esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Ghost", true))
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
}
