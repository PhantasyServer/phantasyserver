#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::future_not_send)]
#![allow(clippy::await_holding_lock)]
pub mod sql;
use clap::Parser;
use data_structs::{
    SerDeFile, ServerData,
    master_ship::{
        MasterShipAction, MasterShipComm, RegisterShipResult, ServerDataResult, SetNicknameResult,
        ShipConnection, ShipInfo, ShipLoginResult, UserLoginResult, start_discovery_loop,
    },
};
use network_interface::{NetworkInterface, NetworkInterfaceConfig};
use p256::ecdsa::SigningKey;
use parking_lot::{RwLock, RwLockWriteGuard};
use pso2packetlib::{
    Connection, PrivateKey, PublicKey,
    protocol::{Packet, PacketType, login},
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    io,
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    db_name: String,
    registration_enabled: bool,
    log_dir: String,
    file_log_level: log::LevelFilter,
    console_log_level: log::LevelFilter,
    data_path: Option<String>,
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Location of the settings file
    #[arg(short, long)]
    settings_file: Option<String>,
    /// Don't create settings file if it doesn't exist
    #[arg(long, default_value_t = false)]
    dont_create_settings: bool,
    /// Path to the DB file
    #[arg(short('D'), long)]
    db_path: Option<String>,
    /// If specified then auto registration will be enabled
    #[arg(short, long)]
    registration_enabled: Option<bool>,
    /// Location of the logs directory
    #[arg(short, long)]
    log_dir: Option<String>,
    /// Log level of log files
    #[arg(short, long)]
    file_log_level: Option<log::LevelFilter>,
    /// Log level of console
    #[arg(short, long)]
    console_log_level: Option<log::LevelFilter>,
    /// Location of complied server data file
    #[arg(short, long)]
    data_path: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Keys {
    ip: Ipv4Addr,
    key: Vec<u8>,
}

struct MSData {
    ships: RwLock<Vec<ShipInfo>>,
    sql: sql::Sql,
    srv_data: Option<ServerData>,
}

struct Ship {
    conn: ShipConnection,
    ms_data: Arc<MSData>,
    authed: bool,
    pings: u8,
}

macro_rules! args_to_settings {
    ($arg:expr => $set:expr) => {
        if let Some(x) = $arg {
            $set = x;
        }
    };
}

impl Settings {
    pub async fn load(path: &str) -> Result<Settings, Error> {
        let args = Args::parse();
        let path = if let Some(path) = &args.settings_file {
            path
        } else {
            path
        };
        let mut settings = match tokio::fs::read_to_string(path).await {
            Ok(s) => toml::from_str(&s)?,
            Err(_) => {
                let settings = Settings::default();
                if args.dont_create_settings {
                    tokio::fs::write(path, toml::to_string_pretty(&settings)?).await?;
                }
                settings
            }
        };
        args_to_settings!(args.db_path => settings.db_name);
        args_to_settings!(args.registration_enabled => settings.registration_enabled);
        args_to_settings!(args.log_dir => settings.log_dir);
        args_to_settings!(args.file_log_level => settings.file_log_level);
        args_to_settings!(args.console_log_level => settings.console_log_level);
        settings.data_path = args.data_path.or(settings.data_path);
        Ok(settings)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            db_name: String::from("master_ship.db"),
            registration_enabled: false,
            log_dir: String::from("logs"),
            file_log_level: log::LevelFilter::Info,
            console_log_level: log::LevelFilter::Debug,
            data_path: None,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid arguments")]
    InvalidData,
    #[error("Invalid action")]
    InvalidAction,
    #[error("Unknown ship")]
    UnknownShip,
    #[error("Invalid password for user id {0}")]
    InvalidPassword(u32),
    #[error("No user")]
    NoUser,
    #[error("Unable to hash the password")]
    HashError,
    #[error("Failed to get network interfaces: {0}")]
    NetworkInterfacesError(#[from] network_interface::Error),

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("SQL error: {0}")]
    SqlError(#[from] sqlx::Error),
    #[error(transparent)]
    DataError(#[from] data_structs::Error),
    #[error("TOML Serialization error: {0}")]
    TomlSerError(#[from] toml::ser::Error),
    #[error("TOML Deserialization error: {0}")]
    TomlDeError(#[from] toml::de::Error),
    #[error("MP Serialization error: {0}")]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[error("MP Deserialization error: {0}")]
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[error("UTF-8 error: {0}")]
    UTF8Error(#[from] std::str::Utf8Error),
    #[error("Client connection error: {0}")]
    ConnError(#[from] pso2packetlib::connection::ConnectionError),
}

enum AddrType {
    Loopback,
    Local,
    Global,
}

static IS_RUNNING: AtomicBool = AtomicBool::new(true);

async fn load_data(path: &str) -> Result<ServerData, Error> {
    Ok(ServerData::load_from_mp_comp(path)?)
}

pub async fn run() -> Result<(), Error> {
    let settings = Settings::load("master_ship.toml").await?;
    // setup logging
    {
        use simplelog::*;
        let _ = std::fs::create_dir_all(&settings.log_dir);
        let mut path = std::path::PathBuf::from(&settings.log_dir);
        path.push(format!(
            "master_ship_{}.log",
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
    log::info!("Starting master ship...");
    tokio::spawn(ctrl_c_handler());
    let sql = sql::Sql::new(&settings.db_name, settings.registration_enabled).await?;
    let servers = RwLock::new(vec![]);
    let server_data = if let Some(path) = settings.data_path {
        match load_data(&path).await {
            Ok(d) => Some(d),
            Err(e) => {
                log::warn!("Failed to load server data: {e}");
                None
            }
        }
    } else {
        None
    };
    let ms_data = Arc::new(MSData {
        sql,
        ships: servers,
        srv_data: server_data,
    });
    start_discovery_loop(15000).await?;
    tokio::spawn(make_keys(ms_data.clone()));
    make_query(ms_data.clone()).await?;
    make_block_balance(ms_data.clone()).await?;
    ship_receiver(ms_data).await?;

    Ok(())
}

pub async fn ctrl_c_handler() {
    tokio::signal::ctrl_c().await.expect("failed to listen");
    log::info!("Shutting down...");
    IS_RUNNING.swap(false, std::sync::atomic::Ordering::Relaxed);
}

pub async fn load_key() -> SigningKey {
    let mut data = tokio::fs::read("master_key.bin").await.unwrap_or_default();
    data.resize_with(32, || OsRng.next_u32() as u8);
    let _ = tokio::fs::write("master_key.bin", &data).await;
    SigningKey::from_slice(&data).unwrap()
}

async fn ship_receiver(ms_data: Arc<MSData>) -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", 15000)).await?;
    log::info!("Loading signing key...");
    let signing_key = load_key().await;
    // this is 65 bytes
    let hostkey = signing_key.verifying_key().to_sec1_bytes().to_vec();
    log::info!("Started master server");
    loop {
        if !IS_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let Ok(result) = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await
        else {
            continue;
        };
        let (socket, _) = result?;
        log::info!("New connection");
        let ms_data = ms_data.clone();
        let signing_key = signing_key.clone();
        let hostkey = hostkey.clone();
        tokio::spawn(async move {
            let conn = match ShipConnection::new_server(socket, &signing_key, &hostkey).await {
                Ok(c) => c,
                Err(e) => {
                    log::warn!("Failed to setup ship connection: {e}");
                    return;
                }
            };
            connection_handler(conn, ms_data).await
        });
    }
}

async fn connection_handler(conn: ShipConnection, ms_data: Arc<MSData>) {
    let mut ship = Ship {
        conn,
        ms_data,
        authed: false,
        pings: 0,
    };
    loop {
        match ship.conn.read_for(Duration::from_secs(5 * 60)).await {
            Ok(d) => match run_action(&mut ship, d).await {
                Ok(a) => match ship.conn.write(a).await {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Write error: {e}");
                        break;
                    }
                },
                Err(e) => log::warn!("Action error: {e}"),
            },
            Err(data_structs::Error::IOError(e))
                if e.kind() == io::ErrorKind::ConnectionAborted =>
            {
                log::info!("Ship disconnected");
                break;
            }
            Err(data_structs::Error::Timeout) => {
                if ship.pings > 5 {
                    break;
                }
                ship.pings += 1;
                let _ = ship
                    .conn
                    .write(MasterShipComm {
                        id: 0,
                        action: MasterShipAction::Ping,
                    })
                    .await;
            }
            Err(e) => {
                log::warn!("Read error: {e}");
                return;
            }
        }
    }
    let Ok(ip) = ship.conn.get_ip() else { return };
    let IpAddr::V4(ip) = ip else { return };
    let mut lock = async_write(&ship.ms_data.ships).await;
    if let Some((i, _)) = lock.iter().enumerate().find(|(_, s)| s.ip == ip) {
        lock.swap_remove(i);
    }
}

async fn run_action(ship: &mut Ship, action: MasterShipComm) -> Result<MasterShipComm, Error> {
    let mut response = MasterShipComm {
        id: action.id,
        action: MasterShipAction::Ok,
    };
    let sql = &ship.ms_data.sql;
    let ships = &ship.ms_data.ships;
    ship.pings = 0;
    match action.action {
        MasterShipAction::ShipLogin(psk) if !ship.authed => {
            let psk = psk.psk;
            match sql.get_ship_data(&psk).await? {
                true => {
                    ship.authed = true;
                }
                false => {
                    if !sql.registration_enabled() {
                        response.action =
                            MasterShipAction::ShipLoginResult(ShipLoginResult::UnknownShip);
                        return Ok(response);
                    }
                    sql.put_ship_data(&psk).await?;
                    ship.authed = true;
                }
            };

            response.action = MasterShipAction::ShipLoginResult(ShipLoginResult::Ok);
        }
        MasterShipAction::Ping => {}
        _ if !ship.authed => {
            response.action = MasterShipAction::Error(String::from("Unauthenticated"));
        }
        MasterShipAction::RegisterShip(ship) => {
            let mut lock = async_write(ships).await;
            for known_ship in lock.iter() {
                if known_ship.id == ship.id {
                    response.action =
                        MasterShipAction::RegisterShipResult(RegisterShipResult::AlreadyTaken);
                    return Ok(response);
                }
            }
            lock.push(ship);
            response.action = MasterShipAction::RegisterShipResult(RegisterShipResult::Success);
        }
        MasterShipAction::RegisterShipResult(_) => {}
        MasterShipAction::UnregisterShip(id) => {
            let mut lock = async_write(ships).await;
            if let Some(pos) = lock.iter().enumerate().find(|x| x.1.id == id).map(|x| x.0) {
                lock.swap_remove(pos);
            }
        }
        MasterShipAction::Ok => {}
        MasterShipAction::Error(_) => {}
        MasterShipAction::UserLogin(data) => {
            match sql
                .get_sega_user(&data.username, &data.password, data.ip)
                .await
            {
                Ok(d) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::Success {
                        id: d.id,
                        nickname: d.nickname,
                        accountflags: d.account_flags,
                        isgm: d.isgm,
                        last_uuid: d.last_uuid,
                    })
                }
                Err(ref e) if matches!(e, Error::NoUser) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::NotFound)
                }
                Err(Error::InvalidPassword(id)) => {
                    response.action =
                        MasterShipAction::UserLoginResult(UserLoginResult::InvalidPassword(id))
                }
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::UserRegister(data) => {
            match sql.create_sega_user(&data.username, &data.password).await {
                Ok(d) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::Success {
                        id: d.id,
                        nickname: d.nickname,
                        accountflags: d.account_flags,
                        isgm: d.isgm,
                        last_uuid: d.last_uuid,
                    })
                }
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::UserLoginVita(data) => {
            match sql.get_psn_user(&data.username, data.ip).await {
                Ok(d) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::Success {
                        id: d.id,
                        nickname: d.nickname,
                        accountflags: d.account_flags,
                        isgm: d.isgm,
                        last_uuid: d.last_uuid,
                    })
                }
                Err(ref e) if matches!(e, Error::NoUser) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::NotFound)
                }
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::UserRegisterVita(data) => {
            match sql.create_psn_user(&data.username).await {
                Ok(d) => {
                    response.action = MasterShipAction::UserLoginResult(UserLoginResult::Success {
                        id: d.id,
                        nickname: d.nickname,
                        accountflags: d.account_flags,
                        isgm: d.isgm,
                        last_uuid: d.last_uuid,
                    })
                }
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::UserLoginResult(_) => {}
        MasterShipAction::GetStorage(player_id) => match sql.get_account_storage(player_id).await {
            Ok(d) => response.action = MasterShipAction::GetStorageResult(d),
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::GetStorageResult(_) => {}
        MasterShipAction::PutStorage { id, storage } => {
            match sql.put_account_storage(id, storage).await {
                Ok(_) => {}
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::GetLogins(id) => match sql.get_logins(id).await {
            Ok(d) => response.action = MasterShipAction::GetLoginsResult(d),
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::GetLoginsResult(_) => {}
        MasterShipAction::GetSettings(id) => match sql.get_settings(id).await {
            Ok(d) => response.action = MasterShipAction::GetSettingsResult(d),
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::GetSettingsResult(_) => {}
        MasterShipAction::PutSettings { id, settings } => {
            match sql.save_settings(id, &settings).await {
                Ok(_) => response.action = MasterShipAction::Ok,
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::NewBlockChallenge(id) => match sql.new_challenge(id).await {
            Ok(challenge) => response.action = MasterShipAction::BlockChallengeResult(challenge),
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::BlockChallengeResult(_) => {}
        MasterShipAction::ChallengeLogin {
            challenge,
            player_id,
        } => match sql.login_challenge(player_id, challenge).await {
            Ok(d) => {
                response.action = MasterShipAction::UserLoginResult(UserLoginResult::Success {
                    id: d.id,
                    nickname: d.nickname,
                    accountflags: d.account_flags,
                    isgm: d.isgm,
                    last_uuid: d.last_uuid,
                })
            }
            Err(ref e) if matches!(e, Error::NoUser) => {
                response.action = MasterShipAction::UserLoginResult(UserLoginResult::NotFound)
            }
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::GetUserInfo(id) => match sql.get_user_info(id).await {
            Ok(d) => response.action = MasterShipAction::UserInfo(d),
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::UserInfo(_) => {}
        MasterShipAction::PutUserInfo { id, info } => match sql.put_user_info(id, info).await {
            Ok(_) => response.action = MasterShipAction::Ok,
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::PutAccountFlags { id, flags } => {
            match sql.put_account_flags(id, flags).await {
                Ok(_) => response.action = MasterShipAction::Ok,
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::PutUUID { id, uuid } => match sql.put_uuid(id, uuid).await {
            Ok(_) => response.action = MasterShipAction::Ok,
            Err(e) => response.action = MasterShipAction::Error(e.to_string()),
        },
        MasterShipAction::ShipLogin { .. } => {
            response.action = MasterShipAction::Error(Error::InvalidAction.to_string())
        }
        MasterShipAction::ShipLoginResult(_) => {
            response.action = MasterShipAction::Error(Error::InvalidAction.to_string())
        }
        MasterShipAction::SetNickname { id, nickname } => {
            match sql.set_nickname(id, &nickname).await {
                Ok(true) => {
                    response.action = MasterShipAction::SetNicknameResult(SetNicknameResult::Ok)
                }
                Ok(false) => {
                    response.action =
                        MasterShipAction::SetNicknameResult(SetNicknameResult::AlreadyTaken)
                }
                Err(e) => response.action = MasterShipAction::Error(e.to_string()),
            }
        }
        MasterShipAction::SetNicknameResult(_) => {}
        MasterShipAction::SetFormat(fmt) => {
            ship.conn.set_deferred_fmt(fmt);
            response.action = MasterShipAction::Ok;
        }
        MasterShipAction::ServerDataRequest => {
            if let Some(data) = ship.ms_data.srv_data.as_ref() {
                response.action = MasterShipAction::ServerDataResponse(ServerDataResult::Ok(
                    Box::new(data.clone()),
                ));
            } else {
                response.action =
                    MasterShipAction::ServerDataResponse(ServerDataResult::NotAvailable);
            }
        }
        MasterShipAction::ServerDataResponse(_) => {}
        MasterShipAction::Pong => {}
    }
    Ok(response)
}

async fn make_keys(servers: Arc<MSData>) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", 11000)).await?;
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_keys(s, servers.clone()).await;
            }
            Err(e) => {
                log::error!("Failed to accept key connection: {e}");
                return Err(e);
            }
        }
    }
}

async fn make_query(servers: Arc<MSData>) -> io::Result<()> {
    let mut info_listeners: Vec<TcpListener> = vec![];
    for i in 0..10 {
        // pc ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12199 + (i * 100))).await?);
        // vita ships
        info_listeners.push(TcpListener::bind(("0.0.0.0", 12194 + (i * 100))).await?);
    }
    for listener in info_listeners {
        let servers = servers.clone();
        tokio::spawn(query_listener(listener, servers));
    }
    Ok(())
}

async fn query_listener(listener: TcpListener, servers: Arc<MSData>) {
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_query(s, servers.clone()).await;
            }
            Err(e) => {
                log::error!("Failed to accept query connection: {e}");
                return;
            }
        }
    }
}

async fn send_query(stream: TcpStream, servers: Arc<MSData>) -> Result<(), Error> {
    log::debug!("Sending query information...");
    stream.set_nodelay(true)?;
    let mut con = Connection::<Packet>::new_async(
        stream,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let mut ships = vec![];
    for server in servers.ships.read().iter() {
        ships.push(login::ShipEntry {
            id: server.id * 1000,
            name: format!("Ship{:02}", server.id).into(),
            ip: server.ip,
            status: server.status,
            order: server.id as u16,
        })
    }
    con.write_packet_async(&Packet::ShipList(login::ShipListPacket {
        ships,
        ..Default::default()
    }))
    .await?;
    Ok(())
}

async fn make_block_balance(server_statuses: Arc<MSData>) -> Result<(), Error> {
    let mut listeners = vec![];
    for i in 0..10 {
        //pc balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12100 + (i * 100))).await?);
        //vita balance
        listeners.push(TcpListener::bind(("0.0.0.0", 12193 + (i * 100))).await?);
    }
    for listener in listeners {
        let server_statuses = server_statuses.clone();
        tokio::spawn(block_listener(listener, server_statuses));
    }
    Ok(())
}

async fn block_listener(listener: TcpListener, server_statuses: Arc<MSData>) {
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_block_balance(s, server_statuses.clone()).await;
            }
            Err(e) => {
                log::error!("Failed to accept connection: {e}");
                return;
            }
        }
    }
}

async fn send_block_balance(stream: TcpStream, servers: Arc<MSData>) -> Result<(), Error> {
    log::debug!("Sending block balance...");
    stream.set_nodelay(true)?;
    let port = stream.local_addr()?.port();
    let id = if port % 3 == 0 {
        (port - 12093) / 100
    } else {
        (port - 12000) / 100
    } as u32;
    let remote_ip = match stream.peer_addr()?.ip() {
        IpAddr::V4(ipv4_addr) => ipv4_addr,
        IpAddr::V6(_) => return Err(Error::InvalidData),
    };
    let local_ip = match stream.local_addr()?.ip() {
        IpAddr::V4(ipv4_addr) => ipv4_addr,
        IpAddr::V6(_) => return Err(Error::InvalidData),
    };
    let mut con = Connection::<Packet>::new_async(
        stream,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let servers = servers.ships.read();
    let Some(server) = servers.iter().find(|x| x.id == id) else {
        con.write_packet_async(&Packet::LoginResponse(login::LoginResponsePacket {
            status: login::LoginStatus::Failure,
            error: "Server is offline".to_string(),
            ..Default::default()
        }))
        .await?;
        return Ok(());
    };

    let ship_ip = server.ip;
    let send_ip = get_addr(remote_ip, local_ip, ship_ip)?;

    let packet = login::BlockBalancePacket {
        ip: send_ip,
        port: server.port,
        blockname: server.name.clone().into(),
        ..Default::default()
    };
    con.write_packet_async(&Packet::BlockBalance(packet))
        .await?;
    Ok(())
}

async fn send_keys(mut stream: TcpStream, servers: Arc<MSData>) -> Result<(), Error> {
    log::debug!("Sending keys...");
    stream.set_nodelay(true)?;
    let remote_ip = match stream.peer_addr()?.ip() {
        IpAddr::V4(ipv4_addr) => ipv4_addr,
        IpAddr::V6(_) => return Err(Error::InvalidData),
    };
    let local_ip = match stream.local_addr()?.ip() {
        IpAddr::V4(ipv4_addr) => ipv4_addr,
        IpAddr::V6(_) => return Err(Error::InvalidData),
    };
    let lock = servers.ships.read();
    let mut data = vec![];
    for ship in lock.iter() {
        let mut key = vec![0x06, 0x02, 0x00, 0x00, 0x00, 0xA4, 0x00, 0x00];
        key.append(&mut b"RSA1".to_vec());
        key.append(&mut (ship.key.n.len() as u32 * 8).to_le_bytes().to_vec());
        let mut e = ship.key.e.to_vec();
        e.resize(4, 0);
        key.append(&mut e);
        key.append(&mut ship.key.n.to_vec());
        let send_ip = get_addr(remote_ip, local_ip, ship.ip)?;
        data.push(Keys { ip: send_ip, key })
    }
    let mut data = rmp_serde::to_vec(&data)?;
    let mut out_data = Vec::with_capacity(data.len());
    out_data.append(&mut (data.len() as u32).to_le_bytes().to_vec());
    out_data.append(&mut data);
    stream.write_all(&out_data).await?;
    Ok(())
}

async fn async_write<T>(mutex: &RwLock<T>) -> RwLockWriteGuard<'_, T>
where
    T: Send + Sync,
{
    loop {
        match mutex.try_write() {
            Some(lock) => return lock,
            None => tokio::task::yield_now().await,
        }
    }
}

fn get_addr_type(chk_addr: Ipv4Addr) -> Result<AddrType, Error> {
    if chk_addr.is_loopback() {
        return Ok(AddrType::Loopback);
    }
    let interfaces = NetworkInterface::show()?;
    for addr in interfaces.into_iter().flat_map(|i| i.addr.into_iter()) {
        let IpAddr::V4(local_addr) = addr.ip() else {
            continue;
        };
        let Some(IpAddr::V4(mask)) = addr.netmask() else {
            continue;
        };
        let local_addr = local_addr.octets();
        let mask = mask.octets();
        let chk_addr = chk_addr.octets();
        let mut masked_local_addr = [0; 4];
        let mut masked_chk_addr = [0; 4];
        masked_local_addr
            .iter_mut()
            .zip(local_addr.iter().zip(mask.iter()).map(|(a, b)| a & b))
            .for_each(|(a, b)| *a = b);
        masked_chk_addr
            .iter_mut()
            .zip(chk_addr.iter().zip(mask.iter()).map(|(a, b)| a & b))
            .for_each(|(a, b)| *a = b);
        if masked_chk_addr == masked_local_addr {
            return Ok(AddrType::Local);
        }
    }
    Ok(AddrType::Global)
}

fn get_addr(remote_ip: Ipv4Addr, local_ip: Ipv4Addr, ship_ip: Ipv4Addr) -> Result<Ipv4Addr, Error> {
    let remote_addr_type = get_addr_type(remote_ip)?;
    let ship_addr_type = get_addr_type(ship_ip)?;
    let send_ip = match (remote_addr_type, ship_addr_type) {
        // if the ship is connected via global ip then it is reachable by anyone
        (_, AddrType::Global) => ship_ip,
        // if the client is connected via loopback then it is the same pc, thus any ship ip works
        (AddrType::Loopback, _) => ship_ip,
        // ship is on the same pc as the master ship, so return connected to address
        (AddrType::Local, AddrType::Loopback) => local_ip,
        // assume that the ship is in the same network as the client
        (AddrType::Local, AddrType::Local) => ship_ip,
        // if the ship is local or on the same pc
        (AddrType::Global, _) => local_ip,
    };

    Ok(send_ip)
}
