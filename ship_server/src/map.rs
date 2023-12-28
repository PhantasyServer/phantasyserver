use crate::{Error, User};
use data_structs::MapData;
use mlua::{Lua, LuaSerdeExt, StdLib};
use parking_lot::{Mutex, MutexGuard};
use pso2packetlib::protocol::{
    self,
    objects::RemoveObjectPacket,
    playerstatus::SetPlayerIDPacket,
    spawn::{CharacterSpawnPacket, CharacterSpawnType},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
    EntityType, ObjectHeader, Packet, PacketType,
};
use std::sync::{Arc, Weak};

pub struct Map {
    lua: Lua,
    data: MapData,
    // id, player
    players: Vec<(u32, Weak<Mutex<User>>)>,
    load_path: std::path::PathBuf,
}
impl Map {
    pub fn new<T: AsRef<std::path::Path>>(path: T, mapid: u32) -> Result<Self, Error> {
        // will be increased as needed
        let lua_libs = StdLib::NONE;
        let mut map = Self {
            lua: Lua::new_with(lua_libs, mlua::LuaOptions::default())?,
            data: MapData::load_from_mp_file(path.as_ref())?,
            players: vec![],
            load_path: path.as_ref().to_owned(),
        };
        map.data.map_data.map_object = ObjectHeader {
            id: mapid,
            entity_type: EntityType::Map,
            ..Default::default()
        };
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
                "if call_type == \"interaction\" then
                    print(packet[\"object1\"][\"id\"], packet[\"action\"])
                elseif call_type == \"to_vita\" then
	                for i=1,size,2 do
		                if data[i] > 50 and data[i] < 80 then
			                data[i] = data[i] - 1
		                end
	                end
                end"
                .into(),
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
                "if call_type == \"interaction\" then
                    if packet[\"action\"] == \"READY\" then
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
                    end
                end"
                .into(),
            );
        }
        Ok(())
    }
    pub fn reload_lua(&mut self) -> Result<(), Error> {
        let map = MapData::load_from_mp_file(&self.load_path)?;
        self.data.luas = map.luas;
        self.data.object_data = map.object_data;
        self.init_lua()?;
        Ok(())
    }
    pub fn reload_objs(&mut self) -> Result<(), Error> {
        let map = MapData::load_from_mp_file(&self.load_path)?;
        exec_users(&self.players, |_, mut player| {
            for obj in self.data.objects.iter() {
                let packet = Packet::RemoveObject(RemoveObjectPacket {
                    receiver: ObjectHeader {
                        id: player.player_id,
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                    removed_object: obj.object,
                });
                let _ = player.send_packet(&packet);
            }
        });
        self.data.objects = map.objects;
        exec_users(&self.players, |_, mut player| {
            let _ = Self::load_objects(&self.lua, &self.data, &mut player);
        });
        Ok(())
    }
    pub fn lua_gc_collect(&self) -> Result<(), Error> {
        self.lua.gc_collect()?;
        self.lua.gc_collect()?;
        Ok(())
    }
    pub fn get_mapid(&self) -> u32 {
        self.data.map_data.settings.map_id
    }
    // called by block
    pub fn add_player(&mut self, new_player: Arc<Mutex<User>>) -> Result<(), Error> {
        let mut other_equipment: Vec<_> = Vec::with_capacity(self.players.len() * 2);
        let other_characters: Vec<_> = self
            .players
            .iter()
            .filter_map(|p| p.1.upgrade())
            .filter_map(|p| {
                let p = p.lock();
                if p.character.is_none() {
                    None
                } else {
                    other_equipment.push(p.palette.send_change_palette(p.player_id));
                    other_equipment.push(p.palette.send_cur_weapon(p.player_id, &p.inventory));
                    other_equipment.push(p.inventory.send_equiped(p.player_id));
                    Some((p.character.clone().unwrap(), p.position))
                }
            })
            .collect();
        let mut np_lock = new_player.lock();
        let np_id = np_lock.player_id;
        let new_character = np_lock.character.clone().ok_or(Error::NoCharacter)?;
        self.data.map_data.receiver.id = np_id;
        self.data.map_data.receiver.entity_type = EntityType::Player;
        np_lock.send_packet(&Packet::LoadLevel(self.data.map_data.clone()))?;
        np_lock.send_packet(&Packet::SetPlayerID(SetPlayerIDPacket {
            player_id: np_id,
            unk2: 4,
            ..Default::default()
        }))?;
        np_lock.position = self.data.default_location;
        np_lock.spawn_character(CharacterSpawnPacket {
            position: self.data.default_location,
            character: new_character.clone(),
            is_me: CharacterSpawnType::Myself,
            player_obj: ObjectHeader {
                id: np_id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            ..Default::default()
        })?;
        Self::load_objects(&self.lua, &self.data, &mut np_lock)?;
        for npc in &self.data.npcs {
            np_lock.send_packet(&Packet::NPCSpawn(npc.clone()))?;
        }
        for (character, position) in other_characters {
            let player_id = character.player_id;
            np_lock.spawn_character(CharacterSpawnPacket {
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
        for equipment in other_equipment {
            np_lock.send_packet(&equipment)?;
        }
        let new_eqipment = (
            np_lock.palette.send_change_palette(np_id),
            np_lock.palette.send_cur_weapon(np_id, &np_lock.inventory),
            np_lock.inventory.send_equiped(np_id),
        );
        let palette_packet = np_lock.palette.send_palette();
        np_lock.send_packet(&palette_packet)?;
        np_lock.send_packet(&new_eqipment.0)?;
        np_lock.send_packet(&new_eqipment.1)?;
        // np_lock.send_packet(&new_eqipment.2)?;
        exec_users(&self.players, |_, mut player| {
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
            let _ = player.send_packet(&new_eqipment.0);
            let _ = player.send_packet(&new_eqipment.1);
            let _ = player.send_packet(&new_eqipment.2);
        });
        drop(np_lock);
        self.players.push((np_id, Arc::downgrade(&new_player)));
        Ok(())
    }
    pub fn send_palette_change(&self, sender_id: u32) -> Result<(), Error> {
        let new_eqipment = self
            .players
            .iter()
            .find(|p| p.0 == sender_id)
            .and_then(|p| p.1.upgrade())
            .map(|p| {
                let p = p.lock();
                (
                    p.palette.send_change_palette(sender_id),
                    p.palette.send_cur_weapon(sender_id, &p.inventory),
                )
            })
            .ok_or(Error::InvalidInput)?;
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&new_eqipment.0);
            let _ = player.send_packet(&new_eqipment.1);
        });

        Ok(())
    }
    // called by block
    pub fn send_movement(&self, packet: Packet, sender_id: u32) {
        match packet {
            Packet::Movement(_) => {
                exec_users(&self.players, |id, mut player| {
                    if id != sender_id {
                        let _ = player.send_packet(&packet);
                    }
                });
            }
            Packet::MovementEnd(mut data) => {
                if data.unk1.id == 0 && data.unk2.id != 0 {
                    data.unk1 = data.unk2;
                }
                let packet = Packet::MovementEnd(data);
                exec_users(&self.players, |id, mut player| {
                    if id != sender_id {
                        let _ = player.send_packet(&packet);
                    }
                });
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
                exec_users(&self.players, |id, mut player| {
                    if id != sender_id {
                        if let Packet::MovementActionServer(ref mut data) = packet {
                            data.receiver.id = player.player_id;
                        }
                        let _ = player.send_packet(&packet);
                    }
                });
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
                exec_users(&self.players, |id, mut player| {
                    if id != sender_id {
                        if let Packet::ActionUpdateServer(ref mut data) = packet {
                            data.receiver.id = player.player_id;
                        }
                        let _ = player.send_packet(&packet);
                    }
                });
            }
            _ => {}
        }
    }
    // called by block
    pub fn send_message(&self, mut packet: Packet, id: u32) {
        if let Packet::ChatMessage(ref mut data) = packet {
            data.object = ObjectHeader {
                id,
                entity_type: EntityType::Player,
                ..Default::default()
            };
        }
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&packet);
        });
    }
    // called by block
    pub fn send_sa(&self, data: SendSymbolArtPacket, id: u32) {
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
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&packet);
        });
    }
    // called by block
    pub fn remove_player(&mut self, id: u32) {
        let Some((pos, _)) = self.players.iter().enumerate().find(|(_, p)| p.0 == id) else {
            return;
        };
        self.players.swap_remove(pos);
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
        exec_users(&self.players, |_, mut player| {
            if let Packet::RemoveObject(data) = &mut packet {
                data.receiver.id = player.player_id;
                let _ = player.send_packet(&packet);
            }
        });
    }
    fn load_objects(lua: &Lua, map_data: &MapData, user: &mut User) -> Result<(), Error> {
        for mut obj in map_data.objects.iter().cloned() {
            if user.packet_type == PacketType::Vita {
                if let Some(lua_code) = map_data.luas.get(&obj.name.to_string()) {
                    let globals = lua.globals();
                    globals.set("data", lua.to_value(&obj.data)?)?;
                    globals.set("call_type", "to_vita")?;
                    globals.set("size", obj.data.len())?;
                    let chunk = lua.load(lua_code);
                    chunk.exec()?;
                    obj.data = lua.from_value(globals.get("data")?)?;
                    globals.raw_remove("data")?;
                    globals.raw_remove("call_type")?;
                    globals.raw_remove("size")?;
                }
            }
            user.send_packet(&Packet::ObjectSpawn(obj))?;
        }

        Ok(())
    }
    // called by block
    pub fn interaction(
        &self,
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
        let player_ids: Vec<_> = self.players.iter().map(|p| p.0).collect();
        globals.set("packet", self.lua.to_value(&packet)?)?;
        globals.set("sender", sender_id)?;
        globals.set("players", player_ids)?;
        globals.set("call_type", "interaction")?;
        self.lua.scope(|scope| {
            /* LUA FUNCTIONS */

            // send packet
            let send =
                scope.create_function_mut(|lua, (receiver, packet): (u32, mlua::Value)| {
                    let packet = lua.from_value(packet)?;
                    self.players
                        .iter()
                        .find(|p| p.0 == receiver)
                        .and_then(|p| p.1.upgrade())
                        .ok_or(mlua::Error::runtime("Couldn't find requested player"))
                        .and_then(|p| {
                            let mut player = p.lock();
                            player.send_packet(&packet).map_err(|e| {
                                mlua::Error::runtime(format!("Failed to send packet: {}", e))
                            })
                        })?;
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
        globals.raw_remove("call_type")?;
        Ok(())
    }
}

fn exec_users<F>(users: &[(u32, Weak<Mutex<User>>)], mut f: F)
where
    F: FnMut(u32, MutexGuard<User>),
{
    users
        .iter()
        .filter_map(|(i, p)| p.upgrade().map(|p| (*i, p)))
        .for_each(|(i, p)| f(i, p.lock()));
}
