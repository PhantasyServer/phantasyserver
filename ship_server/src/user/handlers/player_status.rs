use super::HResult;
use crate::{mutex::MutexGuard, Action, User};
use pso2packetlib::protocol::playerstatus;

pub async fn deal_damage(
    user: MutexGuard<'_, User>,
    packet: playerstatus::DealDamagePacket,
) -> HResult {
    let map = user.get_current_map();
    drop(user);
    if let Some(map) = map {
        map.lock().await.deal_damage(packet).await?;
    }
    Ok(Action::Nothing)
}
