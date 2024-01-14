use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::{objects, Packet};

pub async fn movement(mut user: MutexGuard<'_, User>, packet: objects::MovementPacket) -> HResult {
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
    User::send_position(user, Packet::Movement(packet)).await
}

pub async fn action(user: MutexGuard<'_, User>, packet: objects::InteractPacket) -> HResult {
    let id = user.get_user_id();
    let map = user.get_current_map();
    drop(user);
    if let Some(map) = map {
        map.lock().await.interaction(packet, id).await?;
    }
    Ok(Action::Nothing)
}
