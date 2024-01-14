use pso2packetlib::protocol::{
    flag::{AccountFlagsPacket, CharacterFlagsPacket},
    Packet,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flags {
    flags: Vec<u8>,
    params: Vec<u32>,
}
impl Flags {
    pub fn new() -> Self {
        Self {
            flags: vec![],
            params: vec![],
        }
    }
    pub fn set(&mut self, id: usize, val: u8) {
        let index = id / 8;
        let bit_index = (id % 8) as u8;
        if self.flags.len() < index + 1 {
            self.flags.resize(index + 1, 0);
        }
        let data = self.flags[index];
        self.flags[index] = set_bit(data, bit_index, val);
    }
    pub fn get(&self, id: usize) -> u8 {
        let index = id / 8;
        let bit_index = (id % 8) as u8;
        if self.flags.len() < index + 1 {
            return 0;
        }
        (self.flags[index] & 1 << bit_index) >> bit_index
    }
    pub fn set_param(&mut self, id: usize, val: u32) {
        if self.flags.len() < id + 1 {
            self.flags.resize(id + 1, 0);
        }
        self.params[id] = val;
    }
    pub fn get_param(&self, id: usize) -> u32 {
        if self.flags.len() < id + 1 {
            0
        } else {
            self.params[id]
        }
    }
    pub fn to_account_flags(&self) -> Packet {
        Packet::AccountFlags(AccountFlagsPacket {
            flags: self.flags.clone(),
            params: self.params.clone(),
        })
    }
    pub fn to_char_flags(&self) -> Packet {
        Packet::CharacterFlags(CharacterFlagsPacket {
            flags: self.flags.clone(),
            params: self.params.clone(),
        })
    }
}

fn set_bit(byte: u8, index: u8, val: u8) -> u8 {
    let byte = byte & !(1 << index);
    byte | (val << index)
}
