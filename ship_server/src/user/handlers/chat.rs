use super::HResult;
use crate::{create_attr_files, mutex::MutexGuard, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use pso2packetlib::protocol::{chat::ChatArea, flag::FlagType, Packet};

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
                user.send_system_msg(&mem_data_msg)?;
            }
            "!reload_map" => {
                if let Some(ref map) = user.map {
                    let map = map.clone();
                    drop(user);
                    map.lock().await.reload_objs().await?;
                }
            }
            "!start_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No concert name provided")?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    attribute: format!("Start({name})").into(),
                    ..Default::default()
                });
                user.send_packet(&packet)?;
            }
            "!send_con" => {
                let name = args.next();
                if name.is_none() {
                    user.send_system_msg("No action provided")?;
                    return Ok(Action::Nothing);
                }
                let name = name.unwrap();
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: pso2packetlib::protocol::EntityType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    attribute: name.into(),
                    ..Default::default()
                });
                user.send_packet(&packet)?;
            }
            "!get_pos" => {
                let pos = user.position;
                let pos: pso2packetlib::protocol::models::EulerPosition = pos.into();
                user.send_system_msg(&format!("{pos:?}"))?;
            }
            "!get_close_obj" => {
                let dist = args.next().and_then(|n| n.parse().ok()).unwrap_or(1.0);
                let map = user.get_current_map();
                if map.is_none() {
                    return Ok(Action::Nothing);
                }
                let mapid = user.mapid;
                let map = map.unwrap();
                let lock = map.lock().await;
                let objs = lock.get_close_objects(mapid, |p| user.position.dist_2d(p) < dist);
                let user_pos = user.position;
                for obj in objs {
                    user.send_system_msg(&format!(
                        "Id: {}, Name: {}, Dist: {}",
                        obj.object.id,
                        obj.name,
                        user_pos.dist_2d(&obj.position)
                    ))?;
                }
            }
            "!reload_items" => {
                let mul_progress = indicatif::MultiProgress::new();
                let (pc, vita) = {
                    tokio::task::spawn_blocking(move || create_attr_files(&mul_progress))
                        .await
                        .unwrap()?
                };
                let mut attrs = user.blockdata.item_attrs.write().await;
                attrs.pc_attrs = pc;
                attrs.vita_attrs = vita;
                drop(attrs);
                user.send_item_attrs().await?;
                user.send_system_msg("Done!")?;
            }
            "!set_acc_flag" => set_flag(&mut user, FlagType::Account, &mut args)?,
            "!set_char_flag" => set_flag(&mut user, FlagType::Character, &mut args)?,
            _ => user.send_system_msg("Unknown command")?,
        }
        return Ok(Action::Nothing);
    }
    if let ChatArea::Map = data.area {
        let id = user.player_id;
        let map = user.map.clone();
        drop(user);
        if let Some(map) = map {
            map.lock().await.send_message(packet, id).await;
        }
    }
    Ok(Action::Nothing)
}

fn set_flag<'a>(
    user: &mut User,
    ftype: FlagType,
    args: &mut impl Iterator<Item = &'a str>,
) -> Result<(), crate::Error> {
    let range = match args.next() {
        Some(r) => r,
        None => {
            user.send_system_msg("No range provided")?;
            return Ok(());
        }
    };
    let val = args.next().and_then(|a| a.parse().ok()).unwrap_or(0);
    if range.contains('-') {
        let mut split = range.split('-');
        let lower = split.next().and_then(|r| r.parse().ok());
        let upper = split.next().and_then(|r| r.parse().ok());
        if lower.is_none() || upper.is_none() {
            user.send_system_msg("Invalid range")?;
            return Ok(());
        }
        let lower = lower.unwrap();
        let upper = upper.unwrap();
        if lower > upper {
            user.send_system_msg("Invalid range")?;
            return Ok(());
        }
        for i in lower..=upper {
            match ftype {
                FlagType::Account => user.accountflags.set(i, val),
                FlagType::Character => user.charflags.set(i, val),
            };
            user.send_packet(&Packet::ServerSetFlag(
                pso2packetlib::protocol::flag::ServerSetFlagPacket {
                    flag_type: ftype,
                    id: i as u32,
                    value: val as u32,
                    ..Default::default()
                },
            ))?;
        }
    } else {
        let id = match range.parse() {
            Ok(i) => i,
            Err(_) => {
                user.send_system_msg("Invalid id")?;
                return Ok(());
            }
        };
        match ftype {
            FlagType::Account => user.accountflags.set(id, val),
            FlagType::Character => user.charflags.set(id, val),
        };
        user.send_packet(&Packet::ServerSetFlag(
            pso2packetlib::protocol::flag::ServerSetFlagPacket {
                flag_type: ftype,
                id: id as u32,
                value: val as u32,
                ..Default::default()
            },
        ))?;
    }
    Ok(())
}
