pub mod map;
pub mod sql;
use byteorder::{LittleEndian, WriteBytesExt};
use map::Map;
use pso2packetlib::{
    protocol::{
        self, login,
        models::{character::Character, Position},
        spawn::CharacterSpawnPacket,
        symbolart::{
            SendSymbolArtPacket, SymbolArtClientDataPacket, SymbolArtDataRequestPacket,
            SymbolArtListPacket,
        },
        ChatArea, ObjectHeader, Packet, PacketHeader,
    },
    Connection,
};
use rand::Rng;
use sql::Sql;
use std::{
    cell::RefCell,
    io,
    net::Ipv4Addr,
    rc::Rc,
    sync::{Arc, Mutex, RwLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

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
    #[error("No character loader")]
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
}

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
    LoadLobby,
    SendPosition(Packet),
    SendMapMessage(Packet),
    SendMapSA(SendSymbolArtPacket),
}

pub struct User {
    connection: Connection,
    ip: Ipv4Addr,
    sql: Arc<RwLock<Sql>>,
    player_id: u32,
    char_id: u32,
    position: Position,
    map: Option<Rc<RefCell<Map>>>,
    character: Option<Character>,
    last_ping: Instant,
    failed_pings: u32,
    is_global: bool,
    ready_to_shutdown: bool,
    blockname: String,
}

impl User {
    pub fn new(
        stream: std::net::TcpStream,
        sql: Arc<RwLock<Sql>>,
        blockname: String,
    ) -> Result<User, Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let ip = stream.peer_addr()?.ip();
        let ip = match ip {
            std::net::IpAddr::V4(x) => x,
            std::net::IpAddr::V6(_) => Ipv4Addr::UNSPECIFIED,
        };
        let mut con = Connection::new(stream, false, Some("keypair.pem".into()), None);
        match con.write_packet(&Packet::ServerHello(protocol::server::ServerHelloPacket {
            version: 0xc9,
        })) {
            Ok(_) => {}
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(User {
            connection: con,
            ip,
            sql,
            player_id: 0,
            char_id: 0,
            character: None,
            map: None,
            position: Default::default(),
            last_ping: Instant::now(),
            failed_pings: 0,
            is_global: false,
            ready_to_shutdown: false,
            blockname,
        })
    }
    pub fn tick(&mut self) -> Result<Action, Error> {
        let _ = self.connection.flush();
        if self.ready_to_shutdown && self.last_ping.elapsed().as_millis() >= 500 {
            return Err(Error::IOError(std::io::ErrorKind::ConnectionAborted.into()));
        }
        if self.failed_pings >= 5 {
            return Err(Error::IOError(std::io::ErrorKind::ConnectionAborted.into()));
        }
        if self.last_ping.elapsed().as_secs() >= 10 {
            self.last_ping = Instant::now();
            self.failed_pings += 1;
            let _ = self.connection.write_packet(&Packet::ServerPing);
        }
        match self.connection.read_packet() {
            Ok(packet) => match packet_handler(self, packet) {
                Ok(action) => return Ok(action),
                Err(Error::IOError(x)) if x.kind() == io::ErrorKind::WouldBlock => {}
                Err(x) => return Err(x),
            },
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(Action::Nothing)
    }
    pub fn send_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        self.connection.write_packet(packet)?;
        Ok(())
    }
    pub fn spawn_character(&mut self, mut packet: CharacterSpawnPacket) -> Result<(), Error> {
        packet.is_global = self.is_global;
        self.send_packet(&Packet::CharacterSpawn(packet))?;
        Ok(())
    }
    pub fn get_current_map(&self) -> Option<Rc<RefCell<Map>>> {
        self.map.clone()
    }
    pub fn set_map(&mut self, map: Rc<RefCell<Map>>) {
        self.map = Some(map)
    }
    pub fn get_user_id(&self) -> u32 {
        self.player_id
    }
}

fn packet_handler(user: &mut User, packet: Packet) -> Result<Action, Error> {
    let sql_provider = user.sql.clone();
    match packet {
        Packet::EncryptionRequest(_) => {
            let key = user.connection.get_key();
            user.send_packet(&respond_enc(key))?;
        }
        Packet::SegaIDLogin(packet) => {
            user.is_global = true;
            let (mut id, mut status, mut error) = Default::default();
            let mut sql = sql_provider.write().unwrap();
            match sql.get_sega_user(&packet.username, &packet.password) {
                Ok(x) => {
                    id = x.id;
                    sql.put_login(id, user.ip, login::LoginResult::Successful)?;
                }
                Err(Error::InvalidPassword(id)) => {
                    status = login::LoginStatus::Failure;
                    error = "Invalid password".to_string();
                    sql.put_login(id, user.ip, login::LoginResult::LoginError)?;
                }
                Err(Error::InvalidInput) => {
                    status = login::LoginStatus::Failure;
                    error = "Empty username or password".to_string();
                }
                Err(e) => return Err(e),
            }
            user.player_id = id;
            user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
                status,
                error,
                blockname: user.blockname.clone(),
                player: ObjectHeader {
                    id,
                    entity_type: protocol::EntityType::Player,
                    ..Default::default()
                },
                ..Default::default()
            }))?;
            //TODO: send item attributes
        }
        Packet::ServerPong => user.failed_pings -= 1,
        Packet::VitaLogin(x) => {
            let mut sql = sql_provider.write().unwrap();
            let user_psn = sql.get_psn_user(&x.username)?;
            user.player_id = user_psn.id;
            sql.put_login(user.player_id, user.ip, login::LoginResult::Successful)?;
            user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
                status: login::LoginStatus::Success,
                player: ObjectHeader {
                    id: user_psn.id,
                    entity_type: protocol::EntityType::Player,
                    ..Default::default()
                },
                blockname: user.blockname.clone(),
                ..Default::default()
            }))?;
        }
        Packet::SettingsRequest => {
            let sql = sql_provider.read().unwrap();
            let settings = sql.get_settings(user.player_id)?;
            user.send_packet(&Packet::LoadSettings(protocol::LoadSettingsPacket {
                settings,
            }))?;
        }
        Packet::SaveSettings(packet) => {
            let mut sql = sql_provider.write().unwrap();
            sql.save_settings(user.player_id, &packet.settings)?;
        }
        Packet::ClientPing(packet) => {
            let response = login::ClientPongPacket {
                client_time: packet.time,
                server_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
                unk1: 0,
            };
            user.send_packet(&Packet::ClientPong(response))?;
        }
        Packet::ClientGoodbye => {
            user.ready_to_shutdown = true;
            user.last_ping = Instant::now();
        }
        Packet::CharacterListRequest => {
            let sql = sql_provider.read().unwrap();
            let test = sql.get_characters(user.player_id)?;
            user.send_packet(&Packet::CharacterListResponse(login::CharacterListPacket {
                characters: test,
                is_global: user.is_global,
                ..Default::default()
            }))?;
        }
        Packet::CreateCharacter1 => {
            user.send_packet(&Packet::CreateCharacter1Response(
                login::CreateCharacter1ResponsePacket::default(),
            ))?;
        }
        Packet::CreateCharacter2 => {
            user.send_packet(&Packet::CreateCharacter2Response(
                login::CreateCharacter2ResponsePacket { unk: 1 },
            ))?;
        }
        Packet::CharacterCreate(packet) => {
            let mut sql = sql_provider.write().unwrap();
            user.char_id = sql.put_character(user.player_id, &packet.character)?;
            user.character = Some(packet.character);
            user.send_packet(&Packet::LoadingScreenTransition)?;
        }
        Packet::StartGame(packet) => {
            user.char_id = packet.char_id;
            let sql = sql_provider.read().unwrap();
            user.character = Some(sql.get_character(user.player_id, user.char_id)?);

            // maybe send extra packets before this
            user.send_packet(&Packet::LoadingScreenTransition)?;
        }
        Packet::InitialLoad => {
            // TODO: send inventory, storage, etc
            return Ok(Action::LoadLobby);
        }
        Packet::Movement(ref data) => {
            if let Some(n) = data.rot_x {
                user.position.rot_x = n;
            }
            if let Some(n) = data.rot_y {
                user.position.rot_y = n;
            }
            if let Some(n) = data.rot_z {
                user.position.rot_z = n;
            }
            if let Some(n) = data.rot_w {
                user.position.rot_w = n;
            }
            if let Some(n) = data.cur_x {
                user.position.pos_x = n;
            }
            if let Some(n) = data.cur_y {
                user.position.pos_y = n;
            }
            if let Some(n) = data.cur_z {
                user.position.pos_z = n;
            }
            return Ok(Action::SendPosition(packet));
        }
        Packet::MovementEnd(ref data) => {
            user.position = data.cur_pos;
            return Ok(Action::SendPosition(packet));
        }
        Packet::MovementAction(_) => {
            return Ok(Action::SendPosition(packet));
        }
        Packet::ChatMessage(ref data) => {
            if let ChatArea::Map = data.area {
                return Ok(Action::SendMapMessage(packet));
            }
        }
        Packet::SymbolArtListRequest => {
            let sql = sql_provider.read().unwrap();
            let uuids = sql.get_symbol_art_list(user.player_id)?;
            user.send_packet(&Packet::SymbolArtList(SymbolArtListPacket {
                object: ObjectHeader {
                    id: user.player_id,
                    entity_type: protocol::EntityType::Player,
                    ..Default::default()
                },
                character_id: user.char_id,
                uuids,
            }))?;
        }
        Packet::ChangeSymbolArt(data) => {
            let mut sql = sql_provider.write().unwrap();
            let mut uuids = sql.get_symbol_art_list(user.player_id)?;
            for uuid in data.uuids {
                let slot = uuid.slot;
                let uuid = uuid.uuid;
                if let Some(data) = uuids.get_mut(slot as usize) {
                    *data = uuid;
                }
                if uuid == 0 {
                    continue;
                }
                if sql.get_symbol_art(uuid)?.is_none() {
                    user.send_packet(&Packet::SymbolArtDataRequest(SymbolArtDataRequestPacket {
                        uuid,
                    }))?;
                }
            }
            sql.set_symbol_art_list(uuids, user.player_id)?;
            user.send_packet(&Packet::SymbolArtResult(Default::default()))?;
        }
        Packet::SymbolArtData(data) => {
            let mut sql = sql_provider.write().unwrap();
            sql.add_symbol_art(data.uuid, &data.data, &data.name)?;
        }
        Packet::SymbolArtClientDataRequest(data) => {
            let sql = sql_provider.read().unwrap();
            if let Some(sa) = sql.get_symbol_art(data.uuid)? {
                user.send_packet(&Packet::SymbolArtClientData(SymbolArtClientDataPacket {
                    uuid: data.uuid,
                    data: sa,
                }))?;
            }
        }
        Packet::SendSymbolArt(data) => {
            if let ChatArea::Map = data.area {
                return Ok(Action::SendMapSA(data));
            }
        }
        Packet::MapLoaded(_) => {
            user.send_packet(&Packet::UnlockControls)?;
            user.send_packet(&Packet::FinishLoading)?;
        }
        Packet::QuestCounterRequest => {
            //TODO: send (0x0E, 0x2B), (0x49, 0x01), (0x0E, 0x65), (0x0B, 0x22)
        }
        Packet::AvailableQuestsRequest(_) => {
            //TODO: send (0x0B, 0x16)
        }
        Packet::MissionListRequest => {
            //TODO:
        }
        Packet::MissionPassInfoRequest => {
            //TODO:
        }
        Packet::MissionPassRequest => {
            //TODO:
        }
        Packet::Interact(packet) => {
            if packet.action == "READY" {
                let packett = protocol::objects::SetTagPacket {
                    object1: packet.object3,
                    object2: packet.object1,
                    object3: packet.object1,
                    attribute: "FavsNeutral".into(),
                    ..Default::default()
                };
                user.send_packet(&Packet::SetTag(packett))?;
                let packett = protocol::objects::SetTagPacket {
                    object1: packet.object3,
                    object2: packet.object1,
                    object3: packet.object1,
                    attribute: "AP".into(),
                    ..Default::default()
                };
                user.send_packet(&Packet::SetTag(packett))?;
            //This should not be here
            } else if packet.action == "Transfer" {
                let location = protocol::models::Position {
                    rot_y: half::f16::from_f32(-0.00242615),
                    rot_w: half::f16::from_f32(0.999512),
                    pos_x: half::f16::from_f32(-0.0609131),
                    pos_y: half::f16::from_f32(14.0),
                    pos_z: half::f16::from_f32(-167.125),
                    ..Default::default()
                };
                let packett = protocol::objects::TeleportTransferPacket {
                    source_tele: packet.object1,
                    location,
                    ..Default::default()
                };
                user.send_packet(&Packet::TeleportTransfer(packett))?;
                let packett = protocol::objects::SetTagPacket {
                    object1: packet.object3,
                    object2: packet.object1,
                    attribute: "Forwarded".into(),
                    ..Default::default()
                };
                user.send_packet(&Packet::SetTag(packett))?;
            } else {
                println!("{:?}", packet)
            }
        }
        Packet::SalonEntryRequest => {
            user.send_packet(&Packet::SalonEntryResponse(login::SalonResponse {
                ..Default::default()
            }))?;
        }
        Packet::LoginHistoryRequest => {
            let sql = sql_provider.read().unwrap();
            let attempts = sql.get_logins(user.player_id)?;
            user.send_packet(&Packet::LoginHistoryResponse(login::LoginHistoryPacket {
                attempts,
            }))?;
        }
        Packet::SegaIDInfoRequest => {
            let mut dataout = vec![];
            for _ in 0..0x30 {
                dataout.push(0x42);
                dataout.push(0x41);
            }
            for _ in 0..0x12 {
                dataout.write_u32::<LittleEndian>(1)?;
            }
            user.send_packet(&Packet::Unknown((
                PacketHeader {
                    id: 0x11,
                    subid: 108,
                    ..Default::default()
                },
                dataout,
            )))?;
        }
        x => println!("{:?}", x),
    }
    Ok(Action::Nothing)
}

fn respond_enc(key: Vec<u8>) -> Packet {
    Packet::EncryptionResponse(login::EncryptionResponsePacket { data: key })
}

pub fn send_querry(
    stream: std::net::TcpStream,
    servers: Arc<RwLock<Vec<login::ShipEntry>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream, false, None, None);
    let mut ships = vec![];
    for server in servers.read().unwrap().iter() {
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
    servers: Arc<Mutex<Vec<BlockInfo>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream, false, None, None);
    let mut servers = servers.lock().unwrap();
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
