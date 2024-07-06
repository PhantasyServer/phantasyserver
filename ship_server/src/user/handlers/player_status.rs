use pso2packetlib::protocol::playerstatus;
use crate::{mutex::MutexGuard, Action, User};
use super::HResult;


pub async fn deal_damage(user: MutexGuard<'_, User>, packet: playerstatus::DealDamagePacket) -> HResult {
    let map = user.get_current_map();
    drop(user);
    if let Some(map) = map {
        map.lock().await.deal_damage(packet).await?;
    }
    Ok(Action::Nothing)
}
