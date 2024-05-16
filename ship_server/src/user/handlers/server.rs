use super::HResult;
use crate::{mutex::MutexGuard, Action, Error, User};
use pso2packetlib::protocol::{
    self,
    flag::{FlagType, SetFlagPacket},
    server::{
        BridgeToLobbyPacket, BridgeTransportPacket, CafeToLobbyPacket, CafeTransportPacket,
        CampshipDownPacket, CasinoToLobbyPacket, CasinoTransportPacket, MapLoadedPacket,
        ToCampshipPacket,
    },
    Packet,
};

pub async fn move_to_bridge(user: MutexGuard<'_, User>, _: BridgeTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("bridge").unwrap_or(107);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_bridge(user: MutexGuard<'_, User>, _: BridgeToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("lobby").unwrap_or(106);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_to_casino(user: MutexGuard<'_, User>, _: CasinoTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("casino").unwrap_or(104);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_casino(user: MutexGuard<'_, User>, _: CasinoToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("lobby").unwrap_or(106);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_to_cafe(user: MutexGuard<'_, User>, _: CafeTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("cafe").unwrap_or(160);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_cafe(user: MutexGuard<'_, User>, _: CafeToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("lobby").unwrap_or(106);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn campship_down(user: MutexGuard<'_, User>, _: CampshipDownPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        let mapid = lock.name_to_id("campship_down").unwrap_or(150);
        lock.move_player(id, mapid).await?;
    }

    Ok(Action::Nothing)
}

pub async fn map_loaded(user: &mut User, _: MapLoadedPacket) -> HResult {
    let packet = protocol::unk19::LobbyMonitorPacket { video_id: 1 };
    user.send_packet(&Packet::LobbyMonitor(packet)).await?;
    let Some(character) = &mut user.character else {
        unreachable!("Character should be loaded here");
    };
    let inventory_packets = character.inventory.send(
        user.player_id,
        character.character.name.clone(),
        &*user.blockdata.item_attrs.read().await,
        user.text_lang,
    );
    let char_flags = character.flags.to_char_flags();
    for packet in inventory_packets {
        user.send_packet(&packet).await?;
    }
    if user.firstload {
        let flags = user.accountflags.to_account_flags();
        user.send_packet(&flags).await?;
        user.send_packet(&char_flags).await?;
    }

    user.send_packet(&Packet::LoadPAs(protocol::objects::LoadPAsPacket {
        receiver: protocol::ObjectHeader {
            id: user.player_id,
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        target: protocol::ObjectHeader {
            id: user.player_id,
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        levels: vec![1; 0xee],
        ..Default::default()
    }))
    .await?;
    // unlock controls?
    user.send_packet(&Packet::UnlockControls).await?;
    user.send_packet(&Packet::FinishLoading).await?;
    user.firstload = false;
    Ok(Action::Nothing)
}

pub async fn set_flag(user: &mut User, data: SetFlagPacket) -> HResult {
    match data.flag_type {
        FlagType::Account => user.accountflags.set(data.id as usize, data.value as u8),
        FlagType::Character => user
            .character
            .as_mut()
            .unwrap()
            .flags
            .set(data.id as usize, data.value as u8),
    }
    Ok(Action::Nothing)
}

pub async fn to_campship(user: MutexGuard<'_, User>, _: ToCampshipPacket) -> HResult {
    let party = user
        .get_current_party()
        .ok_or_else(|| Error::InvalidInput("to_campship"))?;
    let map = user
        .get_current_map()
        .ok_or_else(|| Error::InvalidInput("to_campship"))?;
    let player_id = user.get_user_id();
    drop(user);
    let quest_map = party
        .read()
        .await
        .get_quest_map()
        .ok_or_else(|| Error::InvalidInput("to_campship"))?;
    let player = map
        .lock()
        .await
        .remove_player(player_id)
        .await
        .ok_or_else(|| Error::InvalidInput("to_campship"))?;
    player.lock().await.set_map(quest_map.clone());
    quest_map.lock().await.init_add_player(player).await?;
    Ok(Action::Nothing)
}
