use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{friends::FriendListRequestPacket, Flags, Packet, PacketHeader};

pub fn get_friends(user: &mut User, _: FriendListRequestPacket) -> HResult {
    let packet = serde_json::from_str(&std::fs::read_to_string("friend.json")?)?;
    user.send_packet(&Packet::FriendList(packet))?;
    let packet = Packet::Unknown((
        PacketHeader {
            id: 0x18,
            subid: 0x17,
            flag: Flags {
                packed: true,
                ..Default::default()
            },
        },
        vec![0, 0, 0, 0, 96, 57, 0, 0],
    ));
    user.send_packet(&packet)?;

    Ok(Action::Nothing)
}
