use esql::Esql;
use tokio_postgres::NoTls;

#[tokio::main]
async fn main() {
    bb8_example().await.unwrap();
    deadpool_example().await.unwrap();
}

async fn bb8_example() -> Result<(), Box<dyn std::error::Error>> {
    let manager = bb8_postgres::PostgresConnectionManager::new_from_stringlike(
        "host=localhost user=postgres",
        NoTls,
    )?;

    let pool = bb8::Pool::builder().build(manager).await?;
    let mut conn = pool.get().await?;

    // Direct query on pooled connection
    let result: i64 = conn.esql().first(("SELECT 1 + ?", 2)).await?;
    println!("bb8 result: {result}");

    // Transaction on pooled connection
    let mut tx = conn.transaction().await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.esql().execute(("SELECT 1 + ?", 3)).await?;
    tx.commit().await?;

    Ok(())
}

async fn deadpool_example() -> Result<(), Box<dyn std::error::Error>> {
    let mut cfg = deadpool_postgres::Config::new();
    cfg.host = Some("localhost".to_string());
    cfg.user = Some("postgres".to_string());

    let pool = cfg.create_pool(None, NoTls)?;
    let mut conn = pool.get().await?;

    // Direct query on pooled connection
    let result: i64 = conn.esql().first(("SELECT 1 + ?", 2)).await?;
    println!("deadpool result: {result}");

    // Transaction on pooled connection
    let mut tx = conn.transaction().await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.esql().execute(("SELECT 1 + ?", 3)).await?;
    tx.commit().await?;

    Ok(())
}
