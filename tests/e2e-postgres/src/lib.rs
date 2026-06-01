#[cfg(test)]
mod tests {
    use esql::{Esql, FromRow, FromRowError, Query, Row};
    use tokio_postgres::NoTls;

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

    async fn connect() -> tokio_postgres::Client {
        let url = std::env::var("POSTGRES_URL")
            .unwrap_or_else(|_| "host=localhost user=postgres".to_string());

        let (client, connection) = tokio_postgres::connect(&url, NoTls).await.unwrap();
        tokio::spawn(connection);
        client
    }

    #[tokio::test]
    async fn migrations() {
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        let tables: Vec<String> = client
            .esql()
            .query(
                "SELECT table_name FROM information_schema.tables \
                 WHERE table_schema = 'public' AND table_name IN ('migrations', 'users') \
                 ORDER BY table_name",
            )
            .await
            .unwrap();

        assert_eq!(tables, vec!["migrations", "users"]);
    }

    #[tokio::test]
    async fn insert_and_query() {
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 1i64, "Alice", true))
            .await
            .unwrap();

        client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 2i64, "Bob", false))
            .await
            .unwrap();

        let users: Vec<User> = client
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
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 10i64, "Charlie", true))
            .await
            .unwrap();

        client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 11i64, "Diana", true))
            .await
            .unwrap();

        let mut q = Query::from("SELECT name FROM users");
        q.push_where(Query::in_("id", [10i64, 11]));
        q.push("ORDER BY name");

        let names: Vec<String> = client.esql().query(q).await.unwrap();
        assert_eq!(names, vec!["Charlie", "Diana"]);
    }

    #[tokio::test]
    async fn first_returns_single_row() {
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        client
            .esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 20i64, "Eve", true))
            .await
            .unwrap();

        let name: String = client
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 20i64))
            .await
            .unwrap();

        assert_eq!(name, "Eve");
    }

    #[tokio::test]
    async fn transaction_commit() {
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        let mut tx = client.transaction().await.unwrap();
        tx.esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 30i64, "Frank", true))
            .await
            .unwrap();
        tx.commit().await.unwrap();

        let name: String = client
            .esql()
            .first(("SELECT name FROM users WHERE id = ?", 30i64))
            .await
            .unwrap();

        assert_eq!(name, "Frank");
    }

    #[tokio::test]
    async fn transaction_rollback() {
        let mut client = connect().await;
        let migrator = esql::migrate::embed_migrations!("../migrations");
        migrator.run(&mut client.esql()).await.unwrap();

        let mut tx = client.transaction().await.unwrap();
        tx.esql()
            .execute(("INSERT INTO users (id, name, active) VALUES (?, ?, ?)", 31i64, "Ghost", true))
            .await
            .unwrap();
        tx.rollback().await.unwrap();

        let count: i64 = client
            .esql()
            .first(("SELECT COUNT(*) FROM users WHERE id = ?", 31i64))
            .await
            .unwrap();

        assert_eq!(count, 0);
    }
}
