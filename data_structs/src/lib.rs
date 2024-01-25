#![deny(unsafe_code)]
#![warn(clippy::missing_const_for_fn)]

pub mod flags;
pub mod inventory;
pub mod map;
#[cfg(feature = "ship")]
pub mod master_ship;
pub mod quest;

use serde::{de::DeserializeOwned, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Invalid input")]
    InvalidInput,
    #[error("Unknown hostkey: {0:?}")]
    UnknownHostkey(Vec<u8>),
    #[error("Operation timed out")]
    Timeout,

    #[error("IO error: {0}")]
    IOError(#[from] std::io::Error),
    #[cfg(feature = "json")]
    #[error("JSON error: {0}")]
    SerdeError(#[from] serde_json::Error),
    #[cfg(feature = "rmp")]
    #[error("MP Serialization error: {0}")]
    RMPEncodeError(#[from] rmp_serde::encode::Error),
    #[cfg(feature = "rmp")]
    #[error("MP Deserialization error: {0}")]
    RMPDecodeError(#[from] rmp_serde::decode::Error),
    #[cfg(feature = "ship")]
    #[error("ECDSA error: {0}")]
    P256ECDSAError(#[from] p256::ecdsa::Error),
    #[cfg(feature = "ship")]
    #[error("Elliptic curve error: {0}")]
    P256ECError(#[from] p256::elliptic_curve::Error),
    #[cfg(feature = "ship")]
    #[error("Invalid key length")]
    HKDFError,
    #[cfg(feature = "ship")]
    #[error("AEAD error: {0}")]
    AEADError(String),
}

pub trait SerDeFile: Serialize + DeserializeOwned {
    #[cfg(feature = "rmp")]
    fn load_from_mp_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::File::open(path)?;
        let names = rmp_serde::from_read(&data)?;
        Ok(names)
    }
    #[cfg(feature = "json")]
    fn load_from_json_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        let names = serde_json::from_str(&data)?;
        Ok(names)
    }
    #[cfg(feature = "rmp")]
    fn save_to_mp_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let mut file = std::fs::File::create(path)?;
        std::io::Write::write_all(&mut file, &rmp_serde::to_vec(self)?)?;
        Ok(())
    }
    #[cfg(feature = "json")]
    fn save_to_json_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}
impl<T: Serialize + DeserializeOwned> SerDeFile for T {}
