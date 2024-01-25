use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{
    self,
    items::{
        DiscardItemRequestPacket, DiscardStorageItemRequestPacket, GetItemDescriptionPacket,
        LoadItemDescriptionPacket, MoveMesetaPacket, MoveStoragesRequestPacket,
        MoveToInventoryRequestPacket, MoveToStorageRequestPacket,
    },
    login::Language,
};

pub fn move_to_storage(user: &mut User, packet: MoveToStorageRequestPacket) -> HResult {
    let packet = user.inventory.move_to_storage(packet, &mut user.uuid)?;
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

pub fn move_to_inventory(user: &mut User, packet: MoveToInventoryRequestPacket) -> HResult {
    let packet = user.inventory.move_to_inventory(packet, &mut user.uuid)?;
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

pub fn move_meseta(user: &mut User, packet: MoveMesetaPacket) -> HResult {
    let packets = user.inventory.move_meseta(packet);
    for packet in packets {
        user.send_packet(&packet)?;
    }
    Ok(Action::Nothing)
}

pub fn discard_inventory(user: &mut User, packet: DiscardItemRequestPacket) -> HResult {
    let packet = user.inventory.discard_inventory(packet)?;
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

pub fn discard_storage(user: &mut User, packet: DiscardStorageItemRequestPacket) -> HResult {
    let packet = user.inventory.discard_storage(packet)?;
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

pub fn move_storages(user: &mut User, packet: MoveStoragesRequestPacket) -> HResult {
    let packet = user.inventory.move_storages(packet, &mut user.uuid)?;
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

pub async fn get_description(user: &mut User, packet: GetItemDescriptionPacket) -> HResult {
    let names_ref = user.blockdata.item_attrs.clone();
    match names_ref
        .read()
        .await
        .names
        .iter()
        .find(|x| x.id == packet.item)
    {
        Some(name) => {
            let packet = LoadItemDescriptionPacket {
                unk1: 1,
                item: packet.item,
                desc: match user.text_lang {
                    Language::English => name.en_desc.clone(),
                    Language::Japanese => name.jp_desc.clone(),
                },
            };
            user.send_packet(&protocol::Packet::LoadItemDescription(packet))?;
        }
        None => log::debug!("No item description for {:?}", packet.item),
    }

    Ok(Action::Nothing)
}
