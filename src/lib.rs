pub mod ice;
pub mod inventory;
pub mod invites;
pub mod map;
pub mod party;
pub mod sql;
pub mod user;
use console::style;
use ice::{IceFileInfo, IceWriter};
use indicatif::{MultiProgress, ProgressBar};
use inventory::ItemParameters;
use parking_lot::RwLock;
use pso2packetlib::{
    protocol::{
        self, login, models::item_attrs, symbolart::SendSymbolArtPacket, Packet, PacketType,
    },
    Connection,
};
use rand::Rng;
use std::{
    cell::RefCell,
    io::{self, Cursor},
    net::{Ipv4Addr, TcpListener},
    rc::Rc,
    sync::Arc,
    thread,
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
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[error(transparent)]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
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
    InitialLoad,
    // party related
    SendPartyInvite(u32),
    GetPartyDetails(protocol::party::GetPartyDetailsPacket),
    SetPartySettings(protocol::party::NewPartySettingsPacket),
    AcceptPartyInvite(u32),
    TransferLeader(protocol::ObjectHeader),
    KickPartyMember(protocol::ObjectHeader),
    DisbandParty,
    LeaveParty,
    SetBusyState(protocol::party::BusyState),
    SetChatState(protocol::party::ChatStatusPacket),
    // map related
    LoadLobby,
    SendPosition(Packet),
    SendMapMessage(Packet),
    SendMapSA(SendSymbolArtPacket),
    Interact(protocol::objects::InteractPacket),
    MapLuaReload,
}

pub fn init_block(
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
        Ok(x) => Rc::new(RefCell::new(x)),
        Err(e) => {
            eprintln!(
                "{}",
                style(format!("Failed to load lobby map: {}", e)).red()
            );
            return Err(e);
        }
    };

    let mut clients = vec![];
    let mut to_remove = vec![];
    let mut actions = vec![];

    loop {
        for stream in listener.incoming() {
            match stream {
                Ok(s) => {
                    println!("{}", style("Client connected").cyan());
                    clients.push(User::new(
                        s,
                        sql.clone(),
                        name.clone(),
                        this_block.id as u16,
                        item_attrs.clone(),
                    )?);
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => {
                    return Err(e.into());
                }
            }
        }
        for (pos, client) in clients.iter_mut().enumerate() {
            if let Some(action) = handle_error(client.tick(), &mut to_remove, client, pos) {
                actions.push((action, pos));
            }
        }
        for (action, pos) in actions.drain(..) {
            handle_error(
                run_action(&mut clients, pos, action, &lobby, &mut latest_partyid),
                &mut to_remove,
                &mut clients[pos],
                pos,
            );
        }
        to_remove.sort_unstable();
        to_remove.dedup();
        for pos in to_remove.drain(..).rev() {
            println!("{}", style("Client disconnected").cyan());
            let user = &mut clients[pos];
            let _ = user.save_inventory();
            let id = user.get_user_id();
            if let Some(party) = user.get_current_party() {
                let _ = party.borrow_mut().remove_player(&mut clients, id);
            }
            let user = &clients[pos];
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().remove_player(&mut clients, id);
            }
            clients.remove(pos);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn run_action(
    clients: &mut [User],
    pos: usize,
    action: Action,
    lobby: &Rc<RefCell<map::Map>>,
    latest_partyid: &mut u32,
) -> Result<(), Error> {
    match action {
        Action::Nothing => {}
        Action::InitialLoad => {
            let user = &mut clients[pos];
            let id = user.get_user_id();
            user.set_map(lobby.clone());
            party::Party::init_player(clients, id, latest_partyid)?;
            lobby.borrow_mut().add_player(clients, id)?;
        }
        Action::SendPartyInvite(invitee) => {
            let user = &mut clients[pos];
            let inviter = user.get_user_id();
            party::Party::send_invite(clients, inviter, invitee)?;
        }
        Action::LoadLobby => {
            let user = &mut clients[pos];
            let id = user.get_user_id();
            user.set_map(lobby.clone());
            lobby.borrow_mut().add_player(clients, id)?;
        }
        Action::GetPartyDetails(packet) => {
            party::Party::get_details(clients, pos, packet)?;
        }
        Action::AcceptPartyInvite(party_id) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            party::Party::accept_invite(clients, id, party_id)?;
        }
        Action::LeaveParty => {
            let user = &mut clients[pos];
            let id = user.get_user_id();
            user.send_packet(&pso2packetlib::protocol::Packet::RemovedFromParty)?;
            party::Party::init_player(clients, id, latest_partyid)?;
        }
        Action::TransferLeader(data) => {
            let user = &mut clients[pos];
            if let Some(party) = user.get_current_party() {
                party.borrow_mut().change_leader(clients, data)?;
            }
        }
        Action::DisbandParty => {
            let user = &mut clients[pos];
            let id = user.get_user_id();
            party::Party::disband_party(clients, id, latest_partyid)?;
        }
        Action::KickPartyMember(data) => {
            let user = &mut clients[pos];
            if let Some(party) = user.get_current_party() {
                party
                    .borrow_mut()
                    .kick_player(clients, data.id, latest_partyid)?;
            }
        }
        Action::SetPartySettings(packet) => {
            let user = &mut clients[pos];
            if let Some(party) = user.get_current_party() {
                party.borrow_mut().set_settings(clients, packet)?;
            }
        }
        Action::SendPosition(packet) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().send_movement(clients, packet, id);
            }
        }
        Action::SendMapMessage(packet) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().send_message(clients, packet, id);
            }
        }
        Action::SendMapSA(packet) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().send_sa(clients, packet, id);
            }
        }
        Action::Interact(packet) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().interaction(clients, packet, id)?;
            }
        }
        Action::MapLuaReload => {
            let user = &clients[pos];
            if let Some(map) = user.get_current_map() {
                map.borrow_mut().reload_lua()?;
            }
        }
        Action::SetBusyState(state) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(party) = user.get_current_party() {
                party.borrow().set_busy_state(clients, state, id);
            }
        }
        Action::SetChatState(state) => {
            let user = &clients[pos];
            let id = user.get_user_id();
            if let Some(party) = user.get_current_party() {
                party.borrow().set_chat_status(clients, state, id);
            }
        }
    }
    Ok(())
}

fn handle_error<T>(
    result: Result<T, Error>,
    to_remove: &mut Vec<usize>,
    user: &mut User,
    pos: usize,
) -> Option<T> {
    match result {
        Ok(t) => Some(t),
        Err(Error::IOError(x)) if x.kind() == io::ErrorKind::ConnectionAborted => {
            to_remove.push(pos);
            None
        }
        Err(Error::IOError(x)) if x.kind() == io::ErrorKind::WouldBlock => None,
        Err(x) => {
            // to_remove.push(pos);
            let error_msg = format!("Client error: {x}");
            let _ = user.send_error(&error_msg);
            eprintln!("{}", style(error_msg).red());
            None
        }
    }
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
