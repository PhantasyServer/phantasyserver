use super::HResult;
use crate::{async_lock, async_write, create_attr_files, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use parking_lot::MutexGuard;
use pso2packetlib::protocol::{chat::ChatArea, Packet};

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
            "!reload_map_lua" => {
                if let Some(ref map) = user.map {
                    async_lock(map).await.reload_lua()?;
                }
            }
            "!map_gc" => {
                if let Some(ref map) = user.map {
                    async_lock(map).await.lua_gc_collect()?;
                }
            }
            "!reload_map" => {
                if let Some(ref map) = user.map {
                    let map = map.clone();
                    drop(user);
                    tokio::task::spawn_blocking(move || map.lock().reload_objs())
                        .await
                        .unwrap()?
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
                    object1: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    object2: pso2packetlib::protocol::ObjectHeader {
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
                    object1: pso2packetlib::protocol::ObjectHeader {
                        id: user.player_id,
                        entity_type: pso2packetlib::protocol::EntityType::Player,
                        ..Default::default()
                    },
                    object2: pso2packetlib::protocol::ObjectHeader {
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
                let dist = args.next().unwrap_or("1.0").parse().unwrap_or(1.0);
                let map = user.get_current_map();
                if map.is_none() {
                    return Ok(Action::Nothing);
                }
                let map = map.unwrap();
                let lock = async_lock(&map).await;
                let objs = lock.get_close_objects(&user, dist);
                let user_pos = user.position;
                for obj in objs {
                    user.send_system_msg(&format!(
                        "Id: {}, Name: {}, Dist: {}",
                        obj.object.id,
                        obj.name,
                        user_pos.dist(&obj.position)
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
                let mut attrs = async_write(&user.blockdata.item_attrs).await;
                attrs.pc_attrs = pc;
                attrs.vita_attrs = vita;
                drop(attrs);
                user.send_item_attrs()?;
                user.send_system_msg("Done!")?;
            }
            _ => user.send_system_msg("Unknown command")?,
        }
        return Ok(Action::Nothing);
    }
    if let ChatArea::Map = data.area {
        let id = user.player_id;
        let map = user.map.clone();
        drop(user);
        if let Some(map) = map {
            tokio::task::spawn_blocking(move || map.lock().send_message(packet, id))
                .await
                .unwrap();
        }
    }
    Ok(Action::Nothing)
}
