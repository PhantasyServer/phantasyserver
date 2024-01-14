pub mod flags;
#[cfg(feature = "ship")]
pub mod master_ship;
pub mod quest;
#[cfg(feature = "ship")]
pub use master_ship::*;

use pso2packetlib::protocol::{
    items::{Item, ItemId, StorageInfo},
    models::Position,
    server::LoadLevelPacket,
    spawn::{EventSpawnPacket, NPCSpawnPacket, ObjectSpawnPacket, TransporterSpawnPacket},
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

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

//---------------------------------------------------------------------
// new map data

pub type MapId = u32;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NewMapData {
    pub map_data: LoadLevelPacket,
    pub objects: Vec<ObjectData>,
    pub events: Vec<EventData>,
    pub npcs: Vec<NPCData>,
    pub transporters: Vec<TransporterData>,
    pub default_location: Vec<(MapId, Position)>,
    pub luas: HashMap<String, String>,
    pub init_map: MapId,
    pub map_names: HashMap<String, MapId>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ObjectData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: ObjectSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EventData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: EventSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NPCData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: NPCSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TransporterData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: TransporterSpawnPacket,
    pub lua_data: Option<String>,
}

//---------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountStorages {
    pub storage_meseta: u64,
    pub default: StorageInventory,
    pub premium: StorageInventory,
    pub extend1: StorageInventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageInventory {
    pub total_space: u32,
    pub storage_id: u8,
    pub is_enabled: bool,
    pub is_purchased: bool,
    pub storage_type: u8,
    pub items: Vec<Item>,
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

impl StorageInventory {
    pub fn generate_info(&self) -> StorageInfo {
        StorageInfo {
            total_space: self.total_space,
            used_space: self.items.len() as u32,
            storage_id: self.storage_id,
            storage_type: self.storage_type,
            is_locked: (!self.is_purchased) as u8,
            is_enabled: self.is_enabled as u8,
        }
    }
}

impl Default for AccountStorages {
    fn default() -> Self {
        Self {
            storage_meseta: 0,
            default: StorageInventory {
                total_space: 200,
                storage_id: 0,
                is_enabled: true,
                is_purchased: true,
                storage_type: 0,
                items: vec![],
            },
            premium: StorageInventory {
                total_space: 400,
                storage_id: 1,
                is_enabled: false,
                is_purchased: false,
                storage_type: 1,
                items: vec![],
            },
            extend1: StorageInventory {
                total_space: 500,
                storage_id: 2,
                is_enabled: false,
                is_purchased: false,
                storage_type: 2,
                items: vec![],
            },
        }
    }
}
impl Default for StorageInventory {
    fn default() -> Self {
        Self {
            total_space: 300,
            storage_id: 14,
            is_enabled: true,
            is_purchased: true,
            storage_type: 4,
            items: vec![],
        }
    }
}
