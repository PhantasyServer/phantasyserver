use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::palette::{
    SetDefaultPAsPacket, SetPalettePacket, SetSubPalettePacket, UpdatePalettePacket,
    UpdateSubPalettePacket,
};

pub fn send_full_palette(user: &mut User) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character.palette.send_full_palette();
    user.send_packet(&packet)?;
    Ok(Action::Nothing)
}
pub async fn set_palette(mut user: MutexGuard<'_, User>, packet: SetPalettePacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    character.palette.set_palette(packet)?;
    send_palette_update(user).await?;
    Ok(Action::Nothing)
}

pub async fn update_palette(
    mut user: MutexGuard<'_, User>,
    packet: UpdatePalettePacket,
) -> HResult {
    {
        let user: &mut User = &mut user;
        let character = user.character.as_mut().unwrap();
        let out_packet = character
            .palette
            .update_palette(&mut character.inventory, packet)?;
        user.send_packet(&out_packet)?;
    }
    send_palette_update(user).await?;
    Ok(Action::Nothing)
}

pub fn update_subpalette(user: &mut User, packet: UpdateSubPalettePacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let out_packet = character.palette.update_subpalette(packet)?;
    user.send_packet(&out_packet)?;
    Ok(Action::Nothing)
}

pub fn set_subpalette(user: &mut User, packet: SetSubPalettePacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    character.palette.set_subpalette(packet)?;
    Ok(Action::Nothing)
}

pub fn set_default_pa(user: &mut User, packet: SetDefaultPAsPacket) -> HResult {
    let character = user.character.as_mut().unwrap();
    let packet = character.palette.set_default_pas(packet);
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
