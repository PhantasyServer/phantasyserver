use super::HResult;
use crate::{Action, User};
use pso2packetlib::protocol::{self, Packet};

pub fn mission_pass_info(user: &mut User) -> HResult {
    let mut temp = [0u32; 47];
    temp[10] = 1;
    //2 - current tier
    //3 - current stars
    //6 - gold status
    //7 - over run
    //8 - already claimed
    let packet = protocol::MissionPassInfoPacket { unk: temp.into() };
    user.send_packet(&Packet::MissionPassInfo(packet))?;
    Ok(Action::Nothing)
}

pub fn mission_pass(user: &mut User) -> HResult {
    let packet = protocol::MissionPassPacket {
        unk1: 1,
        cur_season_id: 2,
        cur_season: "ew".to_string(),
        stars_per_tier: 333,
        tiers: 3,
        overrun_tiers: 10,
        total_tiers: 13,
        start_date: 6,
        end_date: 1689272266,
        catchup_start: 9,
        unk11: 10,
        cur_banner: "mp_banner_image_03".to_string(),
        price_per_tier: 11,
        gold_pass_price: 12,
        last_season_id: 1,
        last_season: "abc".to_string(),
        ..Default::default()
    };
    user.send_packet(&Packet::MissionPass(packet))?;
    Ok(Action::Nothing)
}
