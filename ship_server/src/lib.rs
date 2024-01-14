#![deny(clippy::undocumented_unsafe_blocks)]
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
mod sql;
mod user;

use console::style;
use data_structs::{ItemParameters, SerDeFile as _, ShipInfo};
use ice::{IceFileInfo, IceWriter};
use indicatif::{MultiProgress, ProgressBar};
use master_conn::MasterConnection;
use mutex::{Mutex, RwLock};
use pso2packetlib::{
    protocol::{login, models::item_attrs, Packet, PacketType},
    Connection, PrivateKey, PublicKey,
};
use quests::Quests;
use rand::Rng;
use rsa::{
    pkcs8::{DecodePrivateKey as _, EncodePrivateKey as _},
    traits::PublicKeyParts as _,
    RsaPrivateKey,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{self, Cursor},
    net::Ipv4Addr,
    sync::{atomic::AtomicU32, Arc},
};
use thiserror::Error;
use user::*;

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    db_name: String,
    blocks: Vec<BlockSettings>,
    balance_port: u16,
    master_ship: String,
    quest_dir: String,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct BlockSettings {
    port: Option<u16>,
    name: String,
    max_players: u32,
    maps: HashMap<String, String>,
}

impl Settings {
    async fn load(path: &str) -> Result<Settings, Error> {
        let string = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(_) => {
                let mut settings = Settings::default();
                settings.blocks.push(BlockSettings {
                    port: Some(13002),
                    name: "Block 2".into(),
                    ..Default::default()
                });
                tokio::fs::write(path, toml::to_string_pretty(&settings)?).await?;
                return Ok(settings);
            }
        };
        Ok(toml::from_str(&string)?)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            db_name: "sqlite://ship.db".into(),
            balance_port: 12000,
            blocks: vec![BlockSettings::default()],
            master_ship: "localhost:15000".into(),
            quest_dir: "quests".to_string(),
        }
    }
}
impl Default for BlockSettings {
    fn default() -> Self {
        Self {
            port: None,
            name: "Block 1".to_string(),
            max_players: 32,
            maps: HashMap::from([("lobby".to_string(), "lobby.mp".to_string())]),
        }
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid input in fn {0}")]
    InvalidInput(&'static str),
    #[error("Invalid password")]
    InvalidPassword,
    #[error("No user found")]
    NoUser,
    #[error("Unable to hash the password")]
    HashError,
    #[error("Invalid character")]
    InvalidCharacter,
    #[error("No character loaded")]
    NoCharacter,
    #[error("No lobby map")]
    NoLobby,
    #[error("Master ship returned error: {0}")]
    MSError(String),
    #[error("Master ship sent unexpected data")]
    MSUnexpected,

    // passthrough errors
    #[error(transparent)]
    SqlError(#[from] sqlx::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    DataError(#[from] data_structs::Error),
    #[error(transparent)]
    LuaError(#[from] mlua::Error),
    #[error(transparent)]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    UTF8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    TomlSerError(#[from] toml::ser::Error),
    #[error(transparent)]
    TomlDeError(#[from] toml::de::Error),
    #[error(transparent)]
    RSAError(#[from] rsa::Error),
    #[error(transparent)]
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
    maps: HashMap<String, String>,
    quests: Arc<Quests>,
}

struct BlockData {
    sql: Arc<sql::Sql>,
    block_id: u32,
    block_name: String,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    //TODO: remove rwlock after testing is done (waaay is the future)
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

pub async fn run() -> Result<(), Error> {
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
    let settings = Settings::load("ship.toml").await?;
    let (data_pc, data_vita) = create_attr_files(&mul_progress)?;
    let quests = Arc::new(Quests::load(&settings.quest_dir));
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
    let mut blockstatus_lock = server_statuses.write().await;
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
            quests: quests.clone(),
        };
        blockstatus_lock.push(new_block.clone());
        let server_statuses = server_statuses.clone();
        let sql = sql.clone();
        let item_data = item_data.clone();
        let key = PrivateKey::Key(key.clone());
        blocks.push(tokio::spawn(async move {
            match block::init_block(server_statuses, new_block, sql, item_data, key).await {
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
                    let _ =
                        send_block_balance(s.into_std().unwrap(), server_statuses.clone()).await;
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

async fn send_block_balance(
    stream: std::net::TcpStream,
    servers: Arc<RwLock<Vec<BlockInfo>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(
        stream,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let mut servers = servers.write().await;
    let server_count = servers.len() as u32;
    for block in servers.iter_mut() {
        if block.ip == Ipv4Addr::UNSPECIFIED {
            if let std::net::IpAddr::V4(addr) = local_addr {
                block.ip = addr
            }
        }
    }
    let server = servers
        .get_mut(rand::thread_rng().gen_range(0..server_count) as usize)
        .unwrap();
    let packet = login::BlockBalancePacket {
        ip: server.ip,
        port: server.port,
        blockname: server.name.clone(),
        ..Default::default()
    };
    con.write_packet(&Packet::BlockBalance(packet))?;
    Ok(())
}

fn create_attr_files(mul_progress: &MultiProgress) -> Result<(Vec<u8>, Vec<u8>), Error> {
    let progress = mul_progress.add(ProgressBar::new_spinner());
    progress.set_message("Loading item attributes...");
    let attrs_str = std::fs::read_to_string("item_attrs.json")?;
    progress.set_message("Parsing item attributes...");
    let attrs: item_attrs::ItemAttributes = serde_json::from_str(&attrs_str)?;

    // PC attributes
    progress.set_message("Creating PC item attributes...");
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
    progress.set_message("Creating Vita item attributes...");
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

    progress.finish_with_message("Created item attributes");
    Ok((outdata_pc, outdata_vita))
}
