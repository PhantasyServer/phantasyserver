use super::HResult;
use crate::{Action, User};
use parking_lot::MutexGuard;
use pso2packetlib::protocol::party::{
    BusyState, ChatStatusPacket, NewPartySettingsPacket, TransferLeaderPacket,
};

pub async fn transfer_leader(user: MutexGuard<'_, User>, data: TransferLeaderPacket) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        tokio::task::spawn_blocking(move || party.write().change_leader(data.target))
            .await
            .unwrap()?;
    }
    Ok(Action::Nothing)
}

pub async fn set_party_settings(
    user: MutexGuard<'_, User>,
    data: NewPartySettingsPacket,
) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        tokio::task::spawn_blocking(move || party.write().set_settings(data))
            .await
            .unwrap()?;
    }
    Ok(Action::Nothing)
}

pub async fn set_busy_state(user: MutexGuard<'_, User>, data: BusyState) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        tokio::task::spawn_blocking(move || party.write().set_busy_state(data, id))
            .await
            .unwrap();
    }
    Ok(Action::Nothing)
}

pub async fn set_chat_state(user: MutexGuard<'_, User>, data: ChatStatusPacket) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        tokio::task::spawn_blocking(move || party.write().set_chat_status(data, id))
            .await
            .unwrap();
    }
    Ok(Action::Nothing)
}
