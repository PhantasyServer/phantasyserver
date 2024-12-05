use crate::{flags::Flags, inventory::AccountStorages, Error};
use aes_gcm::{
    aead::{Aead, KeyInit},
    AeadCore, Aes256Gcm,
};
use p256::{
    ecdh::EphemeralSecret,
    ecdsa::{
        signature::{Signer, Verifier},
        Signature, SigningKey, VerifyingKey,
    },
    PublicKey,
};
use pso2packetlib::{
    protocol::login::{LoginAttempt, ShipStatus, UserInfoPacket},
    AsciiString,
};
use rand_core::OsRng;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MasterShipComm {
    pub id: u32,
    pub action: MasterShipAction,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MasterShipAction {
    /// (S->MS) Ship wants to login.
    ShipLogin(ShipLogin),
    /// (MS->S) Login result.
    ShipLoginResult(ShipLoginResult),
    /// New ship wants to connect
    RegisterShip(ShipInfo),
    RegisterShipResult(RegisterShipResult),
    UserLogin(UserCreds),
    UserRegister(UserCreds),
    UserLoginVita(UserCreds),
    UserRegisterVita(UserCreds),
    UserLoginResult(UserLoginResult),
    SetNickname {
        id: u32,
        nickname: String,
    },
    SetNicknameResult(SetNicknameResult),
    GetUserInfo(u32),
    UserInfo(UserInfoPacket),
    PutUserInfo {
        id: u32,
        info: UserInfoPacket,
    },
    PutAccountFlags {
        id: u32,
        flags: Flags,
    },
    PutUUID {
        id: u32,
        uuid: u64,
    },
    /// Create a new block login challenge. Parameter is the player id
    NewBlockChallenge(u32),
    /// Result of a new block login challenge request.
    /// Parameter is the challenge
    BlockChallengeResult(u32),
    ChallengeLogin {
        challenge: u32,
        player_id: u32,
    },
    GetStorage(u32),
    GetStorageResult(AccountStorages),
    PutStorage {
        id: u32,
        storage: AccountStorages,
    },
    GetLogins(u32),
    GetLoginsResult(Vec<LoginAttempt>),
    GetSettings(u32),
    GetSettingsResult(AsciiString),
    PutSettings {
        id: u32,
        settings: AsciiString,
    },
    /// Delete ship from the list. Parameter is the id of the ship
    UnregisterShip(u32),
    SetFormat(SerializerFormat),
    Ok,
    /// Error has occured
    Error(String),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ShipLogin {
    pub psk: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum ShipLoginResult {
    Ok,
    UnknownShip,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SetNicknameResult {
    Ok,
    AlreadyTaken,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ShipInfo {
    pub ip: Ipv4Addr,
    pub port: u16,
    pub id: u32,
    pub max_players: u32,
    pub name: String,
    pub status: ShipStatus,
    pub key: KeyInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KeyInfo {
    /// Modulus 'n' in little endian form
    pub n: Vec<u8>,
    /// Public exponent 'e' in little endian form
    pub e: Vec<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum UserLoginResult {
    Success {
        id: u32,
        nickname: String,
        accountflags: Flags,
        isgm: bool,
        last_uuid: u64,
    },
    InvalidPassword(u32),
    NotFound,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum RegisterShipResult {
    Success,
    AlreadyTaken,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct UserCreds {
    pub username: String,
    pub password: String,
    pub ip: Ipv4Addr,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum SerializerFormat {
    Json,
    MessagePack,
    MessagePackUnnamed,
    Bincode,
}

#[cfg(feature = "ship")]
pub struct ShipConnection {
    stream: tokio::net::TcpStream,
    raw_read_buffer: Vec<u8>,
    length: u32,
    aes: Aes256Gcm,
    format: SerializerFormat,
    deferred_fmt: Option<SerializerFormat>,
}

#[cfg(feature = "ship")]
impl ShipConnection {
    // this should prevent mitm attacks, but i'm not 100% sure
    // also this is derived from ssh handshake
    pub async fn new_server(
        mut stream: tokio::net::TcpStream,
        priv_key: &SigningKey,
        hostkey: &[u8],
    ) -> Result<Self, Error> {
        //send hostkey
        stream
            .write_all(&(hostkey.len() as u32).to_le_bytes())
            .await?;
        stream.write_all(hostkey).await?;

        let shared_secret = ShipConnection::key_exchange(&mut stream).await?;
        let hash = {
            use sha2::Digest;
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            hasher.update(shared_secret);
            hasher.update(hostkey);
            hasher.finalize()
        };

        let signature = Signer::<Signature>::sign(priv_key, &hash).to_vec();
        stream
            .write_all(&(signature.len() as u32).to_le_bytes())
            .await?;
        stream.write_all(&signature).await?;

        Ok(Self {
            stream,
            raw_read_buffer: vec![],
            length: 0,
            aes: Aes256Gcm::new(&shared_secret.into()),
            format: SerializerFormat::Json,
            deferred_fmt: None,
        })
    }
    pub async fn new_client<F>(mut stream: tokio::net::TcpStream, check: F) -> Result<Self, Error>
    where
        F: FnOnce(Ipv4Addr, &[u8]) -> bool + Send,
    {
        //receive hostkey
        let mut len_buf = [0; 4];

        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len_buf))
            .await
            .map_err(|_| Error::Timeout)??;
        let IpAddr::V4(ip) = stream.peer_addr()?.ip() else {
            unreachable!()
        };

        let key_len = u32::from_le_bytes(len_buf) as usize;
        let mut hostkey = vec![0; key_len];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut hostkey))
            .await
            .map_err(|_| Error::Timeout)??;
        if !check(ip, &hostkey) {
            return Err(Error::UnknownHostkey(hostkey));
        }

        let shared_secret = ShipConnection::key_exchange(&mut stream).await?;
        let hash = {
            use sha2::Digest;
            let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
            hasher.update(shared_secret);
            hasher.update(&hostkey);
            hasher.finalize()
        };

        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut len_buf))
            .await
            .map_err(|_| Error::Timeout)??;
        let signature_len = u32::from_le_bytes(len_buf) as usize;
        let mut signature = vec![0; signature_len];
        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut signature))
            .await
            .map_err(|_| Error::Timeout)??;

        let signature = Signature::from_slice(&signature)?;
        let verifying_key = VerifyingKey::from_sec1_bytes(&hostkey)?;
        verifying_key.verify(&hash, &signature)?;

        Ok(Self {
            stream,
            raw_read_buffer: vec![],
            length: 0,
            aes: Aes256Gcm::new(&shared_secret.into()),
            format: SerializerFormat::Json,
            deferred_fmt: None,
        })
    }
    pub async fn read(&mut self) -> Result<MasterShipComm, Error> {
        self.read_for(Duration::from_secs(24 * 3600)).await
    }
    pub async fn read_for(&mut self, time: Duration) -> Result<MasterShipComm, Error> {
        let mut buf = [0; 4096];
        if !self.raw_read_buffer.is_empty() {
            if let Some(data) = self.extract_data()? {
                return Ok(data);
            }
        }
        loop {
            let read_bytes = self.read_timeout(&mut buf, time).await?;
            self.raw_read_buffer.extend_from_slice(&buf[..read_bytes]);
            if let Some(data) = self.extract_data()? {
                return Ok(data);
            }
        }
    }
    async fn read_timeout(&mut self, buf: &mut [u8], time: Duration) -> Result<usize, Error> {
        match tokio::time::timeout(time, self.stream.read(buf)).await {
            Ok(x) => match x? {
                0 => Err(std::io::Error::from(std::io::ErrorKind::ConnectionAborted).into()),
                x => Ok(x),
            },
            Err(_) => Err(Error::Timeout),
        }
    }
    pub async fn write(&mut self, data: MasterShipComm) -> Result<(), Error> {
        let data = self.encrypt(&self.format.serialize(&data)?)?;
        self.stream.write_all(&data).await?;
        if let Some(fmt) = self.deferred_fmt.take() {
            self.format = fmt;
        }
        Ok(())
    }
    pub fn write_blocking(&mut self, data: MasterShipComm) -> Result<(), Error> {
        let mut data = self.encrypt(&self.format.serialize(&data)?)?;
        loop {
            match self.stream.try_write(&data) {
                Ok(n) => {
                    data.drain(..n);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(e) => return Err(e.into()),
            }
            if data.is_empty() {
                break;
            }
        }
        if let Some(fmt) = self.deferred_fmt.take() {
            self.format = fmt;
        }
        Ok(())
    }
    pub fn get_ip(&self) -> std::io::Result<std::net::IpAddr> {
        self.stream.peer_addr().map(|a| a.ip())
    }
    fn extract_data(&mut self) -> Result<Option<MasterShipComm>, Error> {
        let mut output_data = vec![];
        if self.length == 0 && self.raw_read_buffer.len() > 4 {
            let len_buf: Vec<_> = self.raw_read_buffer.drain(..4).collect();
            self.length = u32::from_le_bytes(len_buf.try_into().unwrap()) - 4;
        }
        if self.raw_read_buffer.len() >= self.length as usize && self.length != 0 {
            output_data.extend(self.raw_read_buffer.drain(..self.length as usize));
            self.length = 0;
            let output_data = self.decrypt(&output_data)?;
            return Ok(Some(self.format.deserialize(&output_data)?));
        }
        Ok(None)
    }
    fn encrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
        let data = self
            .aes
            .encrypt(&nonce, data)
            .map_err(|e| Error::AEADError(e.to_string()))?;
        let length = 4 + nonce.len() + data.len();
        let mut out_data = vec![0; length];
        out_data[..4].copy_from_slice(&(length as u32).to_le_bytes()[..]);
        out_data[4..nonce.len() + 4].copy_from_slice(&nonce);
        out_data[nonce.len() + 4..].copy_from_slice(&data);
        Ok(out_data)
    }
    fn decrypt(&mut self, data: &[u8]) -> Result<Vec<u8>, Error> {
        if data.len() <= 12 {
            return Err(Error::InvalidInput);
        }
        let nonce: [u8; 12] = data[..12].try_into().unwrap();
        let nonce = nonce.into();
        let data = self
            .aes
            .decrypt(&nonce, &data[12..])
            .map_err(|e| Error::AEADError(e.to_string()))?;
        Ok(data)
    }
    async fn key_exchange(stream: &mut tokio::net::TcpStream) -> Result<[u8; 32], Error> {
        let secret = EphemeralSecret::random(&mut OsRng);
        let public_key = secret.public_key().to_sec1_bytes();
        stream.write_all(&public_key).await?;
        let mut public_key = vec![0u8; public_key.len()];
        stream.read_exact(&mut public_key[..]).await?;
        let public_key = PublicKey::from_sec1_bytes(&public_key[..])?;
        let hdkf = secret
            .diffie_hellman(&public_key)
            .extract::<sha2::Sha256>(None);
        let mut output = [0; 32];
        hdkf.expand(&[], &mut output)
            .map_err(|_| Error::HKDFError)?;
        Ok(output)
    }
    pub fn set_format(&mut self, format: SerializerFormat) {
        self.format = format;
    }
    pub fn set_deferred_fmt(&mut self, format: SerializerFormat) {
        self.deferred_fmt = Some(format);
    }
}

impl std::fmt::Debug for ShipLogin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShipLogin")
            .field("psk", &"[REDACTED]")
            .finish()
    }
}

impl std::fmt::Debug for UserCreds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UserCreds")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .field("ip", &self.ip)
            .finish()
    }
}

impl SerializerFormat {
    fn serialize<T: Serialize>(&self, data: &T) -> Result<Vec<u8>, Error> {
        match self {
            Self::Json => Ok(serde_json::to_vec(data)?),
            Self::MessagePack => Ok(rmp_serde::to_vec_named(data)?),
            Self::MessagePackUnnamed => Ok(rmp_serde::to_vec(data)?),
            Self::Bincode => Ok(bincode::serialize(data)?)
        }
    }
    fn deserialize<T: DeserializeOwned>(&self, data: &[u8]) -> Result<T, Error> {
        match self {
            Self::Json => Ok(serde_json::from_slice(data)?),
            Self::MessagePack | Self::MessagePackUnnamed => Ok(rmp_serde::from_slice(data)?),
            Self::Bincode => Ok(bincode::deserialize(data)?),
        }
    }
}
