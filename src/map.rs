use crate::Error;
use pso2packetlib::protocol::{
    self,
    models::Position,
    server::LoadLevelPacket,
    spawn::{CharacterSpawnPacket, CharacterSpawnType, NPCSpawnPacket, ObjectSpawnPacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
    EntityType, ObjectHeader, Packet, SetPlayerIDPacket,
};
use std::io::Write;

pub struct Map {
    data: MapData,
    players: Vec<u32>,
}
impl Map {
    pub fn new<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        Ok(Self {
            data: MapData::load_from_mp_file(path)?,
            players: vec![],
        })
    }
    // called by block
    pub fn add_player(&mut self, players: &mut [crate::User], new_id: u32) -> Result<(), Error> {
        let other_characters: Vec<_> = players
            .iter()
            .filter(|p| self.players.contains(&p.player_id) && p.character.is_some())
            .map(|p| (p.character.clone().unwrap(), p.position))
            .collect();
        let new_player = players
            .iter_mut()
            .find(|p| p.player_id == new_id)
            .ok_or(Error::InvalidInput)?;
        self.data.map_data.receiver.id = new_id;
        self.data.map_data.receiver.entity_type = EntityType::Player;
        new_player.send_packet(&Packet::LoadLevel(self.data.map_data.clone()))?;
        new_player.send_packet(&Packet::SetPlayerID(SetPlayerIDPacket {
            player_id: new_id,
            unk2: 4,
            ..Default::default()
        }))?;
        let new_character = new_player.character.clone().ok_or(Error::NoCharacter)?;
        new_player.position = self.data.default_location;
        new_player.spawn_character(CharacterSpawnPacket {
            position: self.data.default_location,
            character: new_character.clone(),
            is_me: CharacterSpawnType::Myself,
            player_obj: ObjectHeader {
                id: new_id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            ..Default::default()
        })?;
        for object in &self.data.objects {
            new_player.send_packet(&Packet::ObjectSpawn(object.clone()))?;
        }
        for npc in &self.data.npcs {
            new_player.send_packet(&Packet::NPCSpawn(npc.clone()))?;
        }
        for (character, position) in other_characters {
            new_player.spawn_character(CharacterSpawnPacket {
                position,
                is_me: CharacterSpawnType::Other,
                player_obj: ObjectHeader {
                    id: character.player_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
                character,
                ..Default::default()
            })?;
        }
        let other_players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        for player in other_players {
            let _ = player.spawn_character(CharacterSpawnPacket {
                position: self.data.default_location,
                is_me: CharacterSpawnType::Other,
                player_obj: ObjectHeader {
                    id: new_character.player_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
                character: new_character.clone(),
                ..Default::default()
            });
        }
        self.players.push(new_id);
        Ok(())
    }
    // called by block
    pub fn send_movement(&self, players: &mut [crate::User], packet: Packet, sender_id: u32) {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id) && p.player_id != sender_id);
        match packet {
            Packet::Movement(_) => {
                for player in players {
                    let _ = player.send_packet(&packet);
                }
            }
            Packet::MovementEnd(mut data) => {
                if data.unk1.id == 0 && data.unk2.id != 0 {
                    data.unk1 = data.unk2;
                }
                let packet = Packet::MovementEnd(data);
                for player in players {
                    let _ = player.send_packet(&packet);
                }
            }
            Packet::MovementAction(data) => {
                let packet = protocol::objects::MovementActionServerPacket {
                    performer: data.performer,
                    receiver: ObjectHeader {
                        id: 0,
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                    unk3: data.unk3,
                    unk4: data.unk4,
                    unk5: data.unk5,
                    unk6: data.unk6,
                    action: data.action,
                    unk7: data.unk7,
                    unk8: data.unk8,
                    unk9: data.unk9,
                    unk10: data.unk10,
                };
                let mut packet = Packet::MovementActionServer(packet);
                for player in players {
                    if let Packet::MovementActionServer(ref mut data) = packet {
                        data.receiver.id = player.player_id;
                    }
                    let _ = player.send_packet(&packet);
                }
            }
            _ => {}
        }
    }
    // called by block
    pub fn send_message(&self, players: &mut [crate::User], mut packet: Packet, id: u32) {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        if let Packet::ChatMessage(ref mut data) = packet {
            data.object = ObjectHeader {
                id,
                entity_type: EntityType::Player,
                ..Default::default()
            };
        }
        for player in players {
            let _ = player.send_packet(&packet);
        }
    }
    // called by block
    pub fn send_sa(&self, players: &mut [crate::User], data: SendSymbolArtPacket, id: u32) {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        let packet = Packet::ReceiveSymbolArt(ReceiveSymbolArtPacket {
            object: ObjectHeader {
                id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            uuid: data.uuid,
            area: data.area,
            unk1: data.unk1,
            unk2: data.unk2,
            unk3: data.unk3,
        });
        for player in players {
            let _ = player.send_packet(&packet);
        }
    }
    // called by block
    pub fn remove_player(&mut self, players: &mut [crate::User], id: u32) {
        let Some((pos, _)) = self.players.iter().enumerate().find(|(_, &n)| n == id) else {return};
        self.players.swap_remove(pos);
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        let mut packet = Packet::RemoveObject(protocol::objects::RemoveObjectPacket {
            receiver: ObjectHeader {
                id: 0,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            removed_object: ObjectHeader {
                id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
        });
        for player in players {
            if let Packet::RemoveObject(data) = &mut packet {
                data.receiver.id = player.player_id;
                let _ = player.send_packet(&packet);
            }
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    map_data: LoadLevelPacket,
    objects: Vec<ObjectSpawnPacket>,
    npcs: Vec<NPCSpawnPacket>,
    default_location: Position,
}

impl MapData {
    pub fn load_from_mp_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::File::open(path)?;
        let map = rmp_serde::from_read(&data)?;
        Ok(map)
    }
    pub fn load_from_json_file<T: AsRef<std::path::Path>>(path: T) -> Result<Self, Error> {
        let data = std::fs::read_to_string(path)?;
        let map = serde_json::from_str(&data)?;
        Ok(map)
    }
    pub fn save_to_mp_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let mut file = std::fs::File::create(path)?;
        file.write_all(&rmp_serde::to_vec(self)?)?;
        Ok(())
    }
    pub fn save_to_json_file<T: AsRef<std::path::Path>>(&self, path: T) -> Result<(), Error> {
        let file = std::fs::File::create(path)?;
        serde_json::to_writer_pretty(file, self)?;
        Ok(())
    }
}
