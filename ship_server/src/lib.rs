pub mod ice;
pub mod inventory;
pub mod invites;
pub mod map;
pub mod palette;
pub mod party;
pub mod sql;
pub mod user;
use console::style;
use data_structs::ItemParameters;
use ice::{IceFileInfo, IceWriter};
use indicatif::{MultiProgress, ProgressBar};
use parking_lot::{Mutex, MutexGuard, RwLock};
use pso2packetlib::{
    protocol::{self, login, models::item_attrs, Packet, PacketType},
    Connection,
};
use rand::Rng;
use std::{
    io::{self, Cursor},
    net::{Ipv4Addr, TcpListener},
    sync::{mpsc, Arc},
    time::Duration,
};
use thiserror::Error;
pub use user::*;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid input")]
    InvalidInput,
    #[error("Invalid password")]
    InvalidPassword(u32),
    #[error("Unable to hash the password")]
    HashError,
    #[error("Invalid character")]
    InvalidCharacter,
    #[error("No character loaded")]
    NoCharacter,
    #[error(transparent)]
    SQLError(#[from] sqlite::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    DataError(#[from] data_structs::Error),
    #[error(transparent)]
    LuaError(#[from] mlua::Error),
}

#[derive(Clone)]
pub struct BlockInfo {
    pub id: u32,
    pub name: String,
    pub ip: [u8; 4],
    pub port: u16,
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
    _server_statuses: Arc<RwLock<Vec<BlockInfo>>>,
    this_block: BlockInfo,
    sql: Arc<RwLock<sql::Sql>>,
    item_attrs: Arc<RwLock<ItemParameters>>,
) -> Result<(), Error> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", this_block.port))?;
    let name = &this_block.name;
    listener.set_nonblocking(true)?;

    let mut latest_mapid = 0;
    let mut latest_partyid = 0;

    let lobby = match map::Map::new("lobby.mp", &mut latest_mapid) {
        Ok(x) => Arc::new(Mutex::new(x)),
        Err(e) => {
            eprintln!(
                "{}",
                style(format!("Failed to load lobby map: {}", e)).red()
            );
            return Err(e);
        }
    };

    let mut clients = vec![];
    let mut conn_id = 0usize;
    let (send, recv) = mpsc::channel();

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("{}", style("Client connected").cyan());
                    let client = Arc::new(Mutex::new(User::new(
                        s,
                        sql.clone(),
                        name.clone(),
                        this_block.id as u16,
                        item_attrs.clone(),
                    )?));
                    clients.push((conn_id, client.clone()));
                    let send = send.clone();
                    tokio::spawn(async move {
                        loop {
                            match User::tick(async_lock(&client).await).await {
                                Ok(a) if matches!(a, Action::Nothing) => {}
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
            match run_action(&mut clients, id, action, &lobby, &mut latest_partyid).await {
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
    lobby: &Arc<Mutex<map::Map>>,
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
            clients.remove(pos);
        }
        Action::InitialLoad => {
            let (_, user) = &clients[pos];
            let mut user_lock = async_lock(&user).await;
            user_lock.set_map(lobby.clone());
            drop(user_lock);
            party::Party::init_player(user.clone(), latest_partyid)?;
            lobby.lock().add_player(user.clone())?;
        }
        Action::SendPartyInvite(invitee) => {
            let (_, inviter) = &clients[pos];
            let Some(invitee) = clients
                .iter()
                .map(|(_, p)| p)
                .find(|p| p.lock().player_id == invitee)
                .cloned()
            else {
                return Ok(());
            };
            party::Party::send_invite(inviter.clone(), invitee)?;
        }
        Action::LoadLobby => {
            let (_, user) = &clients[pos];
            async_lock(&user).await.set_map(lobby.clone());
            lobby.lock().add_player(user.clone())?;
        }
        Action::AcceptPartyInvite(party_id) => {
            let (_, user) = &clients[pos];
            party::Party::accept_invite(user.clone(), party_id)?;
        }
        Action::LeaveParty => {
            let (_, user) = &clients[pos];
            async_lock(&user)
                .await
                .send_packet(&pso2packetlib::protocol::Packet::RemovedFromParty)?;
            party::Party::init_player(user.clone(), latest_partyid)?;
        }
        Action::DisbandParty => {
            let (_, user) = &clients[pos];
            party::Party::disband_party(user.clone(), latest_partyid)?;
        }
        Action::KickPartyMember(data) => {
            let (_, user) = &clients[pos];
            let party = async_lock(&user).await.get_current_party();
            if let Some(party) = party {
                party.write().kick_player(data.id, latest_partyid)?;
            }
        }
    }
    Ok(())
}

pub fn send_querry(
    stream: std::net::TcpStream,
    servers: Arc<RwLock<Vec<login::ShipEntry>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream, PacketType::Classic, None, None);
    let mut ships = vec![];
    for server in servers.read().iter() {
        let mut ship = server.clone();
        if ship.ip == Ipv4Addr::UNSPECIFIED {
            if let std::net::IpAddr::V4(addr) = local_addr {
                ship.ip = addr
            }
        }
        ships.push(ship);
    }
    con.write_packet(&Packet::ShipList(login::ShipListPacket {
        ships,
        ..Default::default()
    }))?;
    Ok(())
}

pub fn send_block_balance(
    stream: std::net::TcpStream,
    servers: Arc<RwLock<Vec<BlockInfo>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream, PacketType::Classic, None, None);
    let mut servers = servers.write();
    let server_count = servers.len() as u32;
    let server = servers
        .get_mut(rand::thread_rng().gen_range(0..server_count) as usize)
        .unwrap();
    let ip = if server.ip == [0, 0, 0, 0] {
        if let std::net::IpAddr::V4(addr) = local_addr {
            addr
        } else {
            Ipv4Addr::UNSPECIFIED
        }
    } else {
        Ipv4Addr::from(server.ip)
    };
    let packet = login::BlockBalancePacket {
        ip,
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
    //INFO: maybe move attr files to memory?
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
    let outdata_pc = ice_writer.into_inner().map_err(|(_, e)| e)?.into_inner();

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
    let outdata_vita = ice_writer.into_inner().map_err(|(_, e)| e)?.into_inner();

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
