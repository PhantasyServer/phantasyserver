use console::style;
use data_structs::{ItemParameters, ShipInfo};
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::RwLock;
use pso2packetlib::PrivateKey;
use pso2ship_server::{init_block, master_conn::MasterConnection, sql, BlockInfo};
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    traits::PublicKeyParts,
    RsaPrivateKey,
};
use std::{error, io, net::Ipv4Addr, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn error::Error>> {
    let mul_progress = MultiProgress::new();
    let startup_progress = mul_progress.add(ProgressBar::new_spinner());
    startup_progress.set_message("Starting server...");
    let key = match std::fs::metadata("keypair.pem") {
        Ok(..) => RsaPrivateKey::read_pkcs8_pem_file("keypair.pem")?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let key_progress = mul_progress.add(ProgressBar::new_spinner());
            key_progress.set_message(style("No keyfile found, creating...").yellow().to_string());
            let mut rand_gen = rand::thread_rng();
            let key = RsaPrivateKey::new(&mut rand_gen, 1024)?;
            key.write_pkcs8_pem_file("keypair.pem", rsa::pkcs8::LineEnding::default())?;
            key_progress.finish_with_message("Keyfile created.");
            key
        }
        Err(e) => {
            return Err(e.into());
        }
    };
    let settings = pso2ship_server::Settings::load("ship.toml").await?;
    let (data_pc, data_vita) = pso2ship_server::create_attr_files(&mul_progress)?;
    let mut item_data = ItemParameters::load_from_mp_file("names.mp")?;
    item_data.pc_attrs = data_pc;
    item_data.vita_attrs = data_vita;
    let item_data = Arc::new(RwLock::new(item_data));
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));
    let master_conn = MasterConnection::new(
        tokio::net::lookup_host(settings.master_ship)
            .await?
            .next()
            .expect("No ips found for master ship"),
    )
    .await?;
    for id in 2..10 {
        let resp = MasterConnection::register_ship(
            &master_conn,
            ShipInfo {
                ip: Ipv4Addr::UNSPECIFIED,
                id,
                port: settings.balance_port,
                max_players: 32,
                name: "Test".into(),
                data_type: data_structs::DataTypeDef::Parsed,
                status: pso2packetlib::protocol::login::ShipStatus::Online,
                key: data_structs::KeyInfo {
                    n: key.n().to_bytes_le(),
                    e: key.e().to_bytes_le(),
                },
            },
        )
        .await?;
        match resp {
            data_structs::RegisterShipResult::Success => break,
            data_structs::RegisterShipResult::AlreadyTaken => {
                if id != 9 {
                    continue;
                }
                eprintln!("No stots left");
                return Ok(());
            }
        }
    }

    let sql = Arc::new(sql::Sql::new(&settings.db_name, master_conn).await?);
    make_block_balance(server_statuses.clone(), settings.balance_port).await?;
    let mut blocks = vec![];
    let mut ports = 13001;
    let mut blockstatus_lock = server_statuses.write();
    for (i, block) in settings.blocks.into_iter().enumerate() {
        let port = block.port.unwrap_or(ports);
        ports += 1;
        let new_block = BlockInfo {
            id: i as u32 + 1,
            name: block.name.clone(),
            ip: Ipv4Addr::UNSPECIFIED,
            port,
            max_players: block.max_players,
            players: 0,
            maps: block.maps,
        };
        blockstatus_lock.push(new_block.clone());
        let server_statuses = server_statuses.clone();
        let sql = sql.clone();
        let item_data = item_data.clone();
        let key = PrivateKey::Key(key.clone());
        blocks.push(tokio::spawn(async move {
            match init_block(server_statuses, new_block, sql, item_data, key).await {
                Ok(_) => {}
                Err(e) => eprintln!("Block \"{}\" failed: {e}", block.name),
            }
        }))
    }
    drop(blockstatus_lock);

    startup_progress.finish_with_message("Server started.");
    tokio::signal::ctrl_c().await?;

    Ok(())
}

async fn make_block_balance(
    server_statuses: Arc<RwLock<Vec<BlockInfo>>>,
    port: u16,
) -> io::Result<()> {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind(("0.0.0.0", port)).await?;
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
