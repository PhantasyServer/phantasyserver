use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::{
    self,
    chat::ChatArea,
    symbolart::{
        ChangeSymbolArtPacket, SendSymbolArtPacket, SymbolArtClientDataPacket,
        SymbolArtClientDataRequestPacket, SymbolArtDataPacket, SymbolArtDataRequestPacket,
        SymbolArtListPacket,
    },
    ObjectHeader, Packet,
};

pub async fn list_sa(user: &mut User) -> HResult {
    let uuids = user
        .blockdata
        .sql
        .get_symbol_art_list(user.player_id)
        .await?;
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

pub async fn change_sa(user: &mut User, packet: ChangeSymbolArtPacket) -> HResult {
    let mut uuids = user
        .blockdata
        .sql
        .get_symbol_art_list(user.player_id)
        .await?;
    for uuid in packet.uuids {
        let slot = uuid.slot;
        let uuid = uuid.uuid;
        if let Some(data) = uuids.get_mut(slot as usize) {
            *data = uuid;
        }
        if uuid == 0 {
            continue;
        }
        if user.blockdata.sql.get_symbol_art(uuid).await?.is_none() {
            user.send_packet(&Packet::SymbolArtDataRequest(SymbolArtDataRequestPacket {
                uuid,
            }))?;
        }
    }
    user.blockdata
        .sql
        .set_symbol_art_list(uuids, user.player_id)
        .await?;
    user.send_packet(&Packet::SymbolArtResult(Default::default()))?;
    Ok(Action::Nothing)
}

pub async fn add_sa(user: &mut User, packet: SymbolArtDataPacket) -> HResult {
    user.blockdata
        .sql
        .add_symbol_art(packet.uuid, &packet.data, &packet.name)
        .await?;
    Ok(Action::Nothing)
}

pub async fn data_request(user: &mut User, packet: SymbolArtClientDataRequestPacket) -> HResult {
    if let Some(sa) = user.blockdata.sql.get_symbol_art(packet.uuid).await? {
        user.send_packet(&Packet::SymbolArtClientData(SymbolArtClientDataPacket {
            uuid: packet.uuid,
            data: sa,
        }))?;
    }
    Ok(Action::Nothing)
}

pub async fn send_sa(user: MutexGuard<'_, User>, packet: SendSymbolArtPacket) -> HResult {
    let id = user.player_id;
    let map = user.get_current_map();
    let party = user.get_current_party();
    drop(user);
    if ChatArea::Map == packet.area {
        if let Some(map) = map {
            map.lock().await.send_sa(packet, id).await;
        }
    } else if ChatArea::Party == packet.area {
        if let Some(party) = party {
            party.read().await.send_sa(packet, id).await;
        }
    }

    Ok(Action::Nothing)
}
