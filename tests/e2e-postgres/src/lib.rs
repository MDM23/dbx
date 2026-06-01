#[cfg(test)]
mod tests {
    use esql::{Esql, FromRow, FromRowError, Query, Row};
    use tokio_postgres::NoTls;

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
        std::env::var("POSTGRES_URL")
            .unwrap_or_else(|_| "host=localhost user=postgres".to_string())
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
                        let _ = c.execute(&format!("DROP DATABASE IF EXISTS {name}"), &[]).await;
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Alice", true))
            .await
            .unwrap();

        db.client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 2i64, "Bob", false))
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Charlie", true))
            .await
            .unwrap();

        db.client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 2i64, "Diana", true))
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Eve", true))
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
    async fn transaction_commit() {
        let mut db = TestDb::with_users_table("tx_commit").await;

        let mut tx = db.client.transaction().await.unwrap();
        tx.esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Frank", true))
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
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Ghost", true))
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
}
