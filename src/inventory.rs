use crate::Error;
use pso2packetlib::protocol::{
    items::{
        DiscardItemRequestPacket, DiscardStorageItemRequestPacket, InventoryMesetaPacket, Item,
        ItemId, ItemType, LoadItemPacket, LoadPlayerInventoryPacket, LoadStoragesPacket,
        MesetaDirection, MoveMesetaPacket, MoveStoragesPacket, MoveStoragesRequestPacket,
        MoveToInventoryPacket, MoveToInventoryRequestPacket, MoveToStoragePacket,
        MoveToStorageRequestPacket, NamedId, NewInventoryItem, NewStorageItem, StorageInfo,
        StorageMesetaPacket, UpdateInventoryPacket, UpdateStoragePacket,
    },
    login::Language,
    ObjectHeader, Packet,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Inventory {
    pub(crate) inventory: PlayerInventory,
    pub(crate) character: StorageInventory,
    #[serde(skip)]
    pub(crate) storages: AccountStorages,

    #[serde(skip)]
    loaded_items: Vec<ItemId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlayerInventory {
    pub(crate) meseta: u64,
    pub(crate) max_capacity: u32,
    pub(crate) items: Vec<Item>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AccountStorages {
    pub(crate) storage_meseta: u64,
    pub(crate) default: StorageInventory,
    pub(crate) premium: StorageInventory,
    pub(crate) extend1: StorageInventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageInventory {
    pub(crate) total_space: u32,
    pub(crate) storage_id: u8,
    pub(crate) is_enabled: bool,
    pub(crate) is_purchased: bool,
    pub(crate) storage_type: u8,
    pub(crate) items: Vec<Item>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemParameters {
    #[serde(skip)]
    pub pc_attrs: Vec<u8>,
    #[serde(skip)]
    pub vita_attrs: Vec<u8>,
    pub names: Vec<ItemName>,
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

enum ChangeItemResult {
    Changed {
        uuid: u64,
        new_amount: u16,
        moved: u16,
        item: Item,
    },
    New {
        item: Item,
        amount: u16,
    },
    Removed {
        item: Item,
        amount: u16,
    },
}

impl Inventory {
    pub fn send(
        &mut self,
        player_id: u32,
        name: String,
        item_names: &ItemParameters,
        lang: Language,
    ) -> Vec<Packet> {
        let mut packets = vec![];

        // load inventory
        match load_items_inner(
            &mut self.loaded_items,
            &self.inventory.items,
            item_names,
            lang,
        ) {
            Some(x) => packets.push(Packet::LoadItem(x)),
            None => {}
        }
        packets.push(Packet::LoadPlayerInventory(LoadPlayerInventoryPacket {
            object: ObjectHeader {
                id: player_id,
                entity_type: pso2packetlib::protocol::EntityType::Player,
                ..Default::default()
            },
            name,
            meseta: self.inventory.meseta,
            max_capacity: self.inventory.max_capacity,
            items: self.inventory.items.clone(),
        }));

        // load storages
        //BUG: i think that this packet should be split if there are too many items
        let mut storage_items = vec![];
        let mut infos = vec![];
        // character storage
        match load_items_inner(
            &mut self.loaded_items,
            &self.character.items,
            item_names,
            lang,
        ) {
            Some(x) => packets.push(Packet::LoadItem(x)),
            None => {}
        }
        storage_items.extend_from_slice(&self.character.items);
        infos.push(self.character.generate_info());

        // default storage
        match load_items_inner(
            &mut self.loaded_items,
            &self.storages.default.items,
            item_names,
            lang,
        ) {
            Some(x) => packets.push(Packet::LoadItem(x)),
            None => {}
        }
        storage_items.extend_from_slice(&self.storages.default.items);
        infos.push(self.storages.default.generate_info());

        // premium storage
        match load_items_inner(
            &mut self.loaded_items,
            &self.storages.premium.items,
            item_names,
            lang,
        ) {
            Some(x) => packets.push(Packet::LoadItem(x)),
            None => {}
        }
        storage_items.extend_from_slice(&self.storages.premium.items);
        infos.push(self.storages.premium.generate_info());

        // extend1 storage
        match load_items_inner(
            &mut self.loaded_items,
            &self.storages.extend1.items,
            item_names,
            lang,
        ) {
            Some(x) => packets.push(Packet::LoadItem(x)),
            None => {}
        }
        storage_items.extend_from_slice(&self.storages.extend1.items);
        infos.push(self.storages.extend1.generate_info());

        packets.push(Packet::LoadStorages(LoadStoragesPacket {
            stored_meseta: self.storages.storage_meseta,
            unk3: infos,
            unk5: 2,
            items: storage_items,
        }));
        packets
    }
    pub fn move_to_storage(
        &mut self,
        packet: MoveToStorageRequestPacket,
        new_uuid: &mut u64,
    ) -> Result<Packet, Error> {
        let mut packet_out = MoveToStoragePacket::default();
        for info in packet.uuids {
            let storage = match info.storage_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput),
            };
            let result = decrease_item(&mut self.inventory.items, info.uuid, info.amount as u16)?;
            let (item, amount) = match result {
                ChangeItemResult::Changed {
                    uuid,
                    new_amount,
                    moved,
                    mut item,
                } => {
                    packet_out.updated_inventory.push(
                        pso2packetlib::protocol::items::UpdatedInventoryItem {
                            uuid,
                            new_amount,
                            moved,
                        },
                    );
                    *new_uuid += 1;
                    item.uuid = *new_uuid;
                    (item, moved)
                }
                ChangeItemResult::Removed { item, amount } => {
                    packet_out.updated_inventory.push(
                        pso2packetlib::protocol::items::UpdatedInventoryItem {
                            uuid: item.uuid,
                            new_amount: 0,
                            moved: amount,
                        },
                    );
                    (item, amount)
                }
                _ => unreachable!(),
            };
            match increase_item(&mut storage.items, item, amount)? {
                ChangeItemResult::Changed {
                    uuid, new_amount, ..
                } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedItem {
                            uuid,
                            new_amount: new_amount as u32,
                            storage_id: storage.storage_id as u32,
                        });
                }
                ChangeItemResult::New { item, .. } => {
                    packet_out.new_items.push(NewStorageItem {
                        item,
                        storage_id: storage.storage_id as u32,
                    });
                }
                _ => unreachable!(),
            }
        }
        Ok(Packet::MoveToStorage(packet_out))
    }
    pub fn move_to_inventory(
        &mut self,
        packet: MoveToInventoryRequestPacket,
        new_uuid: &mut u64,
    ) -> Result<Packet, Error> {
        let mut packet_out = MoveToInventoryPacket::default();
        for info in packet.uuids {
            let storage = match info.storage_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput),
            };
            let result = decrease_item(&mut storage.items, info.uuid, info.amount as u16)?;
            let (item, amount) = match result {
                ChangeItemResult::Changed {
                    uuid,
                    new_amount,
                    moved,
                    mut item,
                } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid,
                            new_amount: new_amount as u16,
                            storage_id: storage.storage_id as u32,
                            moved,
                        });
                    *new_uuid += 1;
                    item.uuid = *new_uuid;
                    (item, moved)
                }
                ChangeItemResult::Removed { item, amount } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid: item.uuid,
                            new_amount: 0,
                            storage_id: storage.storage_id as u32,
                            moved: amount,
                        });
                    (item, amount)
                }
                _ => unreachable!(),
            };
            match increase_item(&mut self.inventory.items, item, amount)? {
                ChangeItemResult::Changed {
                    new_amount, item, ..
                } => {
                    packet_out.new_items.push(NewInventoryItem {
                        item,
                        amount: new_amount,
                        is_new: 0,
                    });
                }
                ChangeItemResult::New { item, amount } => {
                    packet_out.new_items.push(NewInventoryItem {
                        item,
                        amount,
                        is_new: 1,
                    });
                }
                _ => unreachable!(),
            }
        }
        Ok(Packet::MoveToInventory(packet_out))
    }
    pub fn move_storages(
        &mut self,
        packet: MoveStoragesRequestPacket,
        new_uuid: &mut u64,
    ) -> Result<Packet, Error> {
        let mut packet_out = MoveStoragesPacket::default();
        for info in packet.items {
            let storage_src = match packet.old_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput),
            };
            let result = decrease_item(&mut storage_src.items, info.uuid, info.amount as u16)?;
            let (item, amount) = match result {
                ChangeItemResult::Changed {
                    uuid,
                    new_amount,
                    moved,
                    mut item,
                } => {
                    packet_out.updated_old.push(
                        pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid,
                            new_amount: new_amount as u16,
                            storage_id: storage_src.storage_id as u32,
                            moved,
                        },
                    );
                    *new_uuid += 1;
                    item.uuid = *new_uuid;
                    (item, moved)
                }
                ChangeItemResult::Removed { item, amount } => {
                    packet_out.updated_old.push(
                        pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid: item.uuid,
                            new_amount: 0,
                            storage_id: storage_src.storage_id as u32,
                            moved: amount,
                        },
                    );
                    (item, amount)
                }
                _ => unreachable!(),
            };
            let storage_dst = match packet.new_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput),
            };
            match increase_item(&mut storage_dst.items, item, amount)? {
                ChangeItemResult::Changed {
                    new_amount,
                    uuid,
                    moved,
                    ..
                } => {
                    packet_out.updated_new.push(
                        pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid,
                            new_amount,
                            moved,
                            storage_id: storage_dst.storage_id as u32,
                        },
                    );
                }
                ChangeItemResult::New { item, .. } => {
                    packet_out.new_items.push(NewStorageItem {
                        item,
                        storage_id: storage_dst.storage_id as u32,
                    });
                }
                _ => unreachable!(),
            }
        }
        Ok(Packet::MoveStorages(packet_out))
    }
    pub fn discard_inventory(&mut self, packet: DiscardItemRequestPacket) -> Result<Packet, Error> {
        let mut packet_out = UpdateInventoryPacket::default();
        packet_out.unk2 = 1;
        for info in packet.items {
            match decrease_item(&mut self.inventory.items, info.uuid, info.amount)? {
                ChangeItemResult::Changed {
                    new_amount, moved, ..
                } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedInventoryItem {
                            uuid: info.uuid,
                            new_amount,
                            moved,
                        })
                }
                ChangeItemResult::Removed { amount, .. } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedInventoryItem {
                            uuid: info.uuid,
                            new_amount: 0,
                            moved: amount,
                        })
                }
                _ => unreachable!(),
            }
        }
        Ok(Packet::UpdateInventory(packet_out))
    }
    pub fn discard_storage(
        &mut self,
        packet: DiscardStorageItemRequestPacket,
    ) -> Result<Packet, Error> {
        let mut packet_out = UpdateStoragePacket::default();
        packet_out.unk2 = 1;
        for info in packet.items {
            let storage = match info.storage_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput),
            };
            match decrease_item(&mut storage.items, info.uuid, info.amount as u16)? {
                ChangeItemResult::Changed {
                    new_amount, moved, ..
                } => packet_out
                    .updated
                    .push(pso2packetlib::protocol::items::UpdatedStorageItem {
                        uuid: info.uuid,
                        new_amount,
                        moved,
                        storage_id: storage.storage_id as u32,
                    }),
                ChangeItemResult::Removed { amount, .. } => {
                    packet_out
                        .updated
                        .push(pso2packetlib::protocol::items::UpdatedStorageItem {
                            uuid: info.uuid,
                            new_amount: 0,
                            moved: amount,
                            storage_id: storage.storage_id as u32,
                        })
                }
                _ => unreachable!(),
            }
        }
        Ok(Packet::UpdateStorage(packet_out))
    }
    pub fn move_meseta(&mut self, packet: MoveMesetaPacket) -> Vec<Packet> {
        let mut packets = vec![];
        let (src, dest) = match packet.direction {
            MesetaDirection::ToStorage => (
                &mut self.inventory.meseta,
                &mut self.storages.storage_meseta,
            ),
            MesetaDirection::ToInventory => (
                &mut self.storages.storage_meseta,
                &mut self.inventory.meseta,
            ),
        };
        let to_move = u64::min(*src, packet.meseta);
        *dest += to_move;
        *src -= to_move;
        packets.push(Packet::InventoryMeseta(InventoryMesetaPacket {
            meseta: self.inventory.meseta,
        }));
        packets.push(Packet::StorageMeseta(StorageMesetaPacket {
            meseta: self.storages.storage_meseta,
        }));
        packets
    }
}
fn load_items_inner(
    loaded: &mut Vec<ItemId>,
    items: &[Item],
    item_names: &ItemParameters,
    lang: Language,
) -> Option<LoadItemPacket> {
    let mut load_items = LoadItemPacket::default();
    for item in items {
        if !loaded.contains(&item.id) {
            loaded.push(item.id);
            match item_names.names.iter().find(|x| x.id == item.id) {
                Some(name) => load_items.items.push(NamedId {
                    name: match lang {
                        Language::English => name.en_name.clone(),
                        Language::Japanese => name.jp_name.clone(),
                    },
                    id: item.id,
                }),
                None => {
                    println!("Unknown item: {:?}", item.id);
                    continue;
                }
            }
        }
    }
    if load_items.items.is_empty() {
        None
    } else {
        Some(load_items)
    }
}

fn decrease_item(items: &mut Vec<Item>, uuid: u64, amount: u16) -> Result<ChangeItemResult, Error> {
    let (pos, item) = items
        .iter_mut()
        .enumerate()
        .find(|(_, x)| x.uuid == uuid)
        .ok_or(Error::InvalidInput)?;
    if let ItemType::Consumable(data) = &mut item.data {
        let taken = u16::min(amount, data.amount);
        let new_amount = data.amount.saturating_sub(taken);
        if new_amount == 0 {
            Ok(ChangeItemResult::Removed {
                item: items.swap_remove(pos),
                amount: taken,
            })
        } else {
            data.amount = new_amount;
            let mut data = data.clone();
            data.amount = taken;
            let item = Item {
                uuid: 0,
                id: item.id,
                data: ItemType::Consumable(data),
            };
            Ok(ChangeItemResult::Changed {
                uuid,
                new_amount,
                moved: taken,
                item,
            })
        }
    } else {
        if amount > 1 {
            return Err(Error::InvalidInput);
        }
        Ok(ChangeItemResult::Removed {
            item: items.swap_remove(pos),
            amount: 1,
        })
    }
}

fn increase_item(
    items: &mut Vec<Item>,
    item: Item,
    amount: u16,
) -> Result<ChangeItemResult, Error> {
    let inv_item = items.iter_mut().find(|x| x.id == item.id);
    match inv_item {
        Some(i_item) => {
            if let ItemType::Consumable(i_data) = &mut i_item.data {
                i_data.amount += amount;
                Ok(ChangeItemResult::Changed {
                    uuid: i_item.uuid,
                    new_amount: i_data.amount,
                    moved: amount,
                    item: i_item.clone(),
                })
            } else {
                Err(Error::InvalidInput)
            }
        }
        None => {
            items.push(item.clone());
            Ok(ChangeItemResult::New { item, amount })
        }
    }
}

impl ItemParameters {
    pub fn load_from_mp_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::File::open(path)?;
        let names = rmp_serde::from_read(&data)?;
        Ok(names)
    }
    pub fn load_from_json_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        let names = serde_json::from_str(&data)?;
        Ok(names)
    }
    pub fn save_to_mp_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let mut file = std::fs::File::create(path)?;
        std::io::Write::write_all(&mut file, &rmp_serde::to_vec(self)?)?;
        Ok(())
    }
    pub fn save_to_json_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}

impl StorageInventory {
    fn generate_info(&self) -> StorageInfo {
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

impl Default for PlayerInventory {
    fn default() -> Self {
        Self {
            meseta: 0,
            max_capacity: 50,
            items: vec![],
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
