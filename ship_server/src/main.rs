use console::style;
use data_structs::{ItemParameters, ShipInfo};
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::RwLock;
use pso2ship_server::{init_block, master_conn::MasterConnection, sql, BlockInfo};
use rsa::{pkcs8::EncodePrivateKey, RsaPrivateKey};
use std::{error, io, net::Ipv4Addr, sync::Arc};

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
    let (data_pc, data_vita) = pso2ship_server::create_attr_files(&mul_progress)?;
    let mut item_data = ItemParameters::load_from_mp_file("names.mp")?;
    item_data.pc_attrs = data_pc;
    item_data.vita_attrs = data_vita;
    let item_data = Arc::new(RwLock::new(item_data));
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));
    let master_conn = MasterConnection::new("192.168.0.104:15000".parse()?).await?;
    let resp = MasterConnection::register_ship(
        &master_conn,
        ShipInfo {
            ip: Ipv4Addr::UNSPECIFIED,
            id: 1,
            port: 12000,
            max_players: 32,
            name: "Test".into(),
            data_type: data_structs::DataTypeDef::Parsed,
            status: pso2packetlib::protocol::login::ShipStatus::Online,
        },
    )
    .await?;
    match resp {
        data_structs::RegisterShipResult::Success => {}
        data_structs::RegisterShipResult::AlreadyTaken => {
            eprintln!("Ship id is already taken");
            return Ok(());
        }
    }
    {
        let sql = Arc::new(sql::Sql::new("sqlite://server.db", master_conn).await?);
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
        tokio::signal::ctrl_c().await?;

        Ok(())
    }

    // Ok(())
}

async fn make_block_balance(server_statuses: Arc<RwLock<Vec<BlockInfo>>>) -> io::Result<()> {
    use tokio::net::TcpListener;
    // TODO: add ship id config
    let listener = TcpListener::bind(("0.0.0.0", 12000)).await?;
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((s, _)) => {
                    let _ = pso2ship_server::send_block_balance(
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
    Ok(())
}
