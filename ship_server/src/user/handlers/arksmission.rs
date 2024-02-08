use super::HResult;
use crate::{user::User, Action};
use pso2packetlib::protocol::{missions, Packet};

pub async fn mission_list(user: &mut User) -> HResult {
    let mission = missions::Mission {
        mission_type: 5,
        start_date: 0,
        end_date: 0,
        unk4: 6040309,
        unk5: 4,
        completion_date: 1615045153,
        unk7: 0,
        unk8: 0,
        unk9: 0,
        unk10: 1,
        unk11: 1023,
        unk12: 0,
        unk13: 0,
        unk14: 1,
        unk15: 0,
    };
    let packet = missions::MissionListPacket {
        missions: vec![mission],
        daily_update: 1689272266,
        weekly_update: 1689273267,
        tier_update: 1689273267,
        unk1: 0,
    };
    user.send_packet(&Packet::MissionList(packet)).await?;
    Ok(Action::Nothing)
}
