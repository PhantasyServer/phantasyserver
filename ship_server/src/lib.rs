#![deny(clippy::undocumented_unsafe_blocks)]
pub mod ice;
pub mod inventory;
pub mod invites;
pub mod map;
pub mod master_conn;
pub mod palette;
pub mod party;
pub mod sql;
pub mod user;
use console::style;
use data_structs::ItemParameters;
use ice::{IceFileInfo, IceWriter};
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};
use pso2packetlib::{
    protocol::{self, login, models::item_attrs, Packet, PacketType},
    Connection, PrivateKey, PublicKey,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::{self, Cursor},
    net::{Ipv4Addr, TcpListener},
    sync::{mpsc, Arc},
    time::Duration,
};
use thiserror::Error;
pub use user::*;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub db_name: String,
    pub blocks: Vec<BlockSettings>,
    pub balance_port: u16,
    pub master_ship: String,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct BlockSettings {
    pub port: Option<u16>,
    pub name: String,
    pub max_players: u32,
    pub maps: HashMap<String, String>,
}

impl Settings {
    pub async fn load(path: &str) -> Result<Settings, Error> {
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
    #[error("Invalid input")]
    InvalidInput,
    #[error("Invalid password")]
    InvalidPassword(u32),
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
    UTF8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    TomlSerError(#[from] toml::ser::Error),
    #[error(transparent)]
    TomlDeError(#[from] toml::de::Error),
    #[error("{0}")]
    Generic(String),
}

#[derive(Clone)]
pub struct BlockInfo {
    pub id: u32,
    pub name: String,
    pub ip: Ipv4Addr,
    pub port: u16,
    pub max_players: u32,
    pub players: u32,
    pub maps: HashMap<String, String>,
}

struct BlockData {
    sql: Arc<sql::Sql>,
    block_id: u32,
    block_name: String,
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    item_attrs: Arc<RwLock<ItemParameters>>,
    lobby: Arc<Mutex<map::Map>>,
    key: PrivateKey,
}

#[derive(Default, Clone)]
pub enum Action {
    #[default]
    Nothing,
    Disconnect,
    InitialLoad,
    // party related
    SendPartyInvite(u32),
    AcceptPartyInvite(u32),
    KickPartyMember(protocol::ObjectHeader),
    DisbandParty,
    LeaveParty,
    // map related
    LoadLobby,
}

pub async fn init_block(
    blocks: Arc<RwLock<Vec<BlockInfo>>>,
    this_block: BlockInfo,
    sql: Arc<sql::Sql>,
    item_attrs: Arc<RwLock<ItemParameters>>,
    key: PrivateKey,
) -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", this_block.port))?;
    listener.set_nonblocking(true)?;

    let mut latest_mapid = 0;
    let mut latest_partyid = 0;

    let mut maps = HashMap::new();

    for (map_name, map_path) in this_block.maps {
        match map::Map::new(map_path, latest_mapid) {
            Ok(x) => {
                maps.insert(map_name, Arc::new(Mutex::new(x)));
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    style(format!("Failed to load map {}: {}", map_name, e)).red()
                );
            }
        }
        latest_mapid += 1;
    }

    let lobby = match maps.get("lobby") {
        Some(x) => x.clone(),
        None => return Err(Error::NoLobby),
    };

    let block_data = Arc::new(BlockData {
        sql,
        blocks,
        item_attrs,
        block_id: this_block.id,
        block_name: this_block.name,
        lobby,
        key,
    });

    let mut clients = vec![];
    let mut conn_id = 0usize;
    let (send, recv) = mpsc::channel();

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("{}", style("Client connected").cyan());
                    let mut lock = async_write(&block_data.blocks).await;
                    if let Some(block) = lock.iter_mut().find(|x| x.id == this_block.id) {
                        if block.players >= block.max_players {
                            continue;
                        }
                        block.players += 1;
                    }
                    drop(lock);
                    let client = Arc::new(Mutex::new(User::new(s, block_data.clone())?));
                    clients.push((conn_id, client.clone()));
                    let send = send.clone();
                    tokio::spawn(async move {
                        loop {
                            match User::tick(async_lock(&client).await).await {
                                Ok(Action::Nothing) => {}
                                Ok(a) => {
                                    send.send((conn_id, a)).unwrap();
                                }
                                Err(Error::IOError(e)) if e.kind() == io::ErrorKind::WouldBlock => {
                                }
                                Err(Error::IOError(e))
                                    if e.kind() == io::ErrorKind::ConnectionAborted =>
                                {
                                    send.send((conn_id, Action::Disconnect)).unwrap();
                                    return;
                                }
                                Err(e) => {
                                    let error_msg = format!("Client error: {e}");
                                    let _ = async_lock(&client).await.send_error(&error_msg);
                                    eprintln!("{}", style(error_msg).red());
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    });

                    conn_id += 1;
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        while let Ok((id, action)) = recv.try_recv() {
            match run_action(&mut clients, id, action, &block_data, &mut latest_partyid).await {
                Ok(_) => {}
                Err(e) => eprintln!("{}", style(format!("Client error: {e}")).red()),
            };
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

async fn run_action(
    clients: &mut Vec<(usize, Arc<Mutex<User>>)>,
    conn_id: usize,
    action: Action,
    block_data: &Arc<BlockData>,
    latest_partyid: &mut u32,
) -> Result<(), Error> {
    let Some((pos, _)) = clients
        .iter()
        .enumerate()
        .find(|(_, (c_conn_id, _))| *c_conn_id == conn_id)
    else {
        return Ok(());
    };
    match action {
        Action::Nothing => {}
        Action::Disconnect => {
            println!("{}", style("Client disconnected").cyan());
            let mut lock = async_write(&block_data.blocks).await;
            if let Some(block) = lock.iter_mut().find(|x| x.id == block_data.block_id) {
                block.players -= 1;
            }
            drop(lock);
            clients.remove(pos);
        }
        Action::InitialLoad => {
            let (_, user) = &clients[pos];
            let mut user_lock = async_lock(user).await;
            user_lock.set_map(block_data.lobby.clone());
            drop(user_lock);
            party::Party::init_player(user.clone(), latest_partyid)?;
            block_data.lobby.lock().add_player(user.clone())?;
        }
        Action::SendPartyInvite(invitee) => {
            let (_, inviter) = &clients[pos];
            let invitee = async {
                for client in clients.iter().map(|(_, p)| p) {
                    if async_lock(client).await.player_id == invitee {
                        return Some(client.clone());
                    }
                }
                return None;
            }
            .await;
            if let Some(invitee) = invitee {
                party::Party::send_invite(inviter.clone(), invitee)?;
            }
        }
        Action::LoadLobby => {
            let (_, user) = &clients[pos];
            async_lock(user).await.set_map(block_data.lobby.clone());
            async_lock(&block_data.lobby)
                .await
                .add_player(user.clone())?;
        }
        Action::AcceptPartyInvite(party_id) => {
            let (_, user) = &clients[pos];
            party::Party::accept_invite(user.clone(), party_id)?;
        }
        Action::LeaveParty => {
            let (_, user) = &clients[pos];
            async_lock(user)
                .await
                .send_packet(&pso2packetlib::protocol::Packet::RemovedFromParty)?;
            party::Party::init_player(user.clone(), latest_partyid)?;
        }
        Action::DisbandParty => {
            let (_, user) = &clients[pos];
            let party = async_lock(user).await.get_current_party();
            if let Some(party) = party {
                async_write(&party).await.disband_party(latest_partyid)?;
            }
        }
        Action::KickPartyMember(data) => {
            let (_, user) = &clients[pos];
            let party = async_lock(user).await.get_current_party();
            if let Some(party) = party {
                async_write(&party)
                    .await
                    .kick_player(data.id, latest_partyid)?;
            }
        }
    }
    Ok(())
}

pub fn send_block_balance(
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
    let mut servers = servers.write();
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

pub fn create_attr_files(mul_progress: &MultiProgress) -> Result<(Vec<u8>, Vec<u8>), Error> {
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

async fn async_lock<T>(mutex: &Mutex<T>) -> MutexGuard<T> {
    loop {
        match mutex.try_lock() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}

async fn async_read<T>(lock: &RwLock<T>) -> RwLockReadGuard<T> {
    loop {
        match lock.try_read() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}

async fn async_write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<T> {
    loop {
        match lock.try_write() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}
