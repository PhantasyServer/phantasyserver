use super::HResult;
use crate::{create_attr_files, user::User, Action};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use pso2packetlib::protocol::{ChatArea, Packet};

pub fn send_chat(user: &mut User, packet: Packet) -> HResult {
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
            "!reload_map_lua" => return Ok(Action::MapLuaReload),
            "!map_gc" => {
                user.map.as_ref().map(|map| map.borrow().lua_gc_collect());
            }
            "!reload_items" => {
                let mul_progress = indicatif::MultiProgress::new();
                let (pc, vita) = create_attr_files(&mul_progress)?;
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
        return Ok(Action::SendMapMessage(packet));
    }
    Ok(Action::Nothing)
}
