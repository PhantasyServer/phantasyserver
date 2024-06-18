#![deny(unsafe_code)]
#![warn(clippy::missing_const_for_fn)]

pub mod flags;
pub mod inventory;
pub mod map;
#[cfg(feature = "ship")]
pub mod master_ship;
pub mod quest;
pub mod stats;

use std::collections::HashMap;
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
        let names = Self::deserialize(&mut rmp_serde::Deserializer::new(data).with_human_readable())?;
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
        let file = std::fs::File::create(path)?;
        self.serialize(&mut rmp_serde::Serializer::new(file).with_human_readable())?;
        // std::io::Write::write_all(&mut file, &rmp_serde::to_vec(self)?)?;
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

#[derive(Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ServerData {
    pub maps: HashMap<String, map::MapData>,
    pub quests: Vec<quest::QuestData>,
    pub item_params: inventory::ItemParameters,
    pub player_stats: stats::PlayerStats,
    pub enemy_stats: stats::AllEnemyStats,
    pub attack_stats: Vec<stats::AttackStats>,
}

pub fn name_to_id(name: &str) -> u32 {
    name.chars().fold(0u32, |acc, c| {
        acc ^ ((acc << 6).overflowing_add((acc >> 2).overflowing_sub(0x61c88647 - c as u32).0)).0
    })
}
