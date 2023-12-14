use console::style;
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::RwLock;
use pso2packetlib::protocol::login::ShipEntry;
use pso2server::{init_block, inventory::ItemParameters, sql, BlockInfo};
use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
use std::{error, io, net::Ipv4Addr, sync::Arc, time::Duration};

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let mul_progress = MultiProgress::new();
    let startup_progress = mul_progress.add(ProgressBar::new_spinner());
    startup_progress.set_message("Starting server...");
    match std::fs::metadata("keypair.pem") {
        Ok(..) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key_progress = mul_progress.add(ProgressBar::new_spinner());
            key_progress.set_message(style("No keyfile found, creating...").yellow().to_string());
            let mut rand_gen = rand::thread_rng();
            let key = RsaPrivateKey::new(&mut rand_gen, 1024)?;
            key.write_pkcs8_pem_file("keypair.pem", rsa::pkcs8::LineEnding::default())?;
            key_progress.finish_with_message("Keyfile created.")
        }
        Err(e) => {
            return Err(e.into());
        }
    }
    let (data_pc, data_vita) = pso2server::create_attr_files(&mul_progress)?;
    let mut item_data = ItemParameters::load_from_mp_file("names.mp")?;
    item_data.pc_attrs = data_pc;
    item_data.vita_attrs = data_vita;
    let item_data = Arc::new(RwLock::new(item_data));
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));
    let ship_statuses = Arc::new(RwLock::new(Vec::<ShipEntry>::new()));
    {
        let mut ships = ship_statuses.write();
        ships.push(ShipEntry {
            id: 1000,
            name: "Ship01".to_string(),
            ip: Ipv4Addr::UNSPECIFIED,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
            order: 1,
        });
        ships.push(ShipEntry {
            id: 2000,
            name: "Ship02".to_string(),
            ip: Ipv4Addr::UNSPECIFIED,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
            order: 2,
        });
    }
    {
        let sql = Arc::new(RwLock::new(sql::Sql::new().unwrap()));
        make_query(ship_statuses.clone()).await?;
        make_block_balance(server_statuses.clone()).await?;
        let mut blocks = vec![];
        for i in 100..101 {
            let mut blockstatus_lock = server_statuses.write();
            let new_block = BlockInfo {
                id: i,
                name: format!("Block {}", i),
                ip: [0, 0, 0, 0],
                port: 13000 + i as u16,
            };
            blockstatus_lock.push(new_block.clone());
            let server_statuses = server_statuses.clone();
            let sql = sql.clone();
            let item_data = item_data.clone();
            blocks.push(tokio::spawn(async move {
                init_block(server_statuses, new_block, sql, item_data).await
            }))
        }

        startup_progress.finish_with_message("Server started.");
        loop {
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    // Ok(())
}

async fn make_block_balance(server_statuses: Arc<RwLock<Vec<BlockInfo>>>) -> io::Result<()> {
    use tokio::net::TcpListener;
    // TODO: add ship id config
    let mut listeners = vec![];
    for i in 0..10 {
        //pc balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12100 + (i * 100))).await?);
        //vita balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12193 + (i * 100))).await?);
    }
    for listener in listeners {
        let server_statuses = server_statuses.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        let _ = pso2server::send_block_balance(
                            s.into_std().unwrap(),
                            server_statuses.clone(),
                        );
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}

async fn make_query(server_statuses: Arc<RwLock<Vec<ShipEntry>>>) -> io::Result<()> {
    use tokio::net::TcpListener;
    let mut info_listeners: Vec<TcpListener> = vec![];
    for i in 0..10 {
        //pc ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12199 + (i * 100))).await?);
        //vita ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12194 + (i * 100))).await?);
    }
    for listener in info_listeners {
        let server_statuses = server_statuses.clone();
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        let _ =
                            pso2server::send_querry(s.into_std().unwrap(), server_statuses.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}
