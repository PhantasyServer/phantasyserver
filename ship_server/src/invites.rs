use crate::party::Party;
use parking_lot::RwLock;
use std::sync::Weak;

pub struct PartyInvite {
    pub id: u32,
    pub party: Weak<RwLock<Party>>,
    pub invite_time: u32,
}
