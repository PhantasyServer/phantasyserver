use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{Flags, Packet, PacketHeader, friends::FriendListRequestPacket};

pub async fn get_friends(user: &mut User, _: FriendListRequestPacket) -> HResult {
    let mut packet: pso2packetlib::protocol::friends::FriendListPacket =
        serde_json::from_str(&std::fs::read_to_string("data/friend.json")?)?;
    packet.nickname = user.user_data.nickname.clone();
    user.send_packet(&Packet::FriendList(packet)).await?;
    let packet = Packet::Unknown((
        PacketHeader {
            id: 0x18,
            subid: 0x17,
            flag: Flags::PACKED,
        },
        vec![0, 0, 0, 0, 96, 57, 0, 0],
    ));
    user.send_packet(&packet).await?;
    let packet = Packet::Unknown((
        PacketHeader {
            id: 0x18,
            subid: 0x2F,
            ..Default::default()
        },
        vec![2, 0, 0, 0],
    ));
    user.send_packet(&packet).await?;

    Ok(Action::Nothing)
}
