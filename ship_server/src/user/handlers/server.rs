use super::HResult;
use crate::{Action, Error, User, UserState, mutex::MutexGuard, party};
use pso2packetlib::protocol::{
    self, Packet,
    flag::{FlagType, SetFlagPacket},
    server::{
        BridgeToLobbyPacket, BridgeTransportPacket, CafeToLobbyPacket, CafeTransportPacket,
        CampshipDownPacket, CasinoToLobbyPacket, CasinoTransportPacket, MapLoadedPacket,
        StoryToLobbyPacket, ToCampshipPacket,
    },
};
use std::sync::atomic::Ordering;

pub async fn initial_load(mut user: MutexGuard<'_, User>) -> HResult {
    let conn_id = user.conn_id;
    let blockdata = user.blockdata.clone();

    user.set_map(blockdata.lobby.clone());
    let party_id = blockdata.latest_partyid.fetch_add(1, Ordering::Relaxed);
    drop(user);

    let clients = blockdata.clients.lock().await;
    let Some((_, user)) = clients
        .iter()
        .find(|(c_conn_id, _)| *c_conn_id == conn_id)
        .cloned()
    else {
        unreachable!();
    };
    drop(clients);

    party::Party::init_player(user.clone(), party_id).await?;
    blockdata
        .lobby
        .lock()
        .await
        .init_add_player(user.clone())
        .await?;
    let mut user_lock = user.lock().await;
    user_lock.state = UserState::InGame;
    Ok(Action::Nothing)
}

pub async fn move_to_bridge(user: MutexGuard<'_, User>, _: BridgeTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "bridge").await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_bridge(user: MutexGuard<'_, User>, _: BridgeToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "lobby").await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_to_casino(user: MutexGuard<'_, User>, _: CasinoTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "casino").await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_casino(user: MutexGuard<'_, User>, _: CasinoToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "lobby").await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_to_cafe(user: MutexGuard<'_, User>, _: CafeTransportPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "cafe").await?;
    }

    Ok(Action::Nothing)
}

pub async fn move_from_cafe(user: MutexGuard<'_, User>, _: CafeToLobbyPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "lobby").await?;
    }

    Ok(Action::Nothing)
}

pub async fn campship_down(user: MutexGuard<'_, User>, _: CampshipDownPacket) -> HResult {
    let map = user.get_current_map();
    let id = user.get_user_id();
    drop(user);
    if let Some(map) = map {
        let mut lock = map.lock().await;
        lock.move_player_named(id, "campship_down").await?;
    }

    Ok(Action::Nothing)
}

pub async fn map_loaded(mut user_guard: MutexGuard<'_, User>, _: MapLoadedPacket) -> HResult {
    let user = &mut *user_guard;
    let user_id = user.get_user_id();
    let Some(character) = &mut user.character else {
        unreachable!("Character should be loaded here");
    };
    let inventory_packets = character.inventory.send(
        user_id,
        character.character.name.clone(),
        &user.blockdata.server_data.item_params,
        user.user_data.lang,
    );
    let palette = character.palette.send_palette();
    let default_pa_packet = character.palette.send_default_pa();
    let equiped = character.inventory.send_equiped(user_id);
    let change_palette = character.palette.send_change_palette(user_id);

    let char_flags = character.flags.to_char_flags();
    for packet in inventory_packets {
        user.send_packet(&packet).await?;
    }
    if user.firstload {
        let flags = user.user_data.accountflags.to_account_flags();
        user.send_packet(&flags).await?;
        user.send_packet(&char_flags).await?;
    }

    user.send_packet(&Packet::LoadPAs(protocol::objects::LoadPAsPacket {
        receiver: protocol::ObjectHeader {
            id: user_id,
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        target: protocol::ObjectHeader {
            id: user_id,
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        levels: vec![1; 0xee].into(),
        ..Default::default()
    }))
    .await?;

    user.send_packet(&palette).await?;
    user.send_packet(&default_pa_packet).await?;
    user.send_packet(&equiped).await?;
    user.send_packet(&change_palette).await?;
    // unlock controls?
    user.send_packet(&Packet::UnlockControls).await?;
    user.send_packet(&Packet::FinishLoading).await?;
    let packet = protocol::unk19::LobbyMonitorPacket { video_id: 121 };
    user.send_packet(&Packet::LobbyMonitor(packet)).await?;
    user.firstload = false;

    let map = user.map.clone().unwrap();
    let player_id = user.get_user_id();
    let zone = user.zone_pos;
    drop(user_guard);
    map.lock().await.on_map_loaded(zone, player_id).await?;

    Ok(Action::Nothing)
}

pub async fn set_flag(user: &mut User, data: SetFlagPacket) -> HResult {
    match data.flag_type {
        FlagType::Account => user
            .user_data
            .accountflags
            .set(data.id as usize, data.value as u8),
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
    let Some(party) = user.get_current_party() else {
        unreachable!("User should be in state >= 'PreInGame'");
    };
    let Some(map) = user.get_current_map() else {
        unreachable!("User should be in state >= 'PreInGame'");
    };
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

pub async fn move_from_story(user: MutexGuard<'_, User>, _: StoryToLobbyPacket) -> HResult {
    let Some(map) = user.get_current_map() else {
        unreachable!("User should be in state >= 'PreInGame'");
    };
    let lobby = user.blockdata.lobby.clone();
    let id = user.get_user_id();
    drop(user);
    let player = map
        .lock()
        .await
        .remove_player(id)
        .await
        .ok_or_else(|| Error::InvalidInput("move_from_story"))?;
    player.lock().await.set_map(lobby.clone());
    lobby.lock().await.init_add_player(player).await?;

    Ok(Action::Nothing)
}
