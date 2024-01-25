#![deny(clippy::undocumented_unsafe_blocks)]
#![warn(clippy::future_not_send)]
pub mod sql;
use data_structs::master_ship::{
    MasterShipAction, MasterShipComm, RegisterShipResult, ShipConnection, ShipInfo,
    ShipLoginResult, UserLoginResult,
};
use p256::ecdsa::SigningKey;
use parking_lot::{RwLock, RwLockWriteGuard};
use pso2packetlib::{
    protocol::{login, Packet, PacketType},
    Connection, PrivateKey, PublicKey,
};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    net::Ipv4Addr,
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};

type Ships = Arc<RwLock<Vec<ShipInfo>>>;
type Sql = Arc<sql::Sql>;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub db_name: String,
    pub registration_enabled: bool,
    pub log_dir: String,
    pub file_log_level: log::LevelFilter,
    pub console_log_level: log::LevelFilter,
}

#[derive(Serialize, Deserialize)]
pub struct Keys {
    pub ip: Ipv4Addr,
    pub key: Vec<u8>,
}

impl Settings {
    pub async fn load(path: &str) -> Result<Settings, Error> {
        let string = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(_) => {
                let settings = Settings::default();
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
            db_name: "master_ship.db".into(),
            registration_enabled: false,
            log_dir: String::from("logs"),
            file_log_level: log::LevelFilter::Info,
            console_log_level: log::LevelFilter::Debug,
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
}

static IS_RUNNING: AtomicBool = AtomicBool::new(true);

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
    let sql = Arc::new(sql::Sql::new(&settings.db_name, settings.registration_enabled).await?);
    let servers = Arc::new(RwLock::new(vec![]));
    tokio::spawn(make_keys(servers.clone()));
    make_query(servers.clone()).await?;
    make_block_balance(servers.clone()).await?;
    ship_receiver(servers, sql).await?;
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

pub async fn ship_receiver(servers: Ships, sql: Sql) -> Result<(), Error> {
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
        let servers = servers.clone();
        let sql = sql.clone();
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
            connection_handler(conn, servers, sql).await
        });
    }
}

async fn connection_handler(mut conn: ShipConnection, servers: Ships, sql: Sql) {
    match ship_login(&mut conn, &sql).await {
        Ok(_) => {}
        Err(e) => {
            log::warn!("Login error: {e}");
            return;
        }
    };
    loop {
        match conn.read_for(Duration::from_secs(1)).await {
            Ok(d) => match run_action(&servers, &sql, d).await {
                Ok(a) => match conn.write(a).await {
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Write error: {e}");
                        return;
                    }
                },
                Err(e) => log::warn!("Action error: {e}"),
            },
            Err(data_structs::Error::IOError(e))
                if e.kind() == io::ErrorKind::ConnectionAborted =>
            {
                log::info!("Ship disconnected");
                return;
            }
            Err(data_structs::Error::Timeout) => {}
            Err(e) => {
                log::warn!("Read error: {e}");
                return;
            }
        }
    }
}

async fn ship_login(conn: &mut ShipConnection, sql: &Sql) -> Result<(), Error> {
    let action = conn.read_for(Duration::from_secs(10)).await?;
    let mut response = MasterShipComm {
        id: action.id,
        action: MasterShipAction::Ok,
    };
    let MasterShipAction::ShipLogin { psk } = action.action else {
        response.action = MasterShipAction::Error(String::from("Invalid action"));
        conn.write(response).await?;
        return Err(Error::InvalidAction);
    };

    match sql.get_ship_data(&psk).await? {
        true => {}
        false => {
            if !sql.registration_enabled() {
                response.action = MasterShipAction::ShipLoginResult(ShipLoginResult::UnknownShip);
                conn.write(response).await?;
                return Err(Error::UnknownShip);
            }
            sql.put_ship_data(&psk).await?;
        }
    };

    response.action = MasterShipAction::ShipLoginResult(ShipLoginResult::Ok);
    conn.write(response).await?;

    Ok(())
}

pub async fn run_action(
    ships: &Ships,
    sql: &Sql,
    action: MasterShipComm,
) -> Result<MasterShipComm, Error> {
    let mut response = MasterShipComm {
        id: action.id,
        action: MasterShipAction::Ok,
    };
    match action.action {
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
    }
    Ok(response)
}

pub async fn make_keys(servers: Ships) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", 11000)).await?;
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_keys(s, servers.clone());
            }
            Err(e) => {
                log::error!("Failed to accept key connection: {e}");
                return Err(e);
            }
        }
    }
}

pub async fn make_query(servers: Ships) -> io::Result<()> {
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

async fn query_listener(listener: TcpListener, servers: Ships) {
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_query(s, servers.clone());
            }
            Err(e) => {
                log::error!("Failed to accept query connection: {e}");
                return;
            }
        }
    }
}

fn send_query(stream: TcpStream, servers: Ships) -> io::Result<()> {
    log::debug!("Sending query information...");
    stream.set_nodelay(true)?;
    let mut con = Connection::new(
        stream.into_std()?,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let mut ships = vec![];
    for server in servers.read().iter() {
        ships.push(login::ShipEntry {
            id: server.id * 1000,
            name: format!("Ship{:02}", server.id),
            ip: server.ip,
            status: server.status,
            order: server.id as u16,
        })
    }
    con.write_packet(&Packet::ShipList(login::ShipListPacket {
        ships,
        ..Default::default()
    }))?;
    Ok(())
}

pub async fn make_block_balance(server_statuses: Ships) -> io::Result<()> {
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

async fn block_listener(listener: TcpListener, server_statuses: Ships) {
    loop {
        match listener.accept().await {
            Ok((s, _)) => {
                let _ = send_block_balance(s, server_statuses.clone());
            }
            Err(e) => {
                log::error!("Failed to accept connection: {e}");
                return;
            }
        }
    }
}

pub fn send_block_balance(stream: TcpStream, servers: Ships) -> io::Result<()> {
    log::debug!("Sending block balance...");
    stream.set_nodelay(true)?;
    let port = stream.local_addr()?.port();
    let id = if port % 3 == 0 {
        (port - 12093) / 100
    } else {
        (port - 12000) / 100
    } as u32;
    let mut con = Connection::new(
        stream.into_std()?,
        PacketType::Classic,
        PrivateKey::None,
        PublicKey::None,
    );
    let servers = servers.read();
    let Some(server) = servers.iter().find(|x| x.id == id) else {
        con.write_packet(&Packet::LoginResponse(login::LoginResponsePacket {
            status: login::LoginStatus::Failure,
            error: "Server is offline".to_string(),
            ..Default::default()
        }))?;
        return Ok(());
    };

    let packet = login::BlockBalancePacket {
        ip: server.ip,
        port: server.port,
        blockname: server.name.clone(),
        ..Default::default()
    };
    con.write_packet(&Packet::BlockBalance(packet))?;
    Ok(())
}

pub fn send_keys(stream: TcpStream, servers: Ships) -> Result<(), Error> {
    log::debug!("Sending keys...");
    let mut stream = stream.into_std()?;
    stream.set_nodelay(true)?;
    let lock = servers.read();
    let mut data = vec![];
    for ship in lock.iter() {
        let mut key = vec![0x06, 0x02, 0x00, 0x00, 0x00, 0xA4, 0x00, 0x00];
        key.append(&mut b"RSA1".to_vec());
        key.append(&mut (ship.key.n.len() as u32 * 8).to_le_bytes().to_vec());
        let mut e = ship.key.e.to_vec();
        e.resize(4, 0);
        key.append(&mut e);
        key.append(&mut ship.key.n.to_vec());
        data.push(Keys { ip: ship.ip, key })
    }
    let mut data = rmp_serde::to_vec(&data)?;
    let mut out_data = Vec::with_capacity(data.len());
    out_data.append(&mut (data.len() as u32).to_le_bytes().to_vec());
    out_data.append(&mut data);
    stream.write_all(&out_data)?;
    Ok(())
}

async fn async_write<T>(mutex: &RwLock<T>) -> RwLockWriteGuard<T>
where
    T: Send + Sync,
{
    loop {
        match mutex.try_write() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}
