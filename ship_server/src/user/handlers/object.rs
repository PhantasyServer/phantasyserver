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
    let zone = user.zone_pos;
    drop(user);
    if let Some(map) = map {
        map.lock().await.interaction(zone, packet, id).await?;
    }
    Ok(Action::Nothing)
}

pub async fn change_class(
    mut user: MutexGuard<'_, User>,
    packet: objects::ChangeClassRequestPacket,
) -> HResult {
    let Some(char) = user.character.as_mut() else {
        unreachable!("Character should be already setup");
    };
    char.character.classes.main_class = packet.main_class;
    char.character.classes.sub_class = packet.sub_class;

    let packet = Packet::ChangeClass(objects::ChangeClassPacket {
        new_info: char.character.classes.clone(),
        receiver: user.create_object_header(),
        player: user.create_object_header(),
        unk3: Default::default(),
    });
    user.send_packet(&packet).await?;
    let packet = Packet::Unk042C(objects::Unk042CPacket {
        unk1: user.create_object_header(),
        unk2: user.create_object_header(),
        unk3: 0x20,
        unk4: 0x1,
        ..Default::default()
    });
    user.send_packet(&packet).await?;

    Ok(Action::Nothing)
}
