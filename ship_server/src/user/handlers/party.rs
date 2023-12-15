use super::HResult;
use crate::{Action, User};
use parking_lot::MutexGuard;
use pso2packetlib::protocol::party::{
    BusyState, ChatStatusPacket, NewPartySettingsPacket, TransferLeaderPacket,
};

pub fn transfer_leader(user: MutexGuard<User>, data: TransferLeaderPacket) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().change_leader(data.target)?;
    }
    Ok(Action::Nothing)
}

pub fn set_party_settings(user: MutexGuard<User>, data: NewPartySettingsPacket) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().set_settings(data)?;
    }
    Ok(Action::Nothing)
}

pub fn set_busy_state(user: MutexGuard<User>, data: BusyState) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        party.write().set_busy_state(data, id);
    }
    Ok(Action::Nothing)
}

pub fn set_chat_state(user: MutexGuard<User>, data: ChatStatusPacket) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        party.write().set_chat_status(data, id);
    }
    Ok(Action::Nothing)
}
