use super::HResult;
use crate::{Action, User, mutex::MutexGuard, party};
use pso2packetlib::protocol::party::{
    AcceptInvitePacket, BusyState, ChatStatusPacket, KickMemberPacket, NewPartySettingsPacket,
    TransferLeaderPacket,
};

pub async fn transfer_leader(user: MutexGuard<'_, User>, data: TransferLeaderPacket) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().await.change_leader(data.target).await?;
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
        party.write().await.set_settings(data).await?;
    }
    Ok(Action::Nothing)
}

pub async fn set_busy_state(user: MutexGuard<'_, User>, data: BusyState) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        party.write().await.set_busy_state(data, id).await;
    }
    Ok(Action::Nothing)
}

pub async fn set_chat_state(user: MutexGuard<'_, User>, data: ChatStatusPacket) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        party.write().await.set_chat_status(data, id).await
    }
    Ok(Action::Nothing)
}

pub async fn kick_player(user: MutexGuard<'_, User>, data: KickMemberPacket) -> HResult {
    let party = user.get_current_party();
    let block = user.blockdata.clone();
    drop(user);
    if let Some(party) = party {
        party
            .write()
            .await
            .kick_player(data.member.id, &block)
            .await?;
    }
    Ok(Action::Nothing)
}

pub async fn leave_party(user: MutexGuard<'_, User>) -> HResult {
    let block_data = user.blockdata.clone();
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        let user = party
            .write()
            .await
            .remove_player(id)
            .await?
            .expect("User exists at this point");
        user.lock().await.party = None;
        let party_id = block_data
            .latest_partyid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        party::Party::init_player(user.clone(), party_id).await?;
    }
    Ok(Action::Nothing)
}

pub async fn disband_party(user: MutexGuard<'_, User>) -> HResult {
    let block_data = user.blockdata.clone();
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        let party_id = block_data
            .latest_partyid
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        party.write().await.disband_party(party_id).await?;
    }
    Ok(Action::Nothing)
}

pub async fn accept_invite(user: MutexGuard<'_, User>, packet: AcceptInvitePacket) -> HResult {
    let party = user.get_current_party();
    let id = user.get_user_id();
    drop(user);
    if let Some(party) = party {
        let user = party
            .write()
            .await
            .remove_player(id)
            .await?
            .expect("User exists at this point");
        party::Party::accept_invite(user, packet.party_object.id).await?;
    }
    Ok(Action::Nothing)
}

pub async fn abandon_quest(user: MutexGuard<'_, User>) -> HResult {
    let party = user.get_current_party();
    drop(user);
    if let Some(party) = party {
        party.write().await.abandon().await
    }
    Ok(Action::Nothing)
}

pub async fn send_invite(user: MutexGuard<'_, User>, invitee_id: u32) -> HResult {
    let conn_id = user.conn_id;
    let blockdata = user.blockdata.clone();

    drop(user);

    let clients = blockdata.clients.lock().await;
    let Some((_, inviter)) = clients
        .iter()
        .find(|(c_conn_id, _)| *c_conn_id == conn_id)
        .cloned()
    else {
        unreachable!();
    };

    for (_, client) in &*clients {
        if client.lock().await.get_user_id() == invitee_id {
            party::Party::send_invite(inviter.clone(), client.clone()).await?;
            break;
        }
    }

    Ok(Action::Nothing)
}
