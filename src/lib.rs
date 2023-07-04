pub mod sql;
use byteorder::{LittleEndian, WriteBytesExt};
use pso2packetlib::{
    protocol::{self, login, models::character::Character, ObjectHeader, Packet, PacketHeader},
    Connection,
};
use rand::Rng;
use sql::Sql;
use std::{
    io,
    net::Ipv4Addr,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    SqlError(#[from] sql::Error),
    #[error(transparent)]
    IOError(#[from] io::Error),
}

pub struct User {
    connection: Connection,
    ip: Ipv4Addr,
    sql: Arc<Sql>,
    player_id: u32,
    char_id: u32,
    character: Option<Character>,
    last_ping: Instant,
    failed_pings: u32,
    is_global: bool,
    ready_to_shutdown: bool,
}

impl User {
    pub fn new(stream: std::net::TcpStream, sql: Arc<Sql>) -> Result<User, Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let ip = stream.peer_addr()?.ip();
        let ip = match ip {
            std::net::IpAddr::V4(x) => x,
            std::net::IpAddr::V6(_) => Ipv4Addr::UNSPECIFIED,
        };
        let mut con = Connection::new(stream, false, Some("keypair.pem".into()), None);
        match con.write_packet(Packet::ServerHello(protocol::ServerHelloPacket {
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
            last_ping: Instant::now(),
            failed_pings: 0,
            is_global: false,
            ready_to_shutdown: false,
        })
    }
    pub fn tick(&mut self) -> Result<(), Error> {
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
            let _ = self.connection.write_packet(Packet::ServerPing);
        }
        match self.connection.read_packet() {
            Ok(packet) => packet_handler(self, packet)?,
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(())
    }
}

pub struct ServerInfo {
    pub id: u32,
    pub name: String,
    pub ip: [u8; 4],
    pub port: u16,
    pub status: u16,
    pub order: u16,
}

fn packet_handler(user: &mut User, packet: Packet) -> Result<(), Error> {
    match packet {
        Packet::EncryptionRequest(_) => {
            let key = user.connection.get_key();
            user.connection.write_packet(respond_enc(key))?;
        }
        Packet::SegaIDLogin(packet) => {
            // user.connection.write_packet(Packet::NicknameRequest(login::NicknameRequestPacket {
            //     error: 0,
            // }))?;
            user.is_global = true;
            let (mut id, mut status, mut error) = Default::default();
            match user.sql.get_sega_user(&packet.username, &packet.password) {
                Ok(x) => {
                    id = x.id;
                    user.sql
                        .put_login(id, user.ip, login::LoginResult::Successful)?;
                }
                Err(sql::Error::InvalidPassword(id)) => {
                    status = login::LoginStatus::Failure;
                    error = "Invalid password".to_string();
                    user.sql
                        .put_login(id, user.ip, login::LoginResult::LoginError)?;
                }
                Err(sql::Error::InvalidInput) => {
                    status = login::LoginStatus::Failure;
                    error = "Empty username or password".to_string();
                }
                Err(e) => return Err(e.into()),
            }
            user.player_id = id;
            user.connection
                .write_packet(Packet::LoginResponse(login::LoginResponsePacket {
                    status,
                    error,
                    player: ObjectHeader {
                        id,
                        entity_type: protocol::EntityType::Player,
                    },
                    ..Default::default()
                }))?;
        }
        Packet::ServerPong => user.failed_pings -= 1,
        Packet::VitaLogin(x) => {
            let user_psn = user.sql.get_psn_user(&x.username)?;
            user.player_id = user_psn.id;
            user.sql
                .put_login(user.player_id, user.ip, login::LoginResult::Successful)?;
            user.connection
                .write_packet(Packet::LoginResponse(login::LoginResponsePacket {
                    player: ObjectHeader {
                        id: user_psn.id,
                        entity_type: protocol::EntityType::Player,
                    },
                    ..Default::default()
                }))?;
        }
        Packet::SettingsRequest => {
            let settings = user.sql.get_settings(user.player_id)?;
            user.connection
                .write_packet(Packet::LoadSettings(protocol::LoadSettingsPacket {
                    settings,
                }))?;
        }
        Packet::SaveSettings(packet) => {
            user.sql.save_settings(user.player_id, &packet.settings)?;
        }
        Packet::ClientPing(packet) => {
            let response = login::ClientPongPacket {
                client_time: packet.time,
                server_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
            };
            user.connection.write_packet(Packet::ClientPong(response))?;
        }
        Packet::ClientGoodbye => {
            user.ready_to_shutdown = true;
            user.last_ping = Instant::now();
        }
        Packet::CharacterListRequest => {
            let test = user.sql.get_characters(user.player_id)?;
            user.connection.write_packet(Packet::CharacterListResponse(
                login::CharacterListPacket {
                    characters: test,
                    is_global: user.is_global,
                    ..Default::default()
                },
            ))?;
        }
        Packet::CreateCharacter1 => {
            user.connection
                .write_packet(Packet::CreateCharacter1Response(
                    login::CreateCharacter1ResponsePacket::default(),
                ))?;
        }
        Packet::CreateCharacter2 => {
            user.connection
                .write_packet(Packet::CreateCharacter2Response(
                    login::CreateCharacter2ResponsePacket { unk: 1 },
                ))?;
        }
        Packet::CharacterCreate(packet) => {
            user.char_id = user.sql.put_character(user.player_id, &packet.character)?;
            user.character = Some(packet.character);
        }
        Packet::StartGame(packet) => {
            user.char_id = packet.char_id;
            user.character = Some(user.sql.get_character(user.player_id, user.char_id)?);
            // maybe send extra packets before this
            user.connection
                .write_packet(Packet::LoadingScreenTransition)?;
        }
        Packet::InitialLoad => {
            // we need to load the map, spawn character and etc. but i'm unsure about packet format.

            // let packet = SetPlayerIDPacket{ player_id: user.player_id, ..Default::default()};
            // user.connection.write_packet(Packet::SetPlayerID(packet))?;

            // let packet = protocol::CharacterSpawnPacket{character: user.character.clone().unwrap(), player_obj: ObjectHeader { id: user.player_id, entity_type: protocol::EntityType::Player }, is_global: user.is_global, ..Default::default()};
            // user.connection.write_packet(Packet::CharacterSpawn(packet))?;

            // unlock controls?
            // user.connection.write_packet(Packet::Unknown((PacketHeader{id: 0x3, subid: 0x2b, ..Default::default()}, vec![])))?;
        }
        Packet::LoginHistoryRequest => {
            let attempts = user.sql.get_logins(user.player_id)?;
            user.connection.write_packet(Packet::LoginHistoryResponse(
                login::LoginHistoryPacket { attempts },
            ))?;
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
            user.connection.write_packet(Packet::Unknown((
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
    Ok(())
}

fn respond_enc(key: Vec<u8>) -> Packet {
    Packet::EncryptionResponse(login::EncryptionResponsePacket { data: key })
}

pub async fn send_querry(
    stream: std::net::TcpStream,
    servers: Arc<Mutex<Vec<ServerInfo>>>,
) -> io::Result<()> {
    stream.set_nonblocking(true)?;
    stream.set_nodelay(true)?;
    let local_addr = stream.local_addr()?.ip();
    let mut con = Connection::new(stream, false, None, None);
    let mut ships = vec![];
    for server in servers.lock().unwrap().iter_mut() {
        let ip = if server.ip == [0, 0, 0, 0] {
            if let std::net::IpAddr::V4(addr) = local_addr {
                addr
            } else {
                Ipv4Addr::UNSPECIFIED
            }
        } else {
            Ipv4Addr::from(server.ip)
        };
        ships.push(login::ShipEntry {
            id: server.id,
            ip,
            name: server.name.clone(),
            order: server.order,
            status: login::ShipStatus::Full,
        })
    }
    con.write_packet(Packet::ShipList(login::ShipListPacket {
        ships,
        ..Default::default()
    }))?;
    Ok(())
}

pub async fn send_block_balance(
    stream: std::net::TcpStream,
    servers: Arc<Mutex<Vec<ServerInfo>>>,
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
        blockname: "Test".to_string(),
        ..Default::default()
    };
    con.write_packet(Packet::BlockBalance(packet))?;
    Ok(())
}
