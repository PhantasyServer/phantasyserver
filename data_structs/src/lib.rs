#[cfg(feature = "ship")]
use aes_gcm::{
    aead::{Aead, KeyInit},
    AeadCore, Aes256Gcm,
};
#[cfg(feature = "ship")]
use p256::{ecdh::EphemeralSecret, PublicKey};
use pso2packetlib::protocol::{
    items::ItemId,
    models::Position,
    server::LoadLevelPacket,
    spawn::{NPCSpawnPacket, ObjectSpawnPacket},
};
#[cfg(feature = "ship")]
use rand_core::OsRng;
use serde::{Deserialize, Serialize};
#[cfg(feature = "rmp")]
use std::io::Write;
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use thiserror::Error;
#[cfg(feature = "ship")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid input")]
    InvalidInput,
    #[error("Unknown hostkey: {0:?}")]
    UnknownHostkey([u8; 32]),
    #[error("Operation timedout")]
    Timeout,
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[cfg(feature = "json")]
    #[error(transparent)]
    SerdeError(#[from] serde_json::Error),
    #[cfg(feature = "rmp")]
    #[error(transparent)]
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[cfg(feature = "rmp")]
    #[error(transparent)]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[cfg(feature = "ship")]
    #[error(transparent)]
    P256ECDSAError(#[from] p256::ecdsa::Error),
    #[cfg(feature = "ship")]
    #[error(transparent)]
    P256ECError(#[from] p256::elliptic_curve::Error),
    #[cfg(feature = "ship")]
    #[error("Invalid key length")]
    HKDFError,
    #[cfg(feature = "ship")]
    #[error("AEAD Error: {0}")]
    AEADError(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ItemName {
    #[serde(flatten)]
    pub id: ItemId,
    pub en_name: String,
    pub jp_name: String,
    pub en_desc: String,
    pub jp_desc: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemParameters {
    #[serde(skip)]
    pub pc_attrs: Vec<u8>,
    #[serde(skip)]
    pub vita_attrs: Vec<u8>,
    pub names: Vec<ItemName>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    pub map_data: LoadLevelPacket,
    pub objects: Vec<ObjectSpawnPacket>,
    pub npcs: Vec<NPCSpawnPacket>,
    pub default_location: Position,
    pub luas: HashMap<String, String>,
    pub object_data: HashMap<u32, String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MasterShipComm {
    pub id: u32,
    pub action: MasterShipAction,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum MasterShipAction {
    RegisterShip,
}

#[cfg(feature = "ship")]
pub struct ShipConnection {
    stream: tokio::net::TcpStream,
    raw_read_buffer: Vec<u8>,
    length: u32,
    aes: Aes256Gcm,
}

#[cfg(feature = "ship")]
impl ShipConnection {
    pub async fn new_server(
        mut stream: tokio::net::TcpStream,
        hostkey: &[u8; 32],
    ) -> Result<Self, Error> {
        //send hostkey
        stream.write(hostkey).await?;
        let key = ShipConnection::key_exchange(&mut stream).await?;
        Ok(Self {
            stream,
            raw_read_buffer: vec![],
            length: 0,
            aes: Aes256Gcm::new(&key.into()),
        })
    }
    pub async fn new_client<F>(mut stream: tokio::net::TcpStream, check: F) -> Result<Self, Error>
    where
        F: FnOnce(Ipv4Addr, &[u8; 32]) -> bool,
    {
        //send hostkey
        let mut hostkey = [0; 32];

        tokio::time::timeout(Duration::from_secs(5), stream.read_exact(&mut hostkey))
            .await
            .map_err(|_| Error::Timeout)??;
        let IpAddr::V4(ip) = stream.peer_addr()?.ip() else {
            unreachable!()
        };
        if !check(ip, &hostkey) {
            return Err(Error::UnknownHostkey(hostkey));
        }
        let key = ShipConnection::key_exchange(&mut stream).await?;
        Ok(Self {
            stream,
            raw_read_buffer: vec![],
            length: 0,
            aes: Aes256Gcm::new(&key.into()),
        })
    }
    pub async fn read(&mut self) -> Result<MasterShipComm, Error> {
        self.read_for(Duration::from_secs(1 * 24 * 3600)).await
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
            Ok(x) => Ok(x?),
            Err(_) => Err(Error::Timeout),
        }
    }
    pub async fn write(&mut self, data: MasterShipComm) -> Result<(), Error> {
        let data = self.encrypt(&rmp_serde::to_vec(&data)?)?;
        self.stream.write_all(&data).await?;
        Ok(())
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
            return Ok(Some(rmp_serde::from_slice(&output_data)?));
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
}

impl ItemParameters {
    #[cfg(feature = "rmp")]
    pub fn load_from_mp_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::File::open(path)?;
        let names = rmp_serde::from_read(&data)?;
        Ok(names)
    }
    #[cfg(feature = "json")]
    pub fn load_from_json_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        let names = serde_json::from_str(&data)?;
        Ok(names)
    }
    #[cfg(feature = "rmp")]
    pub fn save_to_mp_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let mut file = std::fs::File::create(path)?;
        std::io::Write::write_all(&mut file, &rmp_serde::to_vec(self)?)?;
        Ok(())
    }
    #[cfg(feature = "json")]
    pub fn save_to_json_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}

impl MapData {
    #[cfg(feature = "rmp")]
    pub fn load_from_mp_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::File::open(path)?;
        let map = rmp_serde::from_read(&data)?;
        Ok(map)
    }
    #[cfg(feature = "json")]
    pub fn load_from_json_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        let map = serde_json::from_str(&data)?;
        Ok(map)
    }
    #[cfg(feature = "rmp")]
    pub fn save_to_mp_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(&rmp_serde::to_vec(self)?)?;
        Ok(())
    }
    #[cfg(feature = "json")]
    pub fn save_to_json_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}
