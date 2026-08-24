//! Both drivers in one binary. The placeholder style comes from the
//! connection's dialect, so the same `?` source syntax reaches MySQL as `?`
//! and Postgres as `$1`.

use esql::Esql;

#[tokio::main]
async fn main() {
    mysql_example().await.unwrap();
    postgres_example().await.unwrap();
}

async fn mysql_example() -> Result<(), esql::Error<mysql_async::Error>> {
    use mysql_async::TxOpts;

    let mut pool = mysql_async::Pool::new("mysql://127.0.0.1");

    // Call .esql() to get an esql handle, then use execute/query/first
    let result: u64 = pool.esql().execute(("SELECT 1 + ?", 2)).await?;
    println!("affected: {result}");

    // Transactions work the same way
    let mut tx = pool.start_transaction(TxOpts::new()).await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.commit().await.map_err(esql::Error::Driver)?;

    Ok(())
}

async fn postgres_example() -> Result<(), esql::Error<tokio_postgres::Error>> {
    use tokio_postgres::NoTls;

    let (mut client, connection) =
        tokio_postgres::connect("host=localhost user=postgres", NoTls).await?;

    tokio::spawn(async move {
        if let Err(e) = connection.await {
            eprintln!("connection error: {e}");
        }
    });

    let result: u64 = client.esql().execute(("SELECT 1 + ?", 2)).await?;
    println!("affected: {result}");

    let mut tx = client.transaction().await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.esql().execute(("SELECT 1 + ?", 2)).await?;
    tx.commit().await.map_err(esql::Error::Driver)?;

    Ok(())
}
