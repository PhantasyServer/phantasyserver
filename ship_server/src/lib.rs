#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::future_not_send)]
#![warn(clippy::missing_const_for_fn)]
#![allow(clippy::await_holding_lock)]
#![allow(dead_code)]

mod battle_stats;
mod block;
mod inventory;
mod invites;
mod map;
mod master_conn;
mod mutex;
mod palette;
mod party;
mod quests;
mod settings;
mod sql;
mod user;

use data_structs::{
    master_ship::{self, ShipInfo},
    SerDeFile, ServerData,
};
use master_conn::MasterConnection;
use mutex::{Mutex, RwLock};
use pso2packetlib::{
    protocol::{login, Packet, PacketType},
    Connection, PrivateKey, PublicKey,
};
use quests::Quests;
use rand::Rng;
use rsa::traits::PublicKeyParts;
use settings::Settings;
use std::{
    io,
    net::Ipv4Addr,
    sync::{atomic::AtomicU32, Arc},
};
use thiserror::Error;
use user::*;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid input in fn {0}")]
    InvalidInput(&'static str),
    #[error("Invalid password")]
    InvalidPassword,
    #[error("No user found")]
    NoUser,
    #[error("No user {0} found in mapset {1}")]
    NoUserInMap(u32, String),
    #[error("Mapid {0} not found in mapset {1}")]
    NoMapInMapSet(u32, String),
    #[error("Master ship returned error: {0}")]
    MSError(String),
    #[error("Master ship sent unexpected data")]
    MSUnexpected,
    #[error("Invalid master ship PSK")]
    MSInvalidPSK,
    #[error("Master server didn't respond")]
    MSNoResponse,
    #[error("User sent unexpected packet while being in state: {0}")]
    UserInvalidState(UserState),
    #[error("Map with name {0} doesn't exist")]
    NoMapFound(String),
    #[error("Item ({0}, {1}) not found in item attributes")]
    NoItemInAttrs(u16, u16),
    #[error("No clothes with model {0} found in item attributes")]
    NoClothes(u16),
    #[error("No enemy data for {0} found")]
    NoEnemyData(String),
    #[error("No damage ID {0} found")]
    NoDamageInfo(u32),
    #[error("Unknown enemy hitbox {0}:{1}")]
    NoHitboxInfo(String, u32),
    #[error("No ship data available")]
    NoShipData,

    // passthrough errors
    #[error("SQL error: {0}")]
    SqlError(#[from] sqlx::Error),
    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    DataError(#[from] data_structs::Error),
    #[error("Lua error: {0}")]
    LuaError(#[from] mlua::Error),
    #[error("MP Serialization error: {0}")]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[error("MP Deserialization error: {0}")]
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[error("TOML Serialization error: {0}")]
    TomlSerError(#[from] toml::ser::Error),
    #[error("TOML Deserialization error: {0}")]
    TomlDeError(#[from] toml::de::Error),
    #[error("RSA error: {0}")]
    RSAError(#[from] rsa::Error),
    #[error("PKCS8 error: {0}")]
    PKCS8Error(#[from] rsa::pkcs8::Error),
    #[error("Client connection error: {0}")]
    ConnError(#[from] pso2packetlib::connection::ConnectionError),
    #[error("Packet error: {0}")]
    PacketError(#[from] pso2packetlib::protocol::PacketError),
    #[error("Task join error: {0}")]
    JoinError(#[from] tokio::task::JoinError),
}

#[derive(Clone)]
struct BlockInfo {
    id: u32,
    name: String,
    ip: Ipv4Addr,
    port: u16,
    max_players: u32,
    players: u32,
    lobby_map: String,
    server_data: Arc<ServerData>,
    quests: Arc<Quests>,
}

struct BlockData {
    sql: Arc<sql::Sql>,
    block_id: u32,
    block_name: String,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    lobby: Arc<Mutex<map::Map>>,
    key: PrivateKey,
    latest_mapid: AtomicU32,
    latest_partyid: AtomicU32,
    server_data: Arc<ServerData>,
    quests: Arc<Quests>,
    clients: Mutex<Vec<(usize, Arc<Mutex<User>>)>>,
}

#[derive(Default, Clone)]
enum Action {
    #[default]
    Nothing,
    Disconnect,
}

// feel free to suggest log level changes
pub async fn run() -> Result<(), Error> {
    let settings = Settings::load("ship.toml").await?;
    // setup logging
    {
        let _ = std::fs::create_dir_all(&settings.log_dir);
        let mut path = std::path::PathBuf::from(&settings.log_dir);
        path.push(format!(
            "ship_{}.log",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        ));
        let log_file = std::fs::File::create(path)?;

        use simplelog::*;
        CombinedLogger::init(vec![
            TermLogger::new(
                settings.console_log_level,
                Config::default(),
                TerminalMode::Mixed,
                ColorChoice::Auto,
            ),
            WriteLogger::new(settings.file_log_level, Config::default(), log_file),
        ])
        .unwrap();
    }

    log::info!("Starting server...");
    let key = settings.load_key()?;
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));

    let master_ip = if let Some(ip) = settings.master_ship.as_ref() {
        tokio::net::lookup_host(ip)
            .await?
            .next()
            .expect("No IPs found for master ship")
    } else {
        log::warn!("No master ship IP provided, discovering...");
        data_structs::master_ship::try_discover().await?
    };
    log::info!("Connecting to master ship...");
    let master_conn = MasterConnection::new(master_ip, settings.master_ship_psk.as_bytes()).await?;
    log::info!("Connected to master ship");
    let total_max_players = settings.blocks.iter().map(|b| b.max_players).sum();
    log::info!("Registering ship");
    for id in settings.min_ship_id..=settings.max_ship_id {
        log::debug!("Requested ship id: {id}");
        let resp = MasterConnection::register_ship(
            &master_conn,
            ShipInfo {
                ip: Ipv4Addr::UNSPECIFIED,
                id,
                port: settings.balance_port,
                max_players: total_max_players,
                name: settings.server_name.clone(),
                status: pso2packetlib::protocol::login::ShipStatus::Online,
                key: master_ship::KeyInfo {
                    n: key.n().to_bytes_le(),
                    e: key.e().to_bytes_le(),
                },
            },
        )
        .await?;
        match resp {
            master_ship::RegisterShipResult::Success => break,
            master_ship::RegisterShipResult::AlreadyTaken => {
                if id < settings.max_ship_id {
                    continue;
                }
                log::error!("No stots left");
                return Ok(());
            }
        }
    }
    log::info!("Registed ship");

    let mut server_data = Arc::new(if let Some(data_path) = settings.data_file {
        log::info!("Loading server data...");
        ServerData::load_from_mp_comp(data_path)?
    } else {
        log::warn!("No server data file provided, receiving from master ship...");
        match master_conn
            .run_action(master_ship::MasterShipAction::ServerDataRequest)
            .await?
        {
            master_ship::MasterShipAction::ServerDataResponse(server_data_result) => {
                match server_data_result {
                    master_ship::ServerDataResult::Ok(server_data) => *server_data,
                    master_ship::ServerDataResult::NotAvailable => {
                        log::error!("No data available from master ship!");
                        return Err(Error::NoShipData);
                    }
                }
            }
            master_ship::MasterShipAction::Error(e) => return Err(Error::MSError(e)),
            _ => return Err(Error::MSUnexpected),
        }
    });
    log::info!("Loaded server data");
    let quests = Arc::new(Quests::load(std::mem::take(
        &mut Arc::get_mut(&mut server_data).unwrap().quests,
    )));

    let sql = Arc::new(sql::Sql::new(&settings.db_name, master_conn).await?);
    make_block_balance(server_statuses.clone(), settings.balance_port).await?;
    let mut blocks = vec![];
    let mut ports = 13001;
    let mut blockstatus_lock = server_statuses.write().await;
    log::info!("Starting blocks...");
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
            lobby_map: block.lobby_map,
            server_data: server_data.clone(),
            quests: quests.clone(),
        };
        blockstatus_lock.push(new_block.clone());
        let server_statuses = server_statuses.clone();
        let sql = sql.clone();
        let key = PrivateKey::Key(key.clone());
        log::debug!("Started block {}", block.name);
        blocks.push(tokio::spawn(async move {
            match block::init_block(server_statuses, new_block, sql, key).await {
                Ok(_) => {}
                Err(e) => log::error!("Block \"{}\" failed: {e}", block.name),
            }
        }))
    }
    drop(blockstatus_lock);

    log::info!("Server started.");
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
                    let _ = send_block_balance(s, server_statuses.clone()).await;
                }
                Err(e) => {
                    log::warn!("Failed to accept block balance connection: {}", e);
                    return;
                }
            }
        }
    });
    Ok(())
}

async fn send_block_balance(
    stream: tokio::net::TcpStream,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
) -> Result<(), Error> {
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    log::debug!("Block balancing {local_addr}...");
    let mut con = Connection::<Packet>::new_async(
        stream,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let mut blocks = blocks.write().await;
    let server_count = blocks.len() as u32;
    for block in blocks.iter_mut() {
        if block.ip == Ipv4Addr::UNSPECIFIED {
            if let std::net::IpAddr::V4(addr) = local_addr {
                block.ip = addr
            }
        }
    }
    let block = &mut blocks[rand::thread_rng().gen_range(0..server_count) as usize];
    let packet = login::BlockBalancePacket {
        ip: block.ip,
        port: block.port,
        blockname: block.name.clone(),
        ..Default::default()
    };
    con.write_packet_async(&Packet::BlockBalance(packet))
        .await?;
    Ok(())
}
