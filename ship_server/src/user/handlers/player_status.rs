use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::playerstatus;

pub async fn deal_damage(
    user: MutexGuard<'_, User>,
    packet: playerstatus::DealDamagePacket,
) -> HResult {
    log::trace!("Got deal damage packet: {packet:?}");
    let map = user.get_current_map();
    let zone = user.zone_pos;
    drop(user);
    if let Some(map) = map {
        map.lock().await.deal_damage(zone, packet).await?;
    }
    Ok(Action::Nothing)
}
