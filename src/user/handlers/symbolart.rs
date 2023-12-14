use super::HResult;
use crate::{Action, User};
use parking_lot::MutexGuard;
use pso2packetlib::protocol::{
    self,
    symbolart::{
        ChangeSymbolArtPacket, SendSymbolArtPacket, SymbolArtClientDataPacket,
        SymbolArtClientDataRequestPacket, SymbolArtDataPacket, SymbolArtDataRequestPacket,
        SymbolArtListPacket,
    },
    ChatArea, ObjectHeader, Packet,
};

pub fn list_sa(user: &mut User) -> HResult {
    let sql_provider = user.sql.clone();
    let sql = sql_provider.read();
    let uuids = sql.get_symbol_art_list(user.player_id)?;
    user.send_packet(&Packet::SymbolArtList(SymbolArtListPacket {
        object: ObjectHeader {
            id: user.player_id,
            entity_type: protocol::EntityType::Player,
            ..Default::default()
        },
        character_id: user.char_id,
        uuids,
    }))?;
    Ok(Action::Nothing)
}

pub fn change_sa(user: &mut User, packet: ChangeSymbolArtPacket) -> HResult {
    let sql_provider = user.sql.clone();
    let mut sql = sql_provider.write();
    let mut uuids = sql.get_symbol_art_list(user.player_id)?;
    for uuid in packet.uuids {
        let slot = uuid.slot;
        let uuid = uuid.uuid;
        if let Some(data) = uuids.get_mut(slot as usize) {
            *data = uuid;
        }
        if uuid == 0 {
            continue;
        }
        if sql.get_symbol_art(uuid)?.is_none() {
            user.send_packet(&Packet::SymbolArtDataRequest(SymbolArtDataRequestPacket {
                uuid,
            }))?;
        }
    }
    sql.set_symbol_art_list(uuids, user.player_id)?;
    user.send_packet(&Packet::SymbolArtResult(Default::default()))?;
    Ok(Action::Nothing)
}

pub fn add_sa(user: &mut User, packet: SymbolArtDataPacket) -> HResult {
    let sql_provider = user.sql.clone();
    let mut sql = sql_provider.write();
    sql.add_symbol_art(packet.uuid, &packet.data, &packet.name)?;
    Ok(Action::Nothing)
}

pub fn data_request(user: &mut User, packet: SymbolArtClientDataRequestPacket) -> HResult {
    let sql_provider = user.sql.clone();
    let sql = sql_provider.read();
    if let Some(sa) = sql.get_symbol_art(packet.uuid)? {
        user.send_packet(&Packet::SymbolArtClientData(SymbolArtClientDataPacket {
            uuid: packet.uuid,
            data: sa,
        }))?;
    }
    Ok(Action::Nothing)
}

pub fn send_sa(user: MutexGuard<User>, packet: SendSymbolArtPacket) -> HResult {
    if let ChatArea::Map = packet.area {
        let id = user.player_id;
        let map = user.map.clone();
        drop(user);
        if let Some(map) = map {
            map.lock().send_sa(packet, id)
        }
    }
    Ok(Action::Nothing)
}
