use super::HResult;
use crate::{mutex::MutexGuard, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use pso2packetlib::protocol::{
    chat::MessageChannel, flag::FlagType, items::ItemId, playerstatus, ObjectType, Packet,
};

pub async fn send_chat(mut user: MutexGuard<'_, User>, packet: Packet) -> HResult {
    let Packet::ChatMessage(ref data) = packet else {
        unreachable!()
    };
    if data.message.starts_with('!') {
        let mut args = data.message.split(' ');
        let cmd = args.next().expect("Should always contain some data");
        match cmd {
            "!mem" => {
                let mem_data_msg = if let Some(mem) = memory_stats() {
                    format!(
                        "Physical memory: {}\nVirtual memory: {}",
                        HumanBytes(mem.physical_mem as u64),
                        HumanBytes(mem.virtual_mem as u64),
                    )
                } else {
                    "Couldn't gather memory info".into()
                };
                user.send_system_msg(&mem_data_msg).await?;
            }
            "!start_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No concert name provided").await?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    attribute: format!("Start({name})").into(),
                    ..Default::default()
                });
                user.send_packet(&packet).await?;
            }
            "!start_cutscene" => {
                let Some(name) = args.next() else {
                    user.send_system_msg("No cutscene name provided").await?;
                    return Ok(Action::Nothing);
                };
                user.send_packet(&Packet::StartCutscene(
                    pso2packetlib::protocol::questlist::StartCutscenePacket {
                        scene_name: name.to_string().into(),
                        ..Default::default()
                    },
                ))
                .await?;
            }
            "!send_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No action provided").await?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    attribute: name.into(),
                    ..Default::default()
                });
                user.send_packet(&packet).await?;
            }
            "!get_pos" => {
                let pos = user.position;
                let pos: pso2packetlib::protocol::models::EulerPosition = pos.into();
                user.send_system_msg(&format!("{pos:?}")).await?;
            }
            "!get_close_obj" => {
                let dist = args.next().and_then(|n| n.parse().ok()).unwrap_or(1.0);
                let Some(map) = user.get_current_map() else {
                    unreachable!("User should be in state >= `InGame`")
                };
                let mapid = user.mapid;
                let lock = map.lock().await;
                let objs = lock.get_close_objects(mapid, |p| user.position.dist_2d(p) < dist);
                let user_pos = user.position;
                for obj in objs {
                    user.send_system_msg(&format!(
                        "Id: {}, Name: {}, Dist: {}",
                        obj.object.id,
                        obj.name,
                        user_pos.dist_2d(&obj.position)
                    ))
                    .await?;
                }
            }
            "!set_acc_flag" => set_flag_parse(&mut user, FlagType::Account, &mut args).await?,
            "!set_char_flag" => set_flag_parse(&mut user, FlagType::Character, &mut args).await?,
            "!add_item" => {
                let Some(item_type) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No item type provided").await?;
                    return Ok(Action::Nothing);
                };
                let Some(id) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No id provided").await?;
                    return Ok(Action::Nothing);
                };
                let Some(subid) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No subid provided").await?;
                    return Ok(Action::Nothing);
                };
                let item_id = ItemId {
                    id,
                    subid,
                    item_type,
                    ..Default::default()
                };
                let user: &mut User = &mut user;
                let character = user.character.as_mut().unwrap();
                let packet = character
                    .inventory
                    .add_default_item(&mut user.uuid, item_id);
                user.send_packet(&packet).await?;
            }
            "!change_lvl" => {
                let Some(level) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No level provided").await?;
                    return Ok(Action::Nothing);
                };
                let Some(exp) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No EXP provided").await?;
                    return Ok(Action::Nothing);
                };
                let Some(char) = user.character.as_mut() else {
                    user.send_system_msg("No character loaded").await?;
                    return Ok(Action::Nothing);
                };
                let stats = char.character.get_level_mut();
                let diff = (exp as i64 - stats.exp as i64).abs();
                stats.level1 = level;
                stats.exp = exp;
                let stats = char.character.get_level();
                let stats2 = char.character.get_sublevel();
                let userexp = playerstatus::EXPReceiver {
                    unk1: 1,
                    unk2: 1,
                    gained: diff as _,
                    total: stats.exp as _,
                    level2: stats.level2,
                    level: stats.level1,
                    gained_sub: 0,
                    total_sub: stats2.exp as _,
                    level2_sub: stats2.level2,
                    level_sub: stats2.level1,
                    class: char.character.classes.main_class,
                    subclass: char.character.classes.sub_class,
                    object: user.create_object_header(),
                    ..Default::default()
                };
                let packet = Packet::GainedEXP(playerstatus::GainedEXPPacket {
                    sender: Default::default(),
                    receivers: vec![userexp],
                });
                user.send_packet(&packet).await?;
            }
            "!calc_stats" => {
                let msg = format!("Stats: {:?}", user.battle_stats);
                user.send_system_msg(&msg).await?;
            }
            "!force_quest" => {
                let Some(quest_id) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No quest id provided").await?;
                    return Ok(Action::Nothing);
                };
                let Some(diff) = args.next().and_then(|a| a.parse().ok()) else {
                    user.send_system_msg("No difficulty provided").await?;
                    return Ok(Action::Nothing);
                };
                let packet = pso2packetlib::protocol::questlist::AcceptQuestPacket {
                    quest_obj: pso2packetlib::protocol::ObjectHeader {
                        id: quest_id,
                        entity_type: ObjectType::Quest,
                        ..Default::default()
                    },
                    diff,
                    ..Default::default()
                };
                super::quest::set_quest(user, packet).await?;
            }
            "!spawn_enemy" => {
                let Some(name) = args.next() else {
                    user.send_system_msg("No enemy name provided").await?;
                    return Ok(Action::Nothing);
                };
                let map_id = user.get_map_id();
                let map = user.get_current_map().unwrap();
                let pos = user.position.clone();
                drop(user);
                map.lock().await.spawn_enemy(name, pos, map_id).await?;
            }
            _ => user.send_system_msg("Unknown command").await?,
        }
        return Ok(Action::Nothing);
    }
    let id = user.player_id;
    match data.channel {
        MessageChannel::Map => {
            let map = user.get_current_map();
            drop(user);
            if let Some(map) = map {
                map.lock().await.send_message(packet, id).await;
            }
        }
        MessageChannel::Party => {
            let party = user.get_current_party();
            drop(user);
            if let Some(party) = party {
                party.read().await.send_message(packet, id).await;
            }
        }
        _ => {}
    }
    Ok(Action::Nothing)
}

async fn set_flag_parse<'a>(
    user: &mut User,
    ftype: FlagType,
    args: &mut (impl Iterator<Item = &'a str> + Send),
) -> Result<(), crate::Error> {
    let range = match args.next() {
        Some(r) => r,
        None => {
            user.send_system_msg("No range provided").await?;
            return Ok(());
        }
    };
    let val = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    if range.contains('-') {
        let mut split = range.split('-');
        let lower = split.next().and_then(|r| r.parse().ok());
        let upper = split.next().and_then(|r| r.parse().ok());
        let (Some(lower), Some(upper)) = (lower, upper) else {
            user.send_system_msg("Invalid range").await?;
            return Ok(());
        };
        if lower > upper {
            user.send_system_msg("Invalid range").await?;
            return Ok(());
        }
        for i in lower..=upper {
            set_flag(user, ftype, i, val).await?;
        }
    } else {
        let id = match range.parse() {
            Ok(i) => i,
            Err(_) => {
                user.send_system_msg("Invalid id").await?;
                return Ok(());
            }
        };
        set_flag(user, ftype, id, val).await?;
    }
    Ok(())
}

async fn set_flag(
    user: &mut User,
    ftype: FlagType,
    id: usize,
    val: u8,
) -> Result<(), crate::Error> {
    let character = user.character.as_mut().unwrap();
    match ftype {
        FlagType::Account => user.accountflags.set(id, val),
        FlagType::Character => character.flags.set(id, val),
    };
    user.send_packet(&Packet::ServerSetFlag(
        pso2packetlib::protocol::flag::ServerSetFlagPacket {
            flag_type: ftype,
            id: id as u32,
            value: val as u32,
            ..Default::default()
        },
    ))
    .await?;

    Ok(())
}
