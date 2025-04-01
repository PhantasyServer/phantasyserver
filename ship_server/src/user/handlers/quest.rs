use super::HResult;
use crate::{Action, User, mutex::MutexGuard, quests::PartyQuest};
use pso2packetlib::protocol::{
    Packet, PacketHeader,
    flag::{CutsceneEndPacket, SkitItemAddRequestPacket},
    questlist::{
        self, AcceptQuestPacket, AcceptStoryQuestPacket, MinimapRevealRequestPacket,
        NewUnlockedQuestsPacket, QuestCategoryRequestPacket, QuestDifficultyPacket,
        QuestDifficultyRequestPacket, UnlockedQuest,
    },
};

pub async fn counter_request(user: &mut User) -> HResult {
    let data = vec![
        0x78, 0x00, 0x78, 0x00, 0x00, 0x00, 0x04, 0x00, 0x64, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0xF4, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xAC, 0x0D,
        0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0xB8, 0x0B, 0x00, 0x00,
    ];
    user.send_packet(&Packet::Unknown((
        PacketHeader {
            id: 0x49,
            subid: 0x01,
            ..Default::default()
        },
        data,
    )))
    .await?;
    let data = vec![
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x04, 0x00, 0x00, 0x00, 0x00, 0x78, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00,
    ];
    user.send_packet(&Packet::Unknown((
        PacketHeader {
            id: 0x0E,
            subid: 0x65,
            ..Default::default()
        },
        data,
    )))
    .await?;
    let quests = user.blockdata.quests.clone();
    let char = user
        .character
        .as_mut()
        .expect("Character should be loaded at this moment");
    // 51 is the size of the array inside NewUnlockedQuestsPacket
    let max_unlocks = char.unlocked_quests_notif.len().min(51);
    let unlocks: Vec<_> = char
        .unlocked_quests_notif
        .drain(..max_unlocks)
        .filter_map(|id| quests.get_quest_by_nameid(id))
        .map(|q| UnlockedQuest {
            name_id: q.definition.name_id,
            quest_type: q.definition.quest_type,
            ..Default::default()
        })
        .collect();
    let packet = Packet::NewUnlockedQuests(NewUnlockedQuestsPacket {
        unlocks: unlocks.into(),
    });
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn avaliable_quests(
    user: &mut User,
    _: questlist::AvailableQuestsRequestPacket,
) -> HResult {
    let char = user
        .character
        .as_ref()
        .expect("Character should be loaded at this moment");
    let packet =
        Packet::AvailableQuests(user.blockdata.quests.get_availiable(&char.unlocked_quests));
    user.send_packet(&packet).await?;
    Ok(Action::Nothing)
}

pub async fn quest_category(user: &mut User, packet: QuestCategoryRequestPacket) -> HResult {
    let char = user
        .character
        .as_ref()
        .expect("Character should be loaded at this moment");
    let packet = user
        .blockdata
        .quests
        .get_category(packet.category, &char.unlocked_quests);
    user.send_packet(&Packet::QuestCategory(packet)).await?;
    user.send_packet(&Packet::QuestCategoryStopper).await?;

    Ok(Action::Nothing)
}

pub async fn quest_difficulty(user: &mut User, packet: QuestDifficultyRequestPacket) -> HResult {
    for quest in packet.quests {
        let diff = user.blockdata.quests.get_diff(quest.id);
        if let Some(packet) = diff {
            user.send_packet(&Packet::QuestDifficulty(QuestDifficultyPacket {
                quests: vec![packet],
            }))
            .await?;
        }
    }
    user.send_packet(&Packet::QuestDifficultyStopper).await?;
    Ok(Action::Nothing)
}

pub async fn set_quest(user: MutexGuard<'_, User>, packet: AcceptQuestPacket) -> HResult {
    let quest = user
        .blockdata
        .quests
        .get_quest(packet, &user.blockdata.latest_mapid)?;
    start_quest(user, quest).await
}

pub async fn questwork(user: MutexGuard<'_, User>, packet: SkitItemAddRequestPacket) -> HResult {
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .on_questwork(zone, playerid, packet)
            .await?;
    }

    Ok(Action::Nothing)
}

pub async fn cutscene_end(user: MutexGuard<'_, User>, packet: CutsceneEndPacket) -> HResult {
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .on_cutscene_end(zone, playerid, packet)
            .await?;
    }

    Ok(Action::Nothing)
}

pub async fn set_story_quest(
    user: MutexGuard<'_, User>,
    packet: AcceptStoryQuestPacket,
) -> HResult {
    let quest = user
        .blockdata
        .quests
        .get_story_quest(packet, &user.blockdata.latest_mapid)?;
    start_quest(user, quest).await
}

pub async fn start_quest(user: MutexGuard<'_, User>, quest: PartyQuest) -> HResult {
    let is_insta = quest.is_insta_transfer();
    let user_id = user.get_user_id();
    let old_map = user.get_current_map().expect("User should have a map");
    let map = quest.get_map();
    // we are the only owner of the map, so this never blocks
    map.lock_blocking().set_block_data(user.blockdata.clone());
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().await.set_quest(quest).await;
    }
    if is_insta {
        let mut lock = old_map.lock().await;
        let player = lock
            .remove_player(user_id)
            .await
            .expect("User should exist");
        drop(lock);
        player.lock().await.set_map(map.clone());
        let mut lock = map.lock().await;
        lock.init_add_player(player).await?;
    }
    Ok(Action::Nothing)
}

pub async fn minimap_reveal(
    mut user: MutexGuard<'_, User>,
    data: MinimapRevealRequestPacket,
) -> HResult {
    user.send_packet(&Packet::SystemMessage(
        pso2packetlib::protocol::unk19::SystemMessagePacket {
            message: format!(
                "Chunk ID: {}, {}:{}",
                data.chunk_id, data.map_row, data.map_column
            ),
            msg_type: pso2packetlib::protocol::unk19::MessageType::EventInformationYellow,
            ..Default::default()
        },
    ))
    .await?;
    if let Some(map) = user.get_current_map() {
        let playerid = user.get_user_id();
        let zone = user.zone_pos;
        drop(user);
        map.lock()
            .await
            .minimap_reveal(zone, playerid, data)
            .await?;
    }
    Ok(Action::Nothing)
}
