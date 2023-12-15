use super::HResult;
use crate::{Action, User};
use parking_lot::MutexGuard;
use pso2packetlib::protocol::{objects, Packet};

pub fn movement(mut user: MutexGuard<User>, packet: objects::MovementPacket) -> HResult {
    if let Some(n) = packet.rot_x {
        user.position.rot_x = n;
    }
    if let Some(n) = packet.rot_y {
        user.position.rot_y = n;
    }
    if let Some(n) = packet.rot_z {
        user.position.rot_z = n;
    }
    if let Some(n) = packet.rot_w {
        user.position.rot_w = n;
    }
    if let Some(n) = packet.cur_x {
        user.position.pos_x = n;
    }
    if let Some(n) = packet.cur_y {
        user.position.pos_y = n;
    }
    if let Some(n) = packet.cur_z {
        user.position.pos_z = n;
    }
    User::send_position(user, Packet::Movement(packet))
}

pub fn action(user: MutexGuard<User>, packet: objects::InteractPacket) -> HResult {
    let id = user.get_user_id();
    let map = user.get_current_map();
    drop(user);
    if let Some(map) = map {
        map.lock().interaction(packet, id)?;
    }
    Ok(Action::Nothing)
}
