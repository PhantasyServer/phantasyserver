use crate::Error;
use data_structs::{AccountStorages, ItemParameters, StorageInventory};
use pso2packetlib::protocol::{
    items::{
        DiscardItemRequestPacket, DiscardStorageItemRequestPacket, EquipedItem,
        InventoryMesetaPacket, Item, ItemId, ItemType, LoadEquipedPacket, LoadItemPacket,
        LoadPlayerInventoryPacket, LoadStoragesPacket, MesetaDirection, MoveMesetaPacket,
        MoveStoragesPacket, MoveStoragesRequestPacket, MoveToInventoryPacket,
        MoveToInventoryRequestPacket, MoveToStoragePacket, MoveToStorageRequestPacket, NamedId,
        NewInventoryItem, NewStorageItem, StorageMesetaPacket, UpdateInventoryPacket,
        UpdateStoragePacket,
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
    equiped: Vec<u64>,
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
        if let Some(x) = load_items_inner(
            &mut self.loaded_items,
            &self.inventory.items,
            item_names,
            lang,
        ) {
            packets.push(Packet::LoadItem(x));
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
        packets.push(self.send_equiped(player_id));

        // load storages
        //BUG: i think that this packet should be split if there are too many items
        let mut storage_items = vec![];
        let mut infos = vec![];
        // character storage
        if let Some(x) = load_items_inner(
            &mut self.loaded_items,
            &self.character.items,
            item_names,
            lang,
        ) {
            packets.push(Packet::LoadItem(x));
        }
        storage_items.extend_from_slice(&self.character.items);
        infos.push(self.character.generate_info());

        // default storage
        if let Some(x) = load_items_inner(
            &mut self.loaded_items,
            &self.storages.default.items,
            item_names,
            lang,
        ) {
            packets.push(Packet::LoadItem(x));
        }
        storage_items.extend_from_slice(&self.storages.default.items);
        infos.push(self.storages.default.generate_info());

        // premium storage
        if let Some(x) = load_items_inner(
            &mut self.loaded_items,
            &self.storages.premium.items,
            item_names,
            lang,
        ) {
            packets.push(Packet::LoadItem(x));
        }
        storage_items.extend_from_slice(&self.storages.premium.items);
        infos.push(self.storages.premium.generate_info());

        // extend1 storage
        if let Some(x) = load_items_inner(
            &mut self.loaded_items,
            &self.storages.extend1.items,
            item_names,
            lang,
        ) {
            packets.push(Packet::LoadItem(x));
        }
        storage_items.extend_from_slice(&self.storages.extend1.items);
        infos.push(self.storages.extend1.generate_info());

        packets.push(Packet::LoadStorages(LoadStoragesPacket {
            stored_meseta: self.storages.storage_meseta,
            unk1: infos,
            unk2: 2,
            items: storage_items,
        }));
        packets
    }
    pub fn send_equiped(&self, player_id: u32) -> Packet {
        let mut equiped_items = LoadEquipedPacket::default();
        for (pos, equiped) in self.inventory.equiped.iter().enumerate() {
            let Some(item) = self.inventory.items.iter().find(|x| x.uuid == *equiped) else {
                continue;
            };
            equiped_items.items.push(EquipedItem {
                item: item.clone(),
                unk: pos as u32,
            });
        }
        equiped_items.player = ObjectHeader {
            id: player_id,
            entity_type: pso2packetlib::protocol::EntityType::Player,
            ..Default::default()
        };
        Packet::LoadEquiped(equiped_items)
    }
    pub fn equip_item(&mut self, uuid: u64) -> Result<(), Error> {
        if self.inventory.equiped.iter().any(|&x| x == uuid) {
            return Ok(());
        }
        self.inventory
            .items
            .iter()
            .find(|x| x.uuid == uuid)
            .ok_or(Error::InvalidInput("equip_item"))?;
        self.inventory.equiped.push(uuid);
        Ok(())
    }
    pub fn unequip_item(&mut self, uuid: u64) -> Result<(), Error> {
        let (pos, _) = self
            .inventory
            .equiped
            .iter()
            .enumerate()
            .find(|(_, &x)| x == uuid)
            .ok_or(Error::InvalidInput("unequip_item"))?;
        self.inventory.equiped.remove(pos);
        Ok(())
    }
    pub fn get_inv_item(&self, uuid: u64) -> Result<Item, Error> {
        self.inventory
            .items
            .iter()
            .find(|x| x.uuid == uuid)
            .ok_or(Error::InvalidInput("get_inv_item"))
            .cloned()
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
                _ => return Err(Error::InvalidInput("move_to_storage")),
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
                _ => return Err(Error::InvalidInput("move_to_inventory")),
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
                            new_amount,
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
                _ => return Err(Error::InvalidInput("move_storages")),
            };
            let result = decrease_item(&mut storage_src.items, info.uuid, info.amount)?;
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
                            new_amount,
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
                _ => return Err(Error::InvalidInput("move_storages")),
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
        let mut packet_out = UpdateInventoryPacket {
            unk2: 1,
            ..Default::default()
        };
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
        let mut packet_out = UpdateStoragePacket {
            unk2: 1,
            ..Default::default()
        };
        for info in packet.items {
            let storage = match info.storage_id {
                0 => &mut self.storages.default,
                1 => &mut self.storages.premium,
                2 => &mut self.storages.extend1,
                14 => &mut self.character,
                _ => return Err(Error::InvalidInput("discard_storage")),
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
        .ok_or(Error::InvalidInput("decrease_item"))?;
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
            return Err(Error::InvalidInput("decrease_item"));
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
                Err(Error::InvalidInput("increase_item"))
            }
        }
        None => {
            items.push(item.clone());
            Ok(ChangeItemResult::New { item, amount })
        }
    }
}

impl Default for PlayerInventory {
    fn default() -> Self {
        Self {
            meseta: 0,
            max_capacity: 50,
            items: vec![],
            equiped: vec![],
        }
    }
}
