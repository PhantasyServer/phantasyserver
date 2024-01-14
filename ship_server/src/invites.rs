use crate::{mutex::RwLock, party::Party};
use std::sync::Weak;

pub struct PartyInvite {
    pub id: u32,
    pub party: Weak<RwLock<Party>>,
    pub invite_time: u32,
}
