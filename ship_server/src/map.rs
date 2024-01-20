use crate::{
    mutex::{Mutex, MutexGuard},
    Error, User,
};
use data_structs::{map::MapData, SerDeFile as _};
use mlua::{Lua, LuaSerdeExt, StdLib};
use pso2packetlib::protocol::{
    self,
    flag::SkitItemAddRequestPacket,
    models::Position,
    objects::RemoveObjectPacket,
    playerstatus::SetPlayerIDPacket,
    server::MapTransferPacket,
    spawn::{CharacterSpawnPacket, CharacterSpawnType, ObjectSpawnPacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
    EntityType, ObjectHeader, Packet, PacketType,
};
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc, Weak,
};

type MapId = u32;
type PlayerId = u32;

pub struct Map {
    // lua is not `Send` so i've put it in a mutex
    // this mutex shouldn't block, because `Map` is under a mutex itself.
    lua: parking_lot::Mutex<Lua>,
    map_objs: Vec<(MapId, ObjectHeader)>,
    data: MapData,
    players: Vec<(PlayerId, MapId, Weak<Mutex<User>>)>,
    load_path: std::path::PathBuf,
    to_move: Vec<(PlayerId, MapId)>,
}
impl Map {
    pub fn new<T: AsRef<std::path::Path>>(path: T, map_obj_id: &AtomicU32) -> Result<Self, Error> {
        let data = MapData::load_from_mp_file(path.as_ref())?;
        let mut map = Self::new_from_data(data, map_obj_id)?;
        map.load_path = path.as_ref().to_owned();
        Ok(map)
    }
    pub fn new_from_data(data: MapData, map_obj_id: &AtomicU32) -> Result<Self, Error> {
        // will be increased as needed
        let lua_libs = StdLib::NONE;
        let mut map = Self {
            lua: Lua::new_with(lua_libs, mlua::LuaOptions::default())?.into(),
            map_objs: vec![],
            data,
            players: vec![],
            load_path: Default::default(),
            to_move: vec![],
        };
        let map_obj = ObjectHeader {
            id: map_obj_id.fetch_add(1, Ordering::Relaxed),
            entity_type: EntityType::Map,
            ..Default::default()
        };
        map.data.map_data.map_object = map_obj;
        let def_id = map.data.init_map;
        map.map_objs.push((def_id, map_obj));
        for settngs in &map.data.map_data.other_settings {
            map.map_objs.push((
                settngs.map_id,
                ObjectHeader {
                    id: map_obj_id.fetch_add(1, Ordering::Relaxed),
                    entity_type: EntityType::Map,
                    ..Default::default()
                },
            ))
        }
        map.init_lua()?;
        Ok(map)
    }
    fn init_lua(&mut self) -> Result<(), Error> {
        // default object handler
        for object in self.data.objects.iter() {
            let name: &str = &object.data.name;
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
            let name: &str = &npc.data.name;
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
                        packet_data[\"receiver\"] = packet[\"object3\"];
                        packet_data[\"target\"] = packet[\"object1\"];
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
    pub async fn reload_objs(&mut self) -> Result<(), Error> {
        if !self.load_path.exists() {
            return Ok(());
        }
        let map = MapData::load_from_mp_file(&self.load_path)?;
        exec_users(&self.players, 0, |_, _, mut player| {
            for obj in self.data.objects.iter() {
                let packet = Packet::RemoveObject(RemoveObjectPacket {
                    receiver: ObjectHeader {
                        id: player.get_user_id(),
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                    removed_object: obj.data.object,
                });
                let _ = player.send_packet(&packet);
            }
        })
        .await;
        self.data.objects = map.objects;
        self.data.luas = map.luas;
        self.init_lua()?;
        exec_users(&self.players, 0, |_, mapid, mut player| {
            let _ = Self::load_objects(&self.lua, &self.data, mapid, &mut player);
        })
        .await;
        Ok(())
    }
    pub fn name_to_id(&self, name: &str) -> Option<u32> {
        self.data.map_names.get(name).copied()
    }

    pub async fn init_add_player(&mut self, new_player: Arc<Mutex<User>>) -> Result<(), Error> {
        let mut np_lock = new_player.lock().await;
        np_lock.send_packet(&Packet::LoadLevel(self.data.map_data.clone()))?;
        drop(np_lock);
        self.add_player(new_player, self.data.init_map).await
    }
    pub async fn move_player(&mut self, id: PlayerId, mapid: MapId) -> Result<(), Error> {
        let Some(player) = self.remove_player(id).await else {
            return Err(Error::NoUserInMap(id, self.data.map_data.unk7.to_string()));
        };
        let mut settings = vec![self.data.map_data.settings.clone()];
        for map in &self.data.map_data.other_settings {
            settings.push(map.clone())
        }
        let Some(map) = settings.iter().find(|s| s.map_id == mapid) else {
            return Err(Error::NoMapInMapSet(
                mapid,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let Some((_, map_obj)) = self.map_objs.iter().find(|(m, _)| *m == map.map_id) else {
            return Err(Error::NoMapInMapSet(
                mapid,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let mut lock = player.lock().await;
        let pid = lock.get_user_id();
        lock.send_packet(&Packet::MapTransfer(MapTransferPacket {
            map: *map_obj,
            target: ObjectHeader {
                id: pid,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            settings: map.clone(),
        }))?;
        drop(lock);
        self.add_player(player, map.map_id).await
    }

    async fn add_player(
        &mut self,
        new_player: Arc<Mutex<User>>,
        mapid: MapId,
    ) -> Result<(), Error> {
        let mut other_equipment = Vec::with_capacity(self.players.len() * 2);
        let mut other_characters = Vec::with_capacity(self.players.len());
        for player in self
            .players
            .iter()
            .filter(|p| p.1 == mapid)
            .filter_map(|p| p.2.upgrade())
        {
            let p = player.lock().await;
            let pid = p.get_user_id();
            if p.character.is_some() {
                other_equipment.push(p.palette.send_change_palette(pid));
                other_equipment.push(p.palette.send_cur_weapon(pid, &p.inventory));
                other_equipment.push(p.inventory.send_equiped(pid));
                other_characters.push((p.character.clone().unwrap(), p.position, p.isgm));
            }
        }
        let mut np_lock = new_player.lock().await;
        np_lock.mapid = mapid;
        let np_id = np_lock.get_user_id();
        let new_character = np_lock.character.clone().ok_or(Error::NoCharacter)?;
        self.data.map_data.receiver.id = np_id;
        self.data.map_data.receiver.entity_type = EntityType::Player;
        np_lock.send_packet(&Packet::SetPlayerID(SetPlayerIDPacket {
            player_id: np_id,
            unk2: 4,
            ..Default::default()
        }))?;
        let pos = self
            .data
            .default_location
            .iter()
            .find(|(i, _)| *i == mapid)
            .map(|(_, p)| *p)
            .unwrap_or_default();
        np_lock.position = pos;
        let np_gm = np_lock.isgm as u32;
        np_lock.spawn_character(CharacterSpawnPacket {
            position: pos,
            character: new_character.clone(),
            is_me: CharacterSpawnType::Myself,
            gm_flag: np_gm,
            player_obj: ObjectHeader {
                id: np_id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            ..Default::default()
        })?;
        Self::load_objects(&self.lua, &self.data, mapid, &mut np_lock)?;
        for (character, position, isgm) in other_characters {
            let player_id = character.player_id;
            np_lock.spawn_character(CharacterSpawnPacket {
                position,
                is_me: CharacterSpawnType::Other,
                gm_flag: isgm as u32,
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
        drop(np_lock);
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.spawn_character(CharacterSpawnPacket {
                position: pos,
                is_me: CharacterSpawnType::Other,
                gm_flag: np_gm,
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
        })
        .await;
        self.players
            .push((np_id, mapid, Arc::downgrade(&new_player)));

        let Some(lua) = self.data.luas.get("on_player_load").cloned() else {
            return Ok(());
        };
        self.run_lua(np_id, mapid, &Packet::None, "on_player_load", &lua)
            .await?;
        Ok(())
    }
    pub async fn send_palette_change(&self, sender_id: PlayerId) -> Result<(), Error> {
        let Some((_, mapid, player)) = self.players.iter().find(|p| p.0 == sender_id) else {
            return Err(Error::NoUserInMap(
                sender_id,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let mapid = *mapid;
        let player = player.upgrade();
        if player.is_none() {
            return Err(Error::InvalidInput("send_palette_change"));
        }
        let new_eqipment = {
            let player = player.unwrap();
            let p = player.lock().await;
            (
                p.palette.send_change_palette(sender_id),
                p.palette.send_cur_weapon(sender_id, &p.inventory),
            )
        };
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.send_packet(&new_eqipment.0);
            let _ = player.send_packet(&new_eqipment.1);
        })
        .await;

        Ok(())
    }

    pub async fn send_movement(&self, packet: Packet, sender_id: PlayerId) {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == sender_id) else {
            return;
        };
        let mapid = *mapid;
        let mut out_packet = match packet {
            Packet::Movement(_) => packet,
            Packet::MovementEnd(mut data) => {
                if data.unk1.id == 0 && data.unk2.id != 0 {
                    data.unk1 = data.unk2;
                }
                Packet::MovementEnd(data)
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
                Packet::MovementActionServer(packet)
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
                Packet::ActionUpdateServer(packet)
            }
            _ => return,
        };
        exec_users(&self.players, mapid, |id, _, mut player| {
            if let Packet::MovementActionServer(ref mut data) = out_packet {
                data.receiver.id = player.get_user_id();
            } else if let Packet::ActionUpdateServer(ref mut data) = out_packet {
                data.receiver.id = player.get_user_id();
            }
            if id != sender_id {
                let _ = player.send_packet(&out_packet);
            }
        })
        .await;
    }

    pub async fn send_message(&self, mut packet: Packet, id: PlayerId) {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == id) else {
            return;
        };
        let mapid = *mapid;
        if let Packet::ChatMessage(ref mut data) = packet {
            data.object = ObjectHeader {
                id,
                entity_type: EntityType::Player,
                ..Default::default()
            };
        }
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.send_packet(&packet);
        })
        .await;
    }

    pub async fn send_sa(&self, data: SendSymbolArtPacket, id: PlayerId) {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == id) else {
            return;
        };
        let mapid = *mapid;
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
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.send_packet(&packet);
        })
        .await;
    }

    pub async fn remove_player(&mut self, id: PlayerId) -> Option<Arc<Mutex<User>>> {
        let Some((pos, _)) = self.players.iter().enumerate().find(|(_, p)| p.0 == id) else {
            return None;
        };
        let (_, mapid, rem_player) = self.players.swap_remove(pos);
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
        exec_users(&self.players, mapid, |_, _, mut player| {
            if let Packet::RemoveObject(data) = &mut packet {
                data.receiver.id = player.get_user_id();
                let _ = player.send_packet(&packet);
            }
        })
        .await;
        rem_player.upgrade()
    }
    fn load_objects(
        lua: &parking_lot::Mutex<Lua>,
        map_data: &MapData,
        mapid: MapId,
        user: &mut User,
    ) -> Result<(), Error> {
        let lua = lua.lock();
        for mut obj in map_data
            .objects
            .iter()
            .filter(|o| o.mapid == mapid)
            .cloned()
        {
            if user.packet_type == PacketType::Vita {
                let lua_code = map_data
                    .luas
                    .get(obj.data.name.as_str())
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let globals = lua.globals();
                globals.set("data", lua.to_value(&obj.data.data)?)?;
                globals.set("call_type", "to_vita")?;
                globals.set("size", obj.data.data.len())?;
                let chunk = lua.load(lua_code);
                chunk.exec()?;
                obj.data.data = lua.from_value(globals.get("data")?)?;
                globals.raw_remove("data")?;
                globals.raw_remove("call_type")?;
                globals.raw_remove("size")?;
            }
            user.send_packet(&Packet::ObjectSpawn(obj.data))?;
        }
        for npc in map_data.npcs.iter().filter(|o| o.mapid == mapid).cloned() {
            user.send_packet(&Packet::NPCSpawn(npc.data))?;
        }
        for event in map_data.events.iter().filter(|e| e.mapid == mapid).cloned() {
            user.send_packet(&Packet::EventSpawn(event.data))?;
        }
        for tele in map_data
            .transporters
            .iter()
            .filter(|t| t.mapid == mapid)
            .cloned()
        {
            user.send_packet(&Packet::TransporterSpawn(tele.data))?;
        }

        Ok(())
    }

    pub async fn interaction(
        &mut self,
        packet: protocol::objects::InteractPacket,
        sender_id: PlayerId,
    ) -> Result<(), Error> {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == sender_id) else {
            return Err(Error::NoUserInMap(
                sender_id,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let mapid = *mapid;
        let Some(lua_data) = self
            .data
            .objects
            .iter()
            .filter(|o| o.mapid == mapid)
            .map(|x| (x.data.object.id, &x.data.name))
            .chain(
                self.data
                    .npcs
                    .iter()
                    .filter(|o| o.mapid == mapid)
                    .map(|x| (x.data.object.id, &x.data.name)),
            )
            .find(|(id, _)| *id == packet.object1.id)
            .and_then(|(_, name)| self.data.luas.get(name.as_str()))
        else {
            return Ok(());
        };
        let lua_data = lua_data.clone();
        self.run_lua(sender_id, mapid, &packet, "interaction", &lua_data)
            .await?;
        Ok(())
    }
    pub async fn on_questwork(
        &mut self,
        player: PlayerId,
        packet: SkitItemAddRequestPacket,
    ) -> Result<(), Error> {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == player) else {
            return Err(Error::NoUserInMap(
                player,
                self.data.map_data.unk7.to_string(),
            ));
        };
        let mapid = *mapid;
        let Some(lua) = self.data.luas.get("on_questwork").cloned() else {
            return Ok(());
        };
        self.run_lua(player, mapid, &packet, "on_questwork", &lua)
            .await?;
        let to_move: Vec<_> = self.to_move.drain(..).collect();
        for (player, mapid) in to_move {
            self.move_player(player, mapid).await?;
        }
        Ok(())
    }
    pub fn get_close_objects<F>(&self, mapid: MapId, pred: F) -> Vec<ObjectSpawnPacket>
    where
        F: Fn(&Position) -> bool,
    {
        let mut obj = vec![];
        for self_obj in self.data.objects.iter().filter(|o| o.mapid == mapid) {
            if pred(&self_obj.data.position) {
                obj.push(self_obj.data.clone());
            }
        }

        obj
    }

    async fn run_lua<S: serde::Serialize + Sync>(
        &mut self,
        sender_id: PlayerId,
        mapid: MapId,
        packet: &S,
        call_type: &str,
        lua_data: &str,
    ) -> Result<(), Error> {
        let mut scheduled_move = vec![];
        let mut to_send = vec![];
        {
            let lua = self.lua.lock();
            let globals = lua.globals();
            let player_ids: Vec<_> = self.players.iter().map(|p| p.0).collect();
            globals.set("mapid", mapid)?;
            globals.set("packet", lua.to_value(&packet)?)?;
            globals.set("sender", sender_id)?;
            globals.set("players", player_ids)?;
            globals.set("call_type", call_type)?;
            lua.scope(|scope| {
                self.setup_scope(&globals, scope, mapid, &mut to_send, &mut scheduled_move)?;
                let chunk = lua.load(lua_data);
                chunk.exec()?;
                Ok(())
            })?;
            globals.raw_remove("packet")?;
            globals.raw_remove("sender")?;
            globals.raw_remove("players")?;
            globals.raw_remove("call_type")?;
            globals.raw_remove("mapid")?;
        }
        for (receiver, packet) in to_send {
            if let Some(p) = self
                .players
                .iter()
                .find(|p| p.0 == receiver)
                .and_then(|p| p.2.upgrade())
            {
                p.lock().await.send_packet(&packet)?;
            }
        }
        for (receiver, mapid) in scheduled_move {
            self.to_move.push((receiver, mapid));
        }
        Ok(())
    }

    fn setup_scope<'s>(
        &'s self,
        globals: &mlua::Table,
        scope: &mlua::Scope<'_, 's>,
        mapid: MapId,
        to_send: &'s mut Vec<(PlayerId, Packet)>,
        scheduled_move: &'s mut Vec<(PlayerId, MapId)>,
    ) -> Result<(), mlua::Error> {
        /* LUA FUNCTIONS */

        // send packet
        let send = scope.create_function_mut(|lua, (receiver, packet): (u32, mlua::Value)| {
            let packet: Packet = lua.from_value(packet)?;
            to_send.push((receiver, packet));
            Ok(())
        })?;
        globals.set("send", send)?;
        // get object data
        let get_object = scope.create_function(move |lua, id: u32| {
            let object = self
                .data
                .objects
                .iter()
                .filter(|o| o.mapid == mapid)
                .find(|obj| obj.data.object.id == id)
                .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
            lua.to_value(&object.data)
        })?;
        globals.set("get_object", get_object)?;
        // get npc data
        let get_npc = scope.create_function(move |lua, id: u32| {
            let object = self
                .data
                .npcs
                .iter()
                .filter(|o| o.mapid == mapid)
                .find(|obj| obj.data.object.id == id)
                .ok_or(mlua::Error::runtime("Couldn't find requested npc"))?;
            lua.to_value(&object.data)
        })?;
        globals.set("get_npc", get_npc)?;
        // get additional data
        let get_extra_data = scope.create_function(move |lua, id: u32| {
            let object = self
                .data
                .objects
                .iter()
                .filter(|o| o.mapid == mapid)
                .map(|x| (x.data.object.id, &x.lua_data))
                .chain(
                    self.data
                        .npcs
                        .iter()
                        .filter(|o| o.mapid == mapid)
                        .map(|x| (x.data.object.id, &x.lua_data)),
                )
                .find(|(obj_id, _)| *obj_id == id)
                .map(|(_, data)| data)
                .ok_or(mlua::Error::runtime("Couldn't find requested object"))?;
            let object: serde_json::Value = match object {
                Some(d) => serde_json::from_str(d)
                    .map_err(|e| mlua::Error::runtime(format!("serde_json error: {e}")))?,
                None => Default::default(),
            };
            lua.to_value(&object)
        })?;
        globals.set("get_extra_data", get_extra_data)?;
        // move player to another submap
        let move_player = scope.create_function_mut(|_, (receiver, mapid): (u32, u32)| {
            scheduled_move.push((receiver, mapid));
            Ok(())
        })?;
        globals.set("move_player", move_player)?;

        /* LUA FUNCTIONS END */
        Ok(())
    }
}

async fn exec_users<F>(users: &[(PlayerId, MapId, Weak<Mutex<User>>)], mapid: MapId, mut f: F)
where
    F: FnMut(PlayerId, MapId, MutexGuard<User>) + Send,
{
    for (id, user_mapid, user) in users
        .iter()
        .filter(|(_, m, _)| if mapid == 0 { true } else { *m == mapid })
        .filter_map(|(i, m, p)| p.upgrade().map(|p| (*i, *m, p)))
    {
        f(id, user_mapid, user.lock().await)
    }
}
