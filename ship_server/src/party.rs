use crate::{invites::PartyInvite, Error, User};
use parking_lot::{Mutex, MutexGuard, RwLock};
use pso2packetlib::protocol::{
    party::{self, BusyState, ChatStatusPacket, NewBusyStatePacket},
    EntityType, ObjectHeader, Packet,
};
use std::{
    sync::{Arc, Weak},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Party {
    id: ObjectHeader,
    leader: ObjectHeader,
    // id, player
    players: Vec<(u32, Weak<Mutex<User>>)>,
    settings: party::PartySettingsPacket,
    questname: String,
}

impl Drop for Party {
    fn drop(&mut self) {
        println!("Party {} dropped", self.id.id);
    }
}
impl Party {
    pub fn new(partyid: &mut u32) -> Self {
        let party = Self {
            id: ObjectHeader {
                id: *partyid,
                entity_type: EntityType::Party,
                ..Default::default()
            },
            leader: Default::default(),
            players: vec![],
            settings: Default::default(),
            questname: String::new(),
        };
        *partyid += 1;
        party
    }
    pub fn init_player(new_user: Arc<Mutex<User>>, partyid: &mut u32) -> Result<(), Error> {
        let old_party = new_user.lock().party.take();
        let player_id = new_user.lock().player_id;
        if let Some(party) = old_party {
            party.write().remove_player(player_id)?;
        }
        let mut party = Self::new(partyid);
        party.add_player(new_user.clone())?;
        new_user.lock().party = Some(Arc::new(RwLock::new(party)));
        Ok(())
    }
    // called by block
    pub fn add_player(&mut self, new_id: Arc<Mutex<User>>) -> Result<(), Error> {
        if self.players.len() >= 4 {
            return Ok(());
        }
        let mut np_lock = new_id.lock();
        let new_player_obj = ObjectHeader {
            id: np_lock.player_id,
            entity_type: EntityType::Player,
            ..Default::default()
        };
        if self.players.is_empty() {
            self.leader = new_player_obj;
        }
        let mut party_init = party::PartyInitPacket {
            party_object: self.id,
            leader: self.leader,
            people_amount: self.players.len() as u32 + 1,
            ..Default::default()
        };
        party_init.entries[0] = party::PartyEntry {
            id: new_player_obj,
            nickname: np_lock.nickname.clone(),
            char_name: np_lock.character.as_ref().unwrap().name.clone(),
            class: np_lock.character.as_ref().unwrap().classes.main_class,
            subclass: np_lock.character.as_ref().unwrap().classes.sub_class,
            hp: [100, 100, 100],
            level: np_lock.character.as_ref().unwrap().get_level().level1 as u8,
            sublevel: np_lock.character.as_ref().unwrap().get_sublevel().level1 as u8,
            map_id: np_lock
                .map
                .as_ref()
                .map(|x| x.lock().get_mapid() as u16)
                .unwrap_or(0),
            ..Default::default()
        };

        let new_player_packet = Packet::AddMember(party::AddMemberPacket {
            new_member: new_player_obj,
            level: party_init.entries[0].level as u32,
            sublevel: party_init.entries[0].sublevel as u32,
            padding: if party_init.entries[0].sublevel as u32 == 0xFF {
                [255, 255, 255]
            } else {
                [0, 0, 0]
            },
            class: party_init.entries[0].class,
            subclass: party_init.entries[0].subclass,
            hp: party_init.entries[0].hp,
            nickname: party_init.entries[0].nickname.clone(),
            char_name: party_init.entries[0].char_name.clone(),
            map_id: party_init.entries[0].map_id,
            ..Default::default()
        });
        let mut colors = if !self.players.is_empty() {
            vec![Packet::SetPartyColor(party::SetPartyColorPacket {
                target: new_player_obj,
                in_party: 1,
                ..Default::default()
            })]
        } else {
            vec![]
        };
        let mut i = 1;
        exec_users(&self.players, |_, mut player| {
            let other_player_obj = ObjectHeader {
                id: player.player_id,
                entity_type: EntityType::Player,
                ..Default::default()
            };
            let color_packet = Packet::SetPartyColor(party::SetPartyColorPacket {
                target: other_player_obj,
                in_party: 1,
                ..Default::default()
            });
            party_init.entries[i] = party::PartyEntry {
                id: other_player_obj,
                nickname: player.nickname.clone(),
                char_name: player.character.as_ref().unwrap().name.clone(),
                class: player.character.as_ref().unwrap().classes.main_class,
                subclass: player.character.as_ref().unwrap().classes.sub_class,
                hp: [100, 100, 100],
                level: player.character.as_ref().unwrap().get_level().level1 as u8,
                sublevel: player.character.as_ref().unwrap().get_sublevel().level1 as u8,
                map_id: player
                    .map
                    .as_ref()
                    .map(|x| x.lock().get_mapid() as u16)
                    .unwrap_or(0),
                ..Default::default()
            };
            let _ = player.send_packet(&new_player_packet);
            let _ = player.send_packet(&colors[0]);
            let _ = player.send_packet(&color_packet);
            colors.push(color_packet);
            let _ = player.send_packet(&Packet::PartySetupFinish(party::PartySetupFinishPacket {
                unk: 1,
            }));
            i += 1;
        });
        self.players
            .push((np_lock.player_id, Arc::downgrade(&new_id)));
        np_lock.send_packet(&Packet::PartyInit(party_init))?;
        np_lock.send_packet(&Packet::PartySettings(self.settings.clone()))?;
        for packet in colors {
            np_lock.send_packet(&packet)?;
        }
        np_lock.send_packet(&Packet::PartySetupFinish(party::PartySetupFinishPacket {
            unk: 0,
        }))?;
        Ok(())
    }
    // called by block
    pub fn send_invite(inviter: Arc<Mutex<User>>, invitee: Arc<Mutex<User>>) -> Result<(), Error> {
        let (target_party, inviter_name, inviter_id) = {
            let mut lock = inviter.lock();
            if lock.party.is_none() || lock.character.is_none() {
                return Err(Error::NoCharacter);
            }
            let _ = lock.send_packet(&Packet::PartyInviteResult(Default::default()));
            (
                lock.party.clone().unwrap(),
                lock.character.as_ref().unwrap().name.clone(),
                lock.player_id,
            )
        };
        let mut invitee = invitee.lock();
        if invitee.party_ignore == party::RejectStatus::Reject {
            return Ok(());
        }
        if invitee
            .party_invites
            .iter()
            .map(|PartyInvite { id, .. }| id)
            .any(|&x| x == target_party.read().id.id)
        {
            return Ok(());
        };
        let party = target_party.read();
        let new_invite = party::NewInvitePacket {
            party_object: party.id,
            inviter: ObjectHeader {
                id: inviter_id,
                entity_type: EntityType::Player,
                ..Default::default()
            },
            name: party.settings.name.clone(),
            inviter_name,
            questname: party.questname.clone(),
        };
        invitee.send_packet(&Packet::NewInvite(new_invite))?;
        invitee.party_invites.push(PartyInvite {
            id: party.id.id,
            party: Arc::downgrade(&target_party),
            invite_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        });
        Ok(())
    }
    // called by block
    pub fn remove_player(&mut self, id: u32) -> Result<(), Error> {
        let (pos, removed_player) = self
            .players
            .iter()
            .enumerate()
            .find(|(_, (pid, _))| *pid == id)
            .map(|(pos, (_, p))| (pos, p.clone()))
            .ok_or(Error::InvalidInput)?;
        self.players.swap_remove(pos);
        if let Some(player) = removed_player.upgrade() {
            let mut rem_player_lock = player.lock();
            for (id, _) in self.players.iter() {
                let _ = rem_player_lock.send_packet(&Packet::SetPartyColor(
                    party::SetPartyColorPacket {
                        target: ObjectHeader {
                            id: *id,
                            entity_type: EntityType::Player,
                            ..Default::default()
                        },
                        in_party: 0,
                        ..Default::default()
                    },
                ));
            }
        }
        let removed_obj = ObjectHeader {
            id,
            entity_type: EntityType::Player,
            ..Default::default()
        };
        let mut remove_packet = Packet::RemoveMember(party::RemoveMemberPacket {
            removed_member: removed_obj,
            receiver: ObjectHeader {
                id: 0,
                entity_type: EntityType::Player,
                ..Default::default()
            },
        });
        exec_users(&self.players, |_, mut player| {
            if let Packet::RemoveMember(ref mut data) = remove_packet {
                data.receiver.id = player.player_id;
                if self.leader.id == data.removed_member.id {
                    self.leader = data.receiver;
                }
                let _ = player.send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                    target: data.removed_member,
                    in_party: 0,
                    ..Default::default()
                }));
                if self.players.len() == 1 {
                    let _ =
                        player.send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                            target: data.receiver,
                            in_party: 0,
                            ..Default::default()
                        }));
                }
            }
            let _ = player.send_packet(&remove_packet);
        });

        Ok(())
    }
    pub fn set_settings(&mut self, settings: party::NewPartySettingsPacket) -> Result<(), Error> {
        self.questname = settings.questname;
        self.settings = party::PartySettingsPacket {
            name: settings.name,
            password: settings.password,
            comments: settings.comments,
            min_level: settings.min_level,
            max_level: settings.max_level,
            playstyle: settings.playstyle,
            flags: settings.flags,
            unk: settings.unk,
        };
        exec_users(&self.players, |_, mut user| {
            let _ = user.send_packet(&Packet::PartySettings(self.settings.clone()));
        });
        Ok(())
    }
    pub fn get_info(
        player: &mut crate::User,
        packet: party::GetPartyInfoPacket,
    ) -> Result<(), Error> {
        let info_reqs: Vec<_> = packet
            .parties
            .iter()
            .map(|ObjectHeader { id, .. }| id)
            .collect();
        if info_reqs.is_empty() {
            player.send_packet(&Packet::PartyInfoStopper(Default::default()))?;
            return Ok(());
        }
        let user_invites: Vec<_> = player
            .party_invites
            .iter()
            .filter(|x| info_reqs.contains(&&x.id))
            .map(
                |PartyInvite {
                     party, invite_time, ..
                 }| (party.clone(), *invite_time),
            )
            .collect();
        if user_invites.is_empty() {
            player.send_packet(&Packet::PartyInfoStopper(Default::default()))?;
            return Ok(());
        }
        let mut packet = party::PartyInfoPacket::default();
        for (party, time) in user_invites.iter() {
            if packet.num_of_infos >= 10 {
                player.send_packet(&Packet::PartyInfo(packet))?;
                packet = Default::default()
            }
            let Some(party) = party.upgrade() else {
                continue;
            };
            let party = party.read();
            packet.infos[packet.num_of_infos as usize] = party::PartyInfo {
                invite_time: *time,
                party_object: party.id,
                name: party.settings.name.clone(),
                unk2: [0, 0, 4, 1, 0, 0, 0, 0, 0],
                unk4: 21,
                unk6: 4294967295,
                ..Default::default()
            };
            packet.num_of_infos += 1;
        }
        if packet.num_of_infos != 0 {
            player.send_packet(&Packet::PartyInfo(packet))?;
        }
        player.send_packet(&Packet::PartyInfoStopper(Default::default()))?;

        Ok(())
    }
    pub fn get_details(
        mut player: MutexGuard<User>,
        packet: party::GetPartyDetailsPacket,
    ) -> Result<(), Error> {
        let info_reqs: Vec<_> = packet
            .parties
            .iter()
            .map(|ObjectHeader { id, .. }| id)
            .collect();
        if info_reqs.is_empty() {
            player.send_packet(&Packet::PartyDetailsStopper)?;
            return Ok(());
        }
        let user_invites: Vec<_> = player
            .party_invites
            .iter()
            .filter(|x| info_reqs.contains(&&x.id))
            .map(|PartyInvite { party, .. }| party.clone())
            .collect();
        if user_invites.is_empty() {
            player.send_packet(&Packet::PartyDetailsStopper)?;
            return Ok(());
        }
        let mut packet = party::PartyDetailsPacket::default();
        for (i, party) in user_invites.iter().enumerate() {
            if packet.num_of_details >= 0xC {
                player.send_packet(&Packet::PartyDetails(packet))?;
                packet = Default::default()
            }
            let Some(party) = party.upgrade() else {
                continue;
            };
            let party = party.read();
            packet.details[i] = party::PartyDetails {
                party_id: party.id,
                party_desc: party.settings.comments.clone(),
                unk5: 4294967295,
                unk6: 3,
                unk7: 1,
                unk9: [0, 0, 1, 100, 1, 0, 0, 255, 0, 0, 0, 0],
                ..Default::default()
            };
            let mut player_i = 0;
            exec_users(&party.players, |_, player| {
                packet.details[i].unk10[player_i] = party::PartyMember {
                    char_name: player.character.as_ref().unwrap().name.clone(),
                    nickname: player.nickname.clone(),
                    id: ObjectHeader {
                        id: player.player_id,
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                    class: player.character.as_ref().unwrap().classes.main_class,
                    subclass: player.character.as_ref().unwrap().classes.sub_class,
                    level: player.character.as_ref().unwrap().get_level().level1 as u8,
                    sublevel: player.character.as_ref().unwrap().get_sublevel().level1 as u8,
                    ..Default::default()
                };
                player_i += 1;
            });
            packet.num_of_details += 1;
        }
        if packet.num_of_details != 0 {
            player.send_packet(&Packet::PartyDetails(packet))?;
        }
        player.send_packet(&Packet::PartyDetailsStopper)?;

        Ok(())
    }
    // called by block
    pub fn accept_invite(player: Arc<Mutex<User>>, partyid: u32) -> Result<(), Error> {
        let mut target_player = player.lock();
        let user_invites = target_player
            .party_invites
            .iter()
            .enumerate()
            .map(|(i, PartyInvite { party, id, .. })| (party.clone(), *id, i))
            .find(|(_, x, _)| *x == partyid);
        let Some((party, _, i)) = user_invites else {
            return Ok(());
        };
        let Some(party) = party.upgrade() else {
            return Ok(());
        };
        let orig_party = target_player.party.take();
        let p_id = target_player.player_id;
        target_player.party_invites.swap_remove(i);
        target_player.party = Some(party.clone());
        drop(target_player);
        if let Some(party) = orig_party {
            let _ = party.write().remove_player(p_id);
        }
        party.write().add_player(player)?;

        Ok(())
    }
    pub fn change_leader(&mut self, leader: ObjectHeader) -> Result<(), Error> {
        self.leader = leader;
        let packet = Packet::NewLeader(party::NewLeaderPacket { leader });
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&packet);
        });
        Ok(())
    }
    // called by block
    pub fn disband_party(&mut self, partyid: &mut u32) -> Result<(), Error> {
        let players = self.players.clone();
        exec_users(&players, |id, mut player| {
            player.party = None;
            let _ = player.send_packet(&Packet::PartyDisbandedMarker);
            drop(player);
            let _ = self.remove_player(id);
        });
        for player in players {
            if let Some(player) = player.1.upgrade() {
                let _ = Self::init_player(player.clone(), partyid);
            }
        }
        Ok(())
    }
    // called by block
    pub fn kick_player(&mut self, kick_id: u32, partyid: &mut u32) -> Result<(), Error> {
        let kicked_player = self
            .players
            .iter()
            .find(|(id, _)| *id == kick_id)
            .map(|(_, p)| p.clone())
            .ok_or(Error::InvalidInput)?;
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&Packet::KickedMember(party::KickedMemberPacket {
                member: ObjectHeader {
                    id: kick_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
            }));
        });
        self.remove_player(kick_id)?;
        if let Some(player) = kicked_player.upgrade() {
            player.lock().party = None;
            Self::init_player(player, partyid)?;
        }
        Ok(())
    }
    // called by block
    pub fn set_busy_state(&self, state: BusyState, sender_id: u32) {
        exec_users(&self.players, |_, mut player| {
            let _ = player.send_packet(&Packet::NewBusyState(NewBusyStatePacket {
                object: ObjectHeader {
                    id: sender_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
                state,
            }));
        });
    }
    // called by block
    pub fn set_chat_status(&self, packet: ChatStatusPacket, sender_id: u32) {
        let mut packet = Packet::ChatStatus(packet);
        exec_users(&self.players, |_, mut player| {
            if let Packet::ChatStatus(ref mut packet) = packet {
                packet.object = ObjectHeader {
                    id: sender_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                };
            }
            let _ = player.send_packet(&packet);
        });
    }
}

fn exec_users<F>(users: &[(u32, Weak<Mutex<User>>)], mut f: F)
where
    F: FnMut(u32, MutexGuard<User>),
{
    for (id, user) in users
        .iter()
        .filter_map(|(i, p)| p.upgrade().map(|p| (*i, p)))
    {
        f(id, user.lock())
    }
}
