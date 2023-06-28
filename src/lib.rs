use byteorder::{BigEndian, LittleEndian, WriteBytesExt};
use pso2packetlib::{
    protocol::{
        self,
        login::{self, LoginAttempt},
        models::character,
        Packet, PacketHeader,
    },
    Connection,
};
use rand::Rng;
use std::time::{Instant, UNIX_EPOCH};
use std::{
    fs::File,
    io::{self, Write},
    sync::{Arc, Mutex},
    time::SystemTime,
};

//TODO: Add database support

pub struct User {
    connection: Connection,
    last_ping: Instant,
    failed_pings: u32,
    is_global: bool,
    ready_to_shutdown: bool,
}

impl User {
    pub fn new(stream: std::net::TcpStream) -> io::Result<User> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let mut con = Connection::new(stream, false, Some("keypair.pem".into()), None);
        match con.write_packet(Packet::ServerHello(protocol::ServerHelloPacket {
            version: 0xc9,
        })) {
            Ok(_) => {}
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x),
        }
        Ok(User {
            connection: con,
            last_ping: Instant::now(),
            failed_pings: 0,
            is_global: false,
            ready_to_shutdown: false,
        })
    }
    pub fn tick(&mut self) -> io::Result<()> {
        let _ = self.connection.flush();
        if self.ready_to_shutdown && self.last_ping.elapsed().as_millis() >= 500 {
            return Err(std::io::ErrorKind::ConnectionAborted.into());
        }
        if self.failed_pings >= 5 {
            return Err(std::io::ErrorKind::ConnectionAborted.into());
        }
        if self.last_ping.elapsed().as_secs() >= 10 {
            self.last_ping = Instant::now();
            self.failed_pings += 1;
            let _ = self.connection.write_packet(Packet::ServerPing);
        }
        match self.connection.read_packet() {
            Ok(packet) => packet_handler(self, packet)?,
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x),
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

fn packet_handler(user: &mut User, packet: Packet) -> io::Result<()> {
    match packet {
        Packet::EncryptionRequest(_) => {
            let key = user.connection.get_key();
            user.connection.write_packet(respond_enc(key))?;
        }
        Packet::SegaIDLogin(_) => {
            // con.write_packet(Packet::NicknameRequest(proto::NicknameRequestPacket {
            //     error: 0,
            // }))?;
            // user.connection.write_packet(Packet::EmailCodeRequest(login::EmailCodeRequestPacket { unk1: 0, ..Default::default() }))?;
            user.is_global = true;
            user.connection
                .write_packet(Packet::LoginResponse(login::LoginResponsePacket {
                    player_id: 1,
                    ..Default::default()
                }))?;
        }
        Packet::ServerPong => user.failed_pings -= 1,
        Packet::VitaLogin(_) => {
            user.connection
                .write_packet(Packet::LoginResponse(login::LoginResponsePacket {
                    player_id: 1,
                    ..Default::default()
                }))?;
            // user.connection.write_packet(Packet::SystemMessage(proto::SystemMessagePacket { message: "Hello".to_string(), msg_type: proto::MessageType::AdminMessageInstant, ..Default::default() }))?;
            // user.connection.write_packet(Packet::EmailCodeRequest(login::EmailCodeRequestPacket { unk1: 3, ..Default::default() }))?;
        }
        Packet::SettingsRequest => {
            let settings_file = File::open("settings.txt")?;
            let settings = std::io::read_to_string(settings_file)?;
            user.connection
                .write_packet(Packet::LoadSettings(protocol::LoadSettingsPacket {
                    settings,
                }))?;
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
            let chars = vec![character::Character {
                name: "Test".to_string(),
                player_id: 1,
                character_id: 1,
                ..Default::default()
            }];
            user.connection.write_packet(Packet::CharacterListResponse(
                login::CharacterListPacket {
                    characters: chars,
                    is_global: user.is_global,
                    ..Default::default()
                },
            ))?;
        }
        Packet::CreateCharacter1 => {
            user.connection
                .write_packet(Packet::CreateCharacter1Response(
                    login::CreateCharacter1ResponsePacket {
                        status: 0,
                        unk2: 100,
                        used_smth: 2,
                        req_ac: 300,
                    },
                ))?;
        }
        Packet::CreateCharacter2 => {
            user.connection
                .write_packet(Packet::CreateCharacter2Response(
                    login::CreateCharacter2ResponsePacket { unk: 1 },
                ))?;
        }
        Packet::LoginHistoryRequest => {
            let mut attempts = vec![];
            for _ in 0..30 {
                attempts.push(LoginAttempt {
                    ..Default::default()
                });
            }
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
        // _ => {}
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
                addr.octets()
            } else {
                server.ip
            }
        } else {
            server.ip
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
    //TODO: add a packet for this
    stream.set_nonblocking(true)?;
    let mut stream = tokio::net::TcpStream::from_std(stream)?;
    let mut inner_data = Vec::<u8>::new();
    let mut servers = servers.lock().unwrap();
    let server_count = servers.len() as u32;
    let server = servers
        .get_mut(rand::thread_rng().gen_range(0..server_count) as usize)
        .unwrap();
    inner_data.write_u32::<BigEndian>(0x11_2C_00_00)?;
    inner_data.extend([0; 0x60].iter());
    if server.ip == [0, 0, 0, 0] {
        if let std::net::IpAddr::V4(addr) = stream.local_addr()?.ip() {
            server.ip = addr.octets();
        }
    }
    inner_data.write_all(&server.ip)?;
    inner_data.write_u16::<LittleEndian>(server.port)?;
    inner_data.extend([0; 0x26].iter());
    let mut data = Vec::<u8>::new();
    data.write_u32::<LittleEndian>((4 + inner_data.len()) as u32)?;
    data.append(&mut inner_data);
    write_to_stream(&mut stream, data)?;
    Ok(())
}

fn write_to_stream(stream: &mut tokio::net::TcpStream, mut data: Vec<u8>) -> io::Result<()> {
    loop {
        match stream.try_write(&data) {
            Ok(0) => return Ok(()),
            Ok(x) => {
                if x < data.len() {
                    data.drain(..x).count();
                    continue;
                }
                break;
            }
            Err(e) => {
                if e.kind() != io::ErrorKind::WouldBlock {
                    return Err(e);
                }
                continue;
            }
        }
    }
    Ok(())
}
