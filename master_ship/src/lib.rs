#![deny(clippy::undocumented_unsafe_blocks)]
pub mod sql;
use data_structs::{
    MasterShipAction, MasterShipComm, RegisterShipResult, ShipInfo, UserLoginResult,
};
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
type ASql = Arc<sql::Sql>;

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub db_name: String,
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
            db_name: "sqlite://master_ship.db".into(),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Invalid arguments")]
    InvalidData,
    #[error("Invalid password for user id {0}")]
    InvalidPassword(u32),
    #[error("No user")]
    NoUser,
    #[error("Unable to hash the password")]
    HashError,
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    SqlError(#[from] sqlx::Error),
    #[error(transparent)]
    DataError(#[from] data_structs::Error),
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[error(transparent)]
    TomlSerError(#[from] toml::ser::Error),
    #[error(transparent)]
    TomlDeError(#[from] toml::de::Error),
    #[error(transparent)]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[error(transparent)]
    UTF8Error(#[from] std::str::Utf8Error),
}

static IS_RUNNING: AtomicBool = AtomicBool::new(true);

pub async fn ctrl_c_handler() {
    tokio::signal::ctrl_c().await.expect("failed to listen");
    println!();
    println!("Shutting down...");
    IS_RUNNING.swap(false, std::sync::atomic::Ordering::Relaxed);
}

pub async fn load_hostkey() -> [u8; 32] {
    let mut data = tokio::fs::read("master_key.bin").await.unwrap_or_default();
    data.resize_with(32, || OsRng.next_u32() as u8);
    let _ = tokio::fs::write("master_key.bin", &data).await;
    data.try_into().unwrap()
}

pub async fn ship_receiver(servers: Ships, sql: ASql) -> Result<(), Error> {
    let listener = TcpListener::bind(("0.0.0.0", 15000)).await?;
    let hostkey = load_hostkey().await;
    loop {
        if !IS_RUNNING.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(());
        }
        let result = match tokio::time::timeout(Duration::from_secs(1), listener.accept()).await {
            Ok(x) => x,
            Err(_) => continue,
        };
        match result {
            Ok((s, _)) => {
                let servers = servers.clone();
                let sql = sql.clone();
                tokio::spawn(async move {
                    let mut conn = data_structs::ShipConnection::new_server(s, &hostkey)
                        .await
                        .unwrap();
                    loop {
                        match conn.read_for(Duration::from_secs(1)).await {
                            Ok(d) => match run_action(&servers, &sql, d).await {
                                Ok(a) => match conn.write(a).await {
                                    Ok(_) => {}
                                    Err(e) => {
                                        eprintln!("Write error: {e}");
                                        return;
                                    }
                                },
                                Err(e) => eprintln!("Action error: {e}"),
                            },
                            Err(data_structs::Error::IOError(e))
                                if e.kind() == io::ErrorKind::ConnectionAborted => {}
                            Err(data_structs::Error::Timeout) => {}
                            Err(e) => {
                                eprintln!("Read error: {e}");
                                return;
                            }
                        }
                    }
                });
            }
            Err(e) => Err(e)?,
        }
    }
}

pub async fn run_action(
    ships: &Ships,
    sql: &ASql,
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
                eprintln!("Failed to accept connection: {}", e);
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
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        let _ = send_querry(s, servers.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}

fn send_querry(stream: TcpStream, servers: Ships) -> io::Result<()> {
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
        tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((s, _)) => {
                        let _ = send_block_balance(s, server_statuses.clone());
                    }
                    Err(e) => {
                        eprintln!("Failed to accept connection: {}", e);
                        return;
                    }
                }
            }
        });
    }
    Ok(())
}

pub fn send_block_balance(stream: TcpStream, servers: Ships) -> io::Result<()> {
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

async fn async_write<T>(mutex: &RwLock<T>) -> RwLockWriteGuard<T> {
    loop {
        match mutex.try_write() {
            Some(lock) => return lock,
            None => tokio::time::sleep(Duration::from_millis(1)).await,
        }
    }
}
