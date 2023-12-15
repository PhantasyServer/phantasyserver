use master_ship::make_query;
use parking_lot::RwLock;
use std::{error, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let servers = Arc::new(RwLock::new(vec![]));
    make_query(servers.clone()).await?;
    tokio::spawn(async { master_ship::test_ship().await.unwrap() });
    master_ship::ship_receiver(servers).await?;
    Ok(())
}
