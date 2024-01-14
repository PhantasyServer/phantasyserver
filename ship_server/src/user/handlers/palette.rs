use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::palette::{
    SetDefaultPAsPacket, SetPalettePacket, SetSubPalettePacket, UpdatePalettePacket,
    UpdateSubPalettePacket,
};

pub fn send_full_palette(user: &mut User) -> HResult {
    user.send_packet(&user.palette.send_full_palette())?;
    Ok(Action::Nothing)
}
pub async fn set_palette(mut user: MutexGuard<'_, User>, packet: SetPalettePacket) -> HResult {
    user.palette.set_palette(packet)?;
    send_palette_update(user).await?;
    Ok(Action::Nothing)
}

pub async fn update_palette(
    mut user: MutexGuard<'_, User>,
    packet: UpdatePalettePacket,
) -> HResult {
    {
        let user: &mut User = &mut user;
        let out_packet = user.palette.update_palette(&mut user.inventory, packet)?;
        user.send_packet(&out_packet)?;
    }
    send_palette_update(user).await?;
    Ok(Action::Nothing)
}

pub fn update_subpalette(user: &mut User, packet: UpdateSubPalettePacket) -> HResult {
    let out_packet = user.palette.update_subpalette(packet)?;
    user.send_packet(&out_packet)?;
    Ok(Action::Nothing)
}

pub fn set_subpalette(user: &mut User, packet: SetSubPalettePacket) -> HResult {
    user.palette.set_subpalette(packet)?;
    Ok(Action::Nothing)
}

pub fn set_default_pa(user: &mut User, packet: SetDefaultPAsPacket) -> HResult {
    let packet = user.palette.set_default_pas(packet);
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}

async fn send_palette_update(user: MutexGuard<'_, User>) -> Result<(), crate::Error> {
    let id = user.player_id;
    let map = user.map.clone();
    drop(user);
    if let Some(map) = map {
        map.lock().await.send_palette_change(id).await?;
    }
    Ok(())
}
