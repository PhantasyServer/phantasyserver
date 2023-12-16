use master_ship::{ctrl_c_handler, make_block_balance, make_query, sql::Sql, Settings};
use parking_lot::RwLock;
use std::{error, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    tokio::spawn(ctrl_c_handler());
    let settings = Settings::load("master_ship.toml").await?;
    let sql = Arc::new(Sql::new(&settings.db_name).await?);
    let servers = Arc::new(RwLock::new(vec![]));
    make_query(servers.clone()).await?;
    make_block_balance(servers.clone()).await?;
    master_ship::ship_receiver(servers, sql).await?;
    Ok(())
}
