use pso2packetlib::protocol::{
    flag::{AccountFlagsPacket, CharacterFlagsPacket},
    Packet,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Flags {
    flags: Vec<u8>,
    params: Vec<u32>,
}
impl Flags {
    pub const fn new() -> Self {
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
        (self.flags[index] >> bit_index) & 1
    }
    pub fn set_param(&mut self, id: usize, val: u32) {
        if self.params.len() < id + 1 {
            self.params.resize(id + 1, 0);
        }
        self.params[id] = val;
    }
    pub fn get_param(&self, id: usize) -> u32 {
        if self.params.len() < id + 1 {
            0
        } else {
            self.params[id]
        }
    }
    pub fn to_account_flags(&self) -> Packet {
        Packet::AccountFlags(AccountFlagsPacket {
            flags: self.flags.clone().into(),
            params: self.params.clone().into(),
        })
    }
    pub fn to_char_flags(&self) -> Packet {
        Packet::CharacterFlags(CharacterFlagsPacket {
            flags: self.flags.clone().into(),
            params: self.params.clone().into(),
        })
    }
}

const fn set_bit(byte: u8, index: u8, val: u8) -> u8 {
    let byte = byte & !(1 << index);
    byte | ((val & 1) << index)
}

#[cfg(test)]
mod tests {
    use super::Flags;

    #[test]
    fn test_flags() {
        let mut flags = Flags::new();
        flags.set(10, 1);
        flags.set(20, 2);
        flags.set(30, 3);
        flags.set_param(10, 123);
        assert_eq!(flags.get(10), 1);
        assert_eq!(flags.get(20), 0);
        assert_eq!(flags.get(30), 1);
        assert_eq!(flags.get_param(10), 123);
    }
}
