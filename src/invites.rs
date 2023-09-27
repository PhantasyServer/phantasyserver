use crate::party::Party;
use std::{cell::RefCell, rc::Rc};

pub struct PartyInvite {
    pub id: u32,
    pub party: Rc<RefCell<Party>>,
    pub invite_time: u32,
}
