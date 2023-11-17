use pso2packetlib::protocol::palette::{
    SetDefaultPAsPacket, SetPalettePacket, SetSubPalettePacket, UpdatePalettePacket,
    UpdateSubPalettePacket,
};

use super::HResult;
use crate::{Action, User};

pub fn send_full_palette(user: &mut User) -> HResult {
    user.send_packet(&user.palette.send_full_palette())?;
    Ok(Action::Nothing)
}
pub fn set_palette(user: &mut User, packet: SetPalettePacket) -> HResult {
    user.palette.set_palette(packet)?;
    Ok(Action::PaletteUpdate)
}

pub fn update_palette(user: &mut User, packet: UpdatePalettePacket) -> HResult {
    let out_packet = user.palette.update_palette(&mut user.inventory, packet)?;
    user.send_packet(&out_packet)?;
    Ok(Action::PaletteUpdate)
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
