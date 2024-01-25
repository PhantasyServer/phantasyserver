#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::future_not_send)]
#![warn(clippy::missing_const_for_fn)]
#![allow(clippy::await_holding_lock)]
#![allow(dead_code)]

mod block;
mod ice;
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
    inventory::ItemParameters,
    master_ship::{self, ShipInfo},
    SerDeFile,
};
use ice::{IceFileInfo, IceWriter};
use master_conn::MasterConnection;
use mutex::{Mutex, RwLock};
use pso2packetlib::{
    protocol::{login, models::item_attrs, Packet, PacketType},
    Connection, PrivateKey, PublicKey,
};
use quests::Quests;
use rand::Rng;
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    traits::PublicKeyParts,
    RsaPrivateKey,
};
use settings::Settings;
use std::{
    io::{self, Cursor},
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
    #[error("Invalid character")]
    InvalidCharacter,
    #[error("No character loaded")]
    NoCharacter,
    #[error("Master ship returned error: {0}")]
    MSError(String),
    #[error("Master ship sent unexpected data")]
    MSUnexpected,
    #[error("Invalid master ship PSK")]
    MSInvalidPSK,

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
    quests: Arc<Quests>,
}

struct BlockData {
    sql: Arc<sql::Sql>,
    block_id: u32,
    block_name: String,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    //TODO: remove rwlock after testing is done (waaay in the future)
    item_attrs: Arc<RwLock<ItemParameters>>,
    lobby: Arc<Mutex<map::Map>>,
    key: PrivateKey,
    latest_mapid: AtomicU32,
    latest_partyid: AtomicU32,
    quests: Arc<Quests>,
}

#[derive(Default, Clone)]
enum Action {
    #[default]
    Nothing,
    Disconnect,
    InitialLoad,

    // party related
    SendPartyInvite(u32),
}

// feel free to suggest log level changes
pub async fn run() -> Result<(), Error> {
    let settings = Settings::load("ship.toml").await?;
    // setup logging
    {
        use simplelog::*;
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
    log::info!("Loading keypair");
    let key = match std::fs::metadata("keypair.pem") {
        Ok(..) => RsaPrivateKey::read_pkcs8_pem_file("keypair.pem")?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            log::warn!("No keyfile found, creating...");
            let mut rand_gen = rand::thread_rng();
            let key = RsaPrivateKey::new(&mut rand_gen, 1024)?;
            key.write_pkcs8_pem_file("keypair.pem", rsa::pkcs8::LineEnding::default())?;
            log::info!("Keyfile created.");
            key
        }
        Err(e) => {
            log::error!("Failed to load keypair: {e}");
            return Err(e.into());
        }
    };
    log::info!("Loaded keypair");
    let (data_pc, data_vita) = create_attr_files()?;
    let quests = Arc::new(Quests::load(&settings.quest_dir));
    let mut item_data = ItemParameters::load_from_mp_file("data/names.mp")?;
    item_data.pc_attrs = data_pc;
    item_data.vita_attrs = data_vita;
    let item_data = Arc::new(RwLock::new(item_data));
    let server_statuses = Arc::new(RwLock::new(Vec::<BlockInfo>::new()));
    log::info!("Connecting to master ship...");
    let master_conn = MasterConnection::new(
        tokio::net::lookup_host(settings.master_ship)
            .await?
            .next()
            .expect("No ips found for master ship"),
        settings.master_ship_psk.as_bytes(),
    )
    .await?;
    log::info!("Connected to master ship");
    let total_max_players = settings.blocks.iter().map(|b| b.max_players).sum();
    log::info!("Registering ship");
    for id in 2..10 {
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
                if id != 9 {
                    continue;
                }
                log::error!("No stots left");
                return Ok(());
            }
        }
    }
    log::info!("Registed ship");

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
            quests: quests.clone(),
        };
        blockstatus_lock.push(new_block.clone());
        let server_statuses = server_statuses.clone();
        let sql = sql.clone();
        let item_data = item_data.clone();
        let key = PrivateKey::Key(key.clone());
        log::debug!("Started block {}", block.name);
        blocks.push(tokio::spawn(async move {
            match block::init_block(server_statuses, new_block, sql, item_data, key).await {
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
                    let _ =
                        send_block_balance(s.into_std().unwrap(), server_statuses.clone()).await;
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
    stream: std::net::TcpStream,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    log::debug!("Block balancing {local_addr}...");
    let mut con = Connection::new(
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
    con.write_packet(&Packet::BlockBalance(packet))?;
    Ok(())
}

fn create_attr_files() -> Result<(Vec<u8>, Vec<u8>), Error> {
    log::info!("Creating item attributes");
    log::debug!("Loading item attributes...");
    let attrs_str = std::fs::read_to_string("data/item_attrs.json")?;
    log::debug!("Parsing item attributes...");
    let attrs: item_attrs::ItemAttributes = serde_json::from_str(&attrs_str)?;

    // PC attributes
    log::debug!("Creating PC item attributes...");
    let outdata_pc = Cursor::new(vec![]);
    let attrs: item_attrs::ItemAttributesPC = attrs.into();
    let mut attrs_data_pc = Cursor::new(vec![]);
    attrs.write_attrs(&mut attrs_data_pc)?;
    attrs_data_pc.set_position(0);
    let mut ice_writer = IceWriter::new(outdata_pc)?;
    ice_writer.load_group(ice::Group::Group2);
    ice_writer.new_file(IceFileInfo {
        filename: "item_parameter.bin".into(),
        file_extension: "bin".into(),
        ..Default::default()
    })?;
    std::io::copy(&mut attrs_data_pc, &mut ice_writer)?;
    let outdata_pc = ice_writer.into_inner()?.into_inner();

    // Vita attributes
    log::debug!("Creating Vita item attributes...");
    let outdata_vita = Cursor::new(vec![]);
    let attrs: item_attrs::ItemAttributesVita = attrs.into();
    let mut attrs_data_vita = Cursor::new(vec![]);
    attrs.write_attrs(&mut attrs_data_vita)?;
    attrs_data_vita.set_position(0);
    let mut ice_writer = IceWriter::new(outdata_vita)?;
    ice_writer.load_group(ice::Group::Group2);
    ice_writer.new_file(IceFileInfo {
        filename: "item_parameter.bin".into(),
        file_extension: "bin".into(),
        ..Default::default()
    })?;
    std::io::copy(&mut attrs_data_vita, &mut ice_writer)?;
    let outdata_vita = ice_writer.into_inner()?.into_inner();

    log::info!("Created item attributes");
    Ok((outdata_pc, outdata_vita))
}
