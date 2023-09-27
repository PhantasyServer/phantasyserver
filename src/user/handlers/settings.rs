use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{self, Packet, SaveSettingsPacket};

pub fn settings_request(user: &mut User) -> HResult {
    let sql_provider = user.sql.clone();
    let sql = sql_provider.read();
    let settings = sql.get_settings(user.player_id)?;
    user.send_packet(&Packet::LoadSettings(protocol::LoadSettingsPacket {
        settings,
    }))?;
    Ok(Action::Nothing)
}

pub fn save_settings(user: &mut User, packet: SaveSettingsPacket) -> HResult {
    let sql_provider = user.sql.clone();
    let mut sql = sql_provider.write();
    sql.save_settings(user.player_id, &packet.settings)?;
    Ok(Action::Nothing)
}
