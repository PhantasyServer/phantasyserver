use super::HResult;
use crate::{create_attr_files, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use parking_lot::MutexGuard;
use pso2packetlib::protocol::{ChatArea, Packet};

pub fn send_chat(mut user: MutexGuard<User>, packet: Packet) -> HResult {
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
                    format!("Couldn't gather memory info")
                };
                user.send_system_msg(&mem_data_msg)?;
            }
            "!reload_map_lua" => {
                user.map.as_ref().map(|map| map.lock().reload_lua());
            }
            "!map_gc" => {
                user.map.as_ref().map(|map| map.lock().lua_gc_collect());
            }
            "!reload_items" => {
                let mul_progress = indicatif::MultiProgress::new();
                let (pc, vita) =
                    MutexGuard::unlocked(&mut user, || create_attr_files(&mul_progress))?;
                let mut attrs = user.item_attrs.write();
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
            map.lock().send_message(packet, id)
        }
    }
    Ok(Action::Nothing)
}
