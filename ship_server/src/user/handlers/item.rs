use super::HResult;
use crate::{Action, Error, User, mutex::MutexGuard};
use pso2packetlib::protocol::{
    self, Packet,
    items::{
        DiscardItemRequestPacket, DiscardStorageItemRequestPacket, EquipItemPacket,
        EquipItemRequestPacket, GetItemDescriptionPacket, ItemType, LoadItemDescriptionPacket,
        MoveMesetaPacket, MoveStoragesRequestPacket, MoveToInventoryRequestPacket,
        MoveToStorageRequestPacket, UnequipItemPacket, UnequipItemRequestPacket,
    },
    login::Language,
};

pub async fn move_to_storage(user: &mut User, packet: MoveToStorageRequestPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character
        .inventory
        .move_to_storage(packet, &mut user.user_data.last_uuid)?;
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn move_to_inventory(user: &mut User, packet: MoveToInventoryRequestPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character
        .inventory
        .move_to_inventory(packet, &mut user.user_data.last_uuid)?;
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn move_meseta(user: &mut User, packet: MoveMesetaPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packets = character.inventory.move_meseta(packet);
    for packet in packets {
        user.send_packet(&packet).await?;
    }
    Ok(Action::Nothing)
}

pub async fn discard_inventory(user: &mut User, packet: DiscardItemRequestPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character.inventory.discard_inventory(packet)?;
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn discard_storage(user: &mut User, packet: DiscardStorageItemRequestPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character.inventory.discard_storage(packet)?;
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn move_storages(user: &mut User, packet: MoveStoragesRequestPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character
        .inventory
        .move_storages(packet, &mut user.user_data.last_uuid)?;
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn get_description(user: &mut User, packet: GetItemDescriptionPacket) -> HResult {
    let names_ref = &user.blockdata.server_data.item_params;
    match names_ref.names.iter().find(|x| x.id == packet.item) {
        Some(name) => {
            let packet = LoadItemDescriptionPacket {
                unk1: 1,
                item: packet.item,
                desc: match user.user_data.lang {
                    Language::English => name.en_desc.clone(),
                    Language::Japanese => name.jp_desc.clone(),
                },
            };
            user.send_packet(&protocol::Packet::LoadItemDescription(packet))
                .await?;
        }
        None => log::debug!("No item description for {:?}", packet.item),
    }

    Ok(Action::Nothing)
}

pub async fn equip_item(mut user: MutexGuard<'_, User>, packet: EquipItemRequestPacket) -> HResult {
    let Some(char) = &mut user.character else {
        unreachable!("User should be in state >= `PreInGame`")
    };
    char.inventory
        .equip_item(packet.uuid, packet.equipment_pos)?;
    let item = char.inventory.get_inv_item(packet.uuid)?;
    if let ItemType::Clothing(data) = &item.data {
        //BUG: a (0x0F, 0x2B) packet should also be sent, but let's not worry about it at this time
        let block_data = user.get_blockdata();
        let clothing_stats = block_data
            .server_data
            .item_params
            .attrs
            .human_costumes
            .iter()
            .find(|a| a.id == item.id.id && a.subid == item.id.subid)
            .cloned()
            .ok_or(Error::NoItemInAttrs(item.id.id, item.id.subid))?;
        let Some(char) = &mut user.character else {
            unreachable!();
        };
        char.character.look.costume_id = clothing_stats.model;
        char.character.look.costume_color = data.color.clone();
    }

    let equip_packet = Packet::EquipItem(EquipItemPacket {
        player_equiped: user.create_object_header(),
        equiped_item: item.clone(),
        equipment_pos: packet.equipment_pos,
        ..Default::default()
    });
    let zone = user.zone_pos;
    if let Some(map) = user.get_current_map() {
        drop(user);
        map.lock().await.send_to_all(zone, &equip_packet).await;
    }

    Ok(Action::Nothing)
}

pub async fn unequip_item(
    mut user: MutexGuard<'_, User>,
    packet: UnequipItemRequestPacket,
) -> HResult {
    let Some(char) = &mut user.character else {
        unreachable!("User should be in state >= `PreInGame`")
    };
    char.inventory.unequip_item(packet.uuid)?;
    let item = char.inventory.get_inv_item(packet.uuid)?;
    let equip_packet = Packet::UnequipItem(UnequipItemPacket {
        player_unequiped: user.create_object_header(),
        unequiped_item: item.clone(),
        equipment_pos: packet.equipment_pos,
        ..Default::default()
    });
    let zone = user.zone_pos;
    if let Some(map) = user.get_current_map() {
        drop(user);
        map.lock().await.send_to_all(zone, &equip_packet).await;
    }

    Ok(Action::Nothing)
}
