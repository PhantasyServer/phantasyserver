use crate::{
    battle_stats::{BattleResult, EnemyStats},
    mutex::{Mutex, MutexGuard},
    BlockData, Error, User,
};
use data_structs::map::MapData;
use mlua::{Lua, LuaSerdeExt, StdLib};
use pso2packetlib::protocol::{
    self,
    flag::SkitItemAddRequestPacket,
    models::Position,
    objects::EnemyActionPacket,
    playerstatus::{DealDamagePacket, GainedEXPPacket, SetPlayerIDPacket},
    server::MapTransferPacket,
    spawn::{CharacterSpawnPacket, CharacterSpawnType, ObjectSpawnPacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
    ObjectHeader, ObjectType, Packet, PacketType,
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
    to_move: Vec<(PlayerId, MapId)>,
    max_id: u32,
    block_data: Option<Arc<BlockData>>,
    enemies: Vec<(u32, MapId, EnemyStats)>,
    enemy_level: u32,
}
impl Map {
    pub fn new_from_data(data: MapData, map_obj_id: &AtomicU32) -> Result<Self, Error> {
        // will be increased as needed
        let lua_libs = StdLib::NONE;
        let mut map = Self {
            lua: Lua::new_with(lua_libs, mlua::LuaOptions::default())?.into(),
            map_objs: vec![],
            data,
            players: vec![],
            to_move: vec![],
            max_id: 0,
            block_data: None,
            enemies: vec![],
            enemy_level: 0,
        };
        let map_obj = ObjectHeader {
            id: map_obj_id.fetch_add(1, Ordering::Relaxed),
            entity_type: ObjectType::Map,
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
                    entity_type: ObjectType::Map,
                    ..Default::default()
                },
            ))
        }
        map.init_lua()?;
        map.find_max_id();
        Ok(map)
    }
    pub fn set_block_data(&mut self, data: Arc<BlockData>) {
        self.block_data = Some(data);
    }
    pub fn set_enemy_level(&mut self, level: u32) {
        self.enemy_level = level;
    }
    fn find_max_id(&mut self) {
        let obj_max = self
            .data
            .objects
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let npc_max = self
            .data
            .npcs
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let event_max = self
            .data
            .events
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        let transporter_max = self
            .data
            .transporters
            .iter()
            .map(|o| o.data.object.id)
            .max()
            .unwrap_or(0);
        self.max_id = obj_max.max(npc_max).max(event_max).max(transporter_max) + 1;
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
                    print(packet.object1.id, packet.action)
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
                    if packet.action == \"READY\" then
                        local ready_data = {}; 
                        local packet_data = {};
                        packet_data.attribute = \"FavsNeutral\";
                        packet_data.receiver = packet.object3;
                        packet_data.target = packet.object1;
                        packet_data.object3 = packet.object1;
                        ready_data.SetTag = packet_data; 
                        send(sender, ready_data);
                        ready_data.SetTag.attribute = \"AP\";
                        send(sender, ready_data);
                    else
                        print(packet.object1.id, packet.action);
                    end
                end"
                .into(),
            );
        }
        Ok(())
    }
    pub fn name_to_id(&self, name: &str) -> Option<u32> {
        self.data.map_names.get(name).copied()
    }

    pub async fn init_add_player(&mut self, new_player: Arc<Mutex<User>>) -> Result<(), Error> {
        let mut np_lock = new_player.lock().await;
        np_lock
            .send_packet(&Packet::LoadLevel(self.data.map_data.clone()))
            .await?;
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
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            settings: map.clone(),
        }))
        .await?;
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
            let Some(char_data) = &p.character else {
                unreachable!("User should be in state >= `PreInGame`")
            };
            other_equipment.push(char_data.palette.send_change_palette(pid));
            other_equipment.push(char_data.palette.send_cur_weapon(pid, &char_data.inventory));
            other_equipment.push(char_data.inventory.send_equiped(pid));
            other_characters.push((char_data.character.clone(), p.position, p.isgm));
        }
        let mut np_lock = new_player.lock().await;
        np_lock.mapid = mapid;
        let np_id = np_lock.get_user_id();
        let Some(new_character) = np_lock.character.to_owned() else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        self.data.map_data.receiver.id = np_id;
        self.data.map_data.receiver.entity_type = ObjectType::Player;
        np_lock
            .send_packet(&Packet::SetPlayerID(SetPlayerIDPacket {
                player_id: np_id,
                unk2: 4,
                ..Default::default()
            }))
            .await?;
        let pos = self
            .data
            .default_location
            .iter()
            .find(|(i, _)| *i == mapid)
            .map(|(_, p)| *p)
            .unwrap_or_default();
        np_lock.position = pos;
        let np_gm = np_lock.isgm as u32;
        np_lock
            .spawn_character(CharacterSpawnPacket {
                position: pos,
                character: new_character.character.clone(),
                spawn_type: CharacterSpawnType::Myself,
                gm_flag: np_gm,
                player_obj: ObjectHeader {
                    id: np_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                ..Default::default()
            })
            .await?;
        Self::load_objects(&self.lua, &self.data, mapid, &mut np_lock)?;
        for (character, position, isgm) in other_characters {
            let player_id = character.player_id;
            np_lock
                .spawn_character(CharacterSpawnPacket {
                    position,
                    spawn_type: CharacterSpawnType::Other,
                    gm_flag: isgm as u32,
                    player_obj: ObjectHeader {
                        id: player_id,
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    character,
                    ..Default::default()
                })
                .await?;
        }
        for equipment in other_equipment {
            np_lock.send_packet(&equipment).await?;
        }
        let new_eqipment = (
            new_character.palette.send_change_palette(np_id),
            new_character
                .palette
                .send_cur_weapon(np_id, &new_character.inventory),
            new_character.inventory.send_equiped(np_id),
        );
        for (id, mapid, enemy) in self.enemies.iter().filter(|(_, mid, _)| *mid == mapid) {
            let (packet, mut packet2) = Self::prepare_enemy_packets(*id, *mapid, enemy);
            if let Packet::EnemyAction(data) = &mut packet2 {
                data.receiver = np_lock.create_object_header();
                data.action_starter = np_lock.create_object_header();
            }
            np_lock.send_packet(&packet).await?;
            np_lock.send_packet(&packet2).await?;
        }
        drop(np_lock);

        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.try_spawn_character(CharacterSpawnPacket {
                position: pos,
                spawn_type: CharacterSpawnType::Other,
                gm_flag: np_gm,
                player_obj: ObjectHeader {
                    id: new_character.character.player_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                character: new_character.character.clone(),
                ..Default::default()
            });
            let _ = player.try_send_packet(&new_eqipment.0);
            let _ = player.try_send_packet(&new_eqipment.1);
            let _ = player.try_send_packet(&new_eqipment.2);
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
        let Some(player) = player.upgrade() else {
            return Err(Error::InvalidInput("send_palette_change"));
        };
        let new_eqipment = {
            let p = player.lock().await;
            let Some(character) = &p.character else {
                unreachable!("Users in map should have characters")
            };
            (
                character.palette.send_change_palette(sender_id),
                character
                    .palette
                    .send_cur_weapon(sender_id, &character.inventory),
            )
        };
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.try_send_packet(&new_eqipment.0);
            let _ = player.try_send_packet(&new_eqipment.1);
        })
        .await;

        Ok(())
    }
    pub async fn send_to_all(&self, sender_id: PlayerId, packet: &Packet) {
        let Some((_, mapid, _)) = self.players.iter().find(|p| p.0 == sender_id) else {
            return  ;
        };
        let mapid = *mapid;
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.try_send_packet(packet);
        }).await;

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
                        entity_type: ObjectType::Player,
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
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                };
                Packet::ActionUpdateServer(packet)
            }
            Packet::ActionEnd(mut data) => {
                data.unk1 = data.performer;
                Packet::ActionEnd(data)
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
                let _ = player.try_send_packet(&out_packet);
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
                entity_type: ObjectType::Player,
                ..Default::default()
            };
        }
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.try_send_packet(&packet);
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
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            uuid: data.uuid,
            area: data.area,
            unk1: data.unk1,
            unk2: data.unk2,
            unk3: data.unk3,
        });
        exec_users(&self.players, mapid, |_, _, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }

    pub async fn remove_player(&mut self, id: PlayerId) -> Option<Arc<Mutex<User>>> {
        let (pos, _) = self.players.iter().enumerate().find(|(_, p)| p.0 == id)?;
        let (_, mapid, rem_player) = self.players.swap_remove(pos);
        let mut packet = Packet::DespawnPlayer(protocol::objects::DespawnPlayerPacket {
            receiver: ObjectHeader {
                id: 0,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            removed_player: ObjectHeader {
                id,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
        });
        exec_users(&self.players, mapid, |_, _, mut player| {
            if let Packet::DespawnPlayer(data) = &mut packet {
                data.receiver.id = player.get_user_id();
                let _ = player.try_send_packet(&packet);
            }
        })
        .await;
        rem_player.upgrade()
    }
    pub async fn spawn_enemy(
        &mut self,
        name: &str,
        pos: Position,
        map_id: MapId,
    ) -> Result<(), Error> {
        let Some(block_data) = self.block_data.to_owned() else {
            return Err(Error::NoEnemyData(name.to_string()));
        };
        let id = self.max_id + 1;
        self.max_id += 1;
        let data = EnemyStats::build(name, self.enemy_level, pos, &block_data.server_data)?;
        let (packet, mut packet2) = Self::prepare_enemy_packets(id, map_id, &data);
        self.enemies.push((id, map_id, data));

        exec_users(&self.players, map_id, |_, _, mut player| {
            let _ = player.try_send_packet(&packet);
            if let Packet::EnemyAction(data) = &mut packet2 {
                data.receiver = player.create_object_header();
                data.action_starter = player.create_object_header();
                let _ = player.try_send_packet(&packet2);
            }
        })
        .await;

        Ok(())
    }
    fn prepare_enemy_packets(enemy_id: u32, map_id: MapId, enemy: &EnemyStats) -> (Packet, Packet) {
        let packet = enemy.create_spawn_packet(enemy_id, map_id as _);
        // techically this is a response to 0x04 0x2B
        let packet2 = Packet::EnemyAction(EnemyActionPacket {
            actor: packet.object.clone(),
            action_id: 7,
            ..Default::default()
        });
        let packet = Packet::EnemySpawn(packet);
        (packet, packet2)
    }
    pub async fn deal_damage(&mut self, dmg: DealDamagePacket) -> Result<(), Error> {
        let Some(block_data) = self.block_data.to_owned() else {
            return Err(Error::InvalidInput("deal_damage"));
        };
        let (inflicter, target) = (dmg.inflicter, dmg.target);
        if inflicter.entity_type == ObjectType::Player && target.entity_type == ObjectType::Object {
            let Some((pos, (_, _, target))) = self
                .enemies
                .iter_mut()
                .enumerate()
                .find(|(_, (id, _, _))| *id == target.id)
            else {
                return Ok(());
            };
            let Some(inflicter) = self
                .players
                .iter()
                .find(|(id, _, _)| *id == inflicter.id)
                .and_then(|p| p.2.upgrade())
            else {
                return Err(Error::InvalidInput("deal_damage"));
            };
            let mut lock = inflicter.lock().await;
            let map_id = lock.get_map_id();
            let result = lock
                .get_stats_mut()
                .damage_enemy(target, &block_data.server_data, dmg)?;
            drop(lock);
            match result {
                BattleResult::Damaged { dmg_packet } => {
                    let mut packet = Packet::DamageReceive(dmg_packet);
                    exec_users(&self.players, map_id, |_, _, mut player| {
                        if let Packet::DamageReceive(data) = &mut packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&packet);
                        }
                    })
                    .await;
                }
                BattleResult::Killed {
                    dmg_packet,
                    kill_packet,
                    exp_amount,
                } => {
                    let mut action_packet = Packet::EnemyAction(EnemyActionPacket {
                        actor: dmg_packet.dmg_target,
                        action_starter: dmg_packet.dmg_inflicter,
                        action_id: 4,
                        ..Default::default()
                    });
                    let mut dmg_packet = Packet::DamageReceive(dmg_packet);
                    let mut kill_packet = Packet::EnemyKilled(kill_packet);
                    let mut exp_packets = vec![];
                    exec_users(&self.players, map_id, |_, _, mut player| {
                        exp_packets.push(player.add_exp(exp_amount))
                    })
                    .await;
                    let exp_packets = exp_packets.into_iter().collect::<Result<Vec<_>, _>>()?;
                    let mut exp_packet = Packet::GainedEXP(GainedEXPPacket {
                        receivers: exp_packets,
                        ..Default::default()
                    });
                    exec_users(&self.players, map_id, |_, _, mut player| {
                        if let Packet::DamageReceive(data) = &mut dmg_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&dmg_packet);
                        }
                        if let Packet::EnemyKilled(data) = &mut kill_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&kill_packet);
                        }
                        if let Packet::GainedEXP(data) = &mut exp_packet {
                            data.sender = player.create_object_header();
                            let _ = player.try_send_packet(&exp_packet);
                        }
                        if let Packet::EnemyAction(data) = &mut action_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&action_packet);
                        }
                    })
                    .await;
                    self.enemies.remove(pos);
                }
            }
        } else if inflicter.entity_type == ObjectType::Object
            && target.entity_type == ObjectType::Player
        {
            let Some(target) = self
                .players
                .iter_mut()
                .find(|(id, _, _)| *id == target.id)
                .and_then(|p| p.2.upgrade())
            else {
                return Err(Error::InvalidInput("deal_damage"));
            };
            let Some((_, _, inflicter)) = self
                .enemies
                .iter_mut()
                .find(|(id, _, _)| *id == inflicter.id)
            else {
                return Ok(());
            };
            let mut lock = target.lock().await;
            let map_id = lock.get_map_id();
            let result =
                inflicter.damage_player(lock.get_stats_mut(), &block_data.server_data, dmg)?;
            drop(lock);

            match result {
                BattleResult::Damaged { dmg_packet } => {
                    let mut packet = Packet::DamageReceive(dmg_packet);
                    exec_users(&self.players, map_id, |_, _, mut player| {
                        if let Packet::DamageReceive(data) = &mut packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&packet);
                        }
                    })
                    .await;
                }
                BattleResult::Killed { dmg_packet, .. } => {
                    let mut dmg_packet = Packet::DamageReceive(dmg_packet);
                    exec_users(&self.players, map_id, |_, _, mut player| {
                        if let Packet::DamageReceive(data) = &mut dmg_packet {
                            data.receiver = player.create_object_header();
                            let _ = player.try_send_packet(&dmg_packet);
                        }
                    })
                    .await;
                    todo!();
                }
            }
        }

        Ok(())
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
                globals.set("data", &*obj.data.data)?;
                globals.set("call_type", "to_vita")?;
                globals.set("size", obj.data.data.len())?;
                let chunk = lua.load(lua_code);
                chunk.exec()?;
                obj.data.data = globals.get("data")?;
                globals.raw_remove("data")?;
                globals.raw_remove("call_type")?;
                globals.raw_remove("size")?;
            }
            user.try_send_packet(&Packet::ObjectSpawn(obj.data))?;
        }
        for npc in map_data.npcs.iter().filter(|o| o.mapid == mapid).cloned() {
            user.try_send_packet(&Packet::NPCSpawn(npc.data))?;
        }
        for event in map_data.events.iter().filter(|e| e.mapid == mapid).cloned() {
            user.try_send_packet(&Packet::EventSpawn(event.data))?;
        }
        for tele in map_data
            .transporters
            .iter()
            .filter(|t| t.mapid == mapid)
            .cloned()
        {
            user.try_send_packet(&Packet::TransporterSpawn(tele.data))?;
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
                p.lock().await.send_packet(&packet).await?;
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
