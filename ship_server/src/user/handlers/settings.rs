use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{
    settings::{LoadSettingsPacket, SaveSettingsPacket},
    Packet,
};

pub async fn settings_request(user: &mut User) -> HResult {
    let settings = user.sql.get_settings(user.player_id).await?;
    user.send_packet(&Packet::LoadSettings(LoadSettingsPacket { settings }))?;
    Ok(Action::Nothing)
}

pub async fn save_settings(user: &mut User, packet: SaveSettingsPacket) -> HResult {
    user.sql
        .save_settings(user.player_id, &packet.settings)
        .await?;
    Ok(Action::Nothing)
}
