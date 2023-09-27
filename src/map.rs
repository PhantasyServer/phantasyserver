use crate::Error;
use mlua::{Lua, LuaSerdeExt, StdLib};
use pso2packetlib::protocol::{
    self,
    models::Position,
    server::LoadLevelPacket,
    spawn::{CharacterSpawnPacket, CharacterSpawnType, NPCSpawnPacket, ObjectSpawnPacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
    EntityType, ObjectHeader, Packet, PacketType, SetPlayerIDPacket,
};
use std::{collections::HashMap, io::Write};

pub struct Map {
    lua: Lua,
    data: MapData,
    players: Vec<u32>,
    load_path: std::path::PathBuf,
}
impl Map {
    pub fn new<T: AsRef<std::path::Path>>(path: T, mapid: &mut u32) -> Result<Self, Error> {
        // will be increased as needed
        let lua_libs = StdLib::NONE;
        let mut map = Self {
            lua: Lua::new_with(lua_libs, mlua::LuaOptions::default())?,
            data: MapData::load_from_mp_file(path.as_ref())?,
            players: vec![],
            load_path: path.as_ref().to_owned(),
        };
        map.data.map_data.map_object = ObjectHeader {
            id: *mapid,
            entity_type: EntityType::Map,
            ..Default::default()
        };
        *mapid += 1;
        map.init_lua()?;
        Ok(map)
    }
    fn init_lua(&mut self) -> Result<(), Error> {
        self.lua
            .globals()
            .set("object_data", self.data.object_data.clone())?;
        // default object handler
        for object in self.data.objects.iter() {
            let name: &str = &object.name;
            if self.data.luas.contains_key(name) {
                continue;
            }
            self.data.luas.insert(
                name.to_owned(),
                "print(packet[\"object1\"][\"id\"], packet[\"action\"])".into(),
            );
        }
        // default npc handler
        for npc in self.data.npcs.iter() {
            let name: &str = &npc.name;
            if self.data.luas.contains_key(name) {
                continue;
            }
            self.data.luas.insert(
                name.to_owned(),
                "if packet[\"action\"] == \"READY\" then
                    local ready_data = {}; 
                    local packet_data = {};
                    packet_data[\"attribute\"] = \"FavsNeutral\";
                    packet_data[\"object1\"] = packet[\"object3\"];
                    packet_data[\"object2\"] = packet[\"object1\"];
                    packet_data[\"object3\"] = packet[\"object1\"];
                    ready_data[\"SetTag\"] = packet_data; 
                    send(sender, ready_data);
                    ready_data[\"SetTag\"][\"attribute\"] = \"AP\";
                    send(sender, ready_data);
                    else
                    print(packet[\"object1\"][\"id\"], packet[\"action\"]);
                    end"
                .into(),
            );
        }
        Ok(())
    }
    // called by block
    pub fn reload_lua(&mut self) -> Result<(), Error> {
        let map = MapData::load_from_mp_file(&self.load_path)?;
        self.data.luas = map.luas;
        self.data.object_data = map.object_data;
        self.init_lua()?;
        Ok(())
    }
    // called by player
    pub fn lua_gc_collect(&self) -> Result<(), Error> {
        self.lua.gc_collect()?;
        self.lua.gc_collect()?;
        Ok(())
    }
    pub fn get_mapid(&self) -> u32 {
        self.data.map_data.settings.map_id
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
            let mut obj = object.clone();
            if new_player.packet_type == PacketType::Vita {
                obj.data.remove(7);
            }
            new_player.send_packet(&Packet::ObjectSpawn(obj))?;
        }
        for npc in &self.data.npcs {
            new_player.send_packet(&Packet::NPCSpawn(npc.clone()))?;
        }
        for (character, position) in other_characters {
            let player_id = character.player_id;
            new_player.spawn_character(CharacterSpawnPacket {
                position,
                is_me: CharacterSpawnType::Other,
                player_obj: ObjectHeader {
                    id: player_id,
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
            Packet::ActionUpdate(data) => {
                let packet = protocol::objects::ActionUpdateServerPacket {
                    performer: data.performer,
                    unk2: data.unk2,
                    receiver: ObjectHeader {
                        id: 0,
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                };
                let mut packet = Packet::ActionUpdateServer(packet);
                for player in players {
                    if let Packet::ActionUpdateServer(ref mut data) = packet {
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
        let Some((pos, _)) = self.players.iter().enumerate().find(|(_, &n)| n == id) else {
            return;
        };
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
    // called by block
    pub fn interaction(
        &self,
        players: &mut [crate::User],
        packet: protocol::objects::InteractPacket,
        sender_id: u32,
    ) -> Result<(), Error> {
        let name = self
            .data
            .objects
            .iter()
            .map(|x| (x.object.id, &x.name))
            .chain(self.data.npcs.iter().map(|x| (x.object.id, &x.name)))
            .find(|(id, _)| *id == packet.object1.id)
            .map(|(_, name)| name.to_string())
            .ok_or(Error::InvalidInput)?;
        if !self.data.luas.contains_key(&name) {
            return Ok(());
        }
        let globals = self.lua.globals();
        globals.set("packet", self.lua.to_value(&packet)?)?;
        globals.set("sender", sender_id)?;
        globals.set("players", self.players.clone())?;
        let mut pending_packets: Vec<(u32, Packet)> = vec![];
        self.lua.scope(|scope| {
            /* LUA FUNCTIONS */

            // prepare packets for sending
            let send =
                scope.create_function_mut(|lua, (receiver, packet): (u32, mlua::Value)| {
                    let packet = lua.from_value(packet)?;
                    pending_packets.push((receiver, packet));
                    Ok(())
                })?;
            self.lua.globals().set("send", send)?;
            // get object data
            let get_object = scope.create_function(|lua, id: u32| {
                let object = self
                    .data
                    .objects
                    .iter()
                    .find(|obj| obj.object.id == id)
                    .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
                lua.to_value(object)
            })?;
            self.lua.globals().set("get_object", get_object)?;
            // get npc data
            let get_npc = scope.create_function(|lua, id: u32| {
                let object = self
                    .data
                    .npcs
                    .iter()
                    .find(|obj| obj.object.id == id)
                    .ok_or(mlua::Error::runtime("Couldn't find requested npc"))?;
                lua.to_value(object)
            })?;
            self.lua.globals().set("get_npc", get_npc)?;
            // get additional data
            let get_extra_data = scope.create_function(|lua, id: u32| {
                let object = self
                    .data
                    .object_data
                    .iter()
                    .find(|(&obj_id, _)| obj_id == id)
                    .map(|(_, data)| data)
                    .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
                let object: serde_json::Value = serde_json::from_str(object)
                    .map_err(|e| mlua::Error::runtime(format!("serde_json error: {e}")))?;
                lua.to_value(&object)
            })?;
            self.lua.globals().set("get_extra_data", get_extra_data)?;

            /* LUA FUNCTIONS END */

            let lua_data = self.data.luas.get(&name).unwrap();
            let chunk = self.lua.load(lua_data);
            chunk.exec()?;
            Ok(())
        })?;
        globals.raw_remove("packet")?;
        globals.raw_remove("sender")?;
        globals.raw_remove("players")?;
        for (id, packet) in pending_packets {
            let player = players
                .iter_mut()
                .find(|p| p.player_id == id)
                .ok_or(Error::InvalidInput)?;
            player.send_packet(&packet)?;
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    map_data: LoadLevelPacket,
    objects: Vec<ObjectSpawnPacket>,
    npcs: Vec<NPCSpawnPacket>,
    default_location: Position,
    luas: HashMap<String, String>,
    object_data: HashMap<u32, String>,
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
