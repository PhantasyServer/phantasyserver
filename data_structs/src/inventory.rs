use pso2packetlib::protocol::{items::{Item, ItemId, StorageInfo}, models::item_attrs::ItemAttributesPC};
use serde::{Deserialize, Serialize};

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
    #[serde(skip)]
    pub attrs: ItemAttributesPC,
    pub names: Vec<ItemName>,
}

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
