use crate::{
    BlockData, Error, User,
    invites::PartyInvite,
    map::Map,
    mutex::{Mutex, MutexGuard, RwLock},
    quests::PartyQuest,
};
use pso2packetlib::protocol::{
    ObjectHeader, ObjectType, Packet,
    party::{self, BusyState, ChatStatusPacket, Color, NewBusyStatePacket},
    symbolart::{ReceiveSymbolArtPacket, SendSymbolArtPacket},
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
    colors: Vec<(u32, Color)>,
    settings: party::PartySettingsPacket,
    questname: String,
    quest: Option<PartyQuest>,
}

impl Drop for Party {
    fn drop(&mut self) {
        log::trace!("Party {} dropped", self.id.id);
    }
}
impl Party {
    pub fn new(partyid: u32) -> Self {
        log::trace!("Party {partyid} created");
        Self {
            id: ObjectHeader {
                id: partyid,
                entity_type: ObjectType::Party,
                ..Default::default()
            },
            leader: Default::default(),
            players: vec![],
            colors: vec![],
            settings: Default::default(),
            questname: String::new(),
            quest: None,
        }
    }
    fn add_color(&mut self, id: u32) -> Color {
        let colors = [Color::Red, Color::Blue, Color::Green, Color::Yellow];
        for color in colors {
            if self.colors.iter().any(|(_, c)| *c == color) {
                continue;
            }
            self.colors.push((id, color));
            return color;
        }
        Color::Red
    }
    fn get_color(&self, id: u32) -> Color {
        self.colors
            .iter()
            .find(|(i, _)| *i == id)
            .map(|(_, c)| *c)
            .unwrap_or_default()
    }
    pub async fn init_player(new_user: Arc<Mutex<User>>, partyid: u32) -> Result<(), Error> {
        let old_party = new_user.lock().await.party.take();
        let player_id = new_user.lock().await.get_user_id();
        if let Some(party) = old_party {
            party.write().await.remove_player(player_id).await?;
        }
        let mut party = Self::new(partyid);
        party.add_player(new_user.clone()).await?;
        new_user.lock().await.party = Some(Arc::new(RwLock::new(party)));
        Ok(())
    }
    // called by block
    pub async fn add_player(&mut self, new_id: Arc<Mutex<User>>) -> Result<(), Error> {
        if self.players.len() >= 4 {
            return Ok(());
        }
        let mut np_lock = new_id.lock().await;
        let (hp, max_hp) = np_lock.get_stats().get_hp();
        let color = self.add_color(np_lock.get_user_id());
        let new_player_obj = np_lock.create_object_header();
        if self.players.is_empty() {
            self.leader = new_player_obj;
        }
        let mut party_init = party::PartyInitPacket {
            party_object: self.id,
            leader: self.leader,
            people_amount: self.players.len() as u32 + 1,
            ..Default::default()
        };
        let new_char = np_lock
            .character
            .as_ref()
            .expect("User should be in state >= `PreInGame`");
        party_init.entries[0] = party::PartyEntry {
            id: new_player_obj,
            nickname: np_lock.user_data.nickname.clone(),
            char_name: new_char.character.name.clone(),
            class: new_char.character.classes.main_class,
            subclass: new_char.character.classes.sub_class,
            hp: [hp, max_hp, max_hp],
            level: new_char.character.get_level().level1 as u8,
            sublevel: new_char.character.get_sublevel().level1 as u8,
            map_id: np_lock.get_map_id() as u16,
            color,
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
            color,
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
        exec_users(&self.players, |id, mut player| {
            let other_player_obj = player.create_object_header();
            let (hp, max_hp) = player.get_stats().get_hp();
            let color_packet = Packet::SetPartyColor(party::SetPartyColorPacket {
                target: other_player_obj,
                in_party: 1,
                ..Default::default()
            });
            let char = player
                .character
                .as_ref()
                .expect("User should be in state >= `PreInGame`");
            party_init.entries[i] = party::PartyEntry {
                id: other_player_obj,
                nickname: player.user_data.nickname.clone(),
                char_name: char.character.name.clone(),
                class: char.character.classes.main_class,
                subclass: char.character.classes.sub_class,
                hp: [hp, max_hp, max_hp],
                level: char.character.get_level().level1 as u8,
                sublevel: char.character.get_sublevel().level1 as u8,
                map_id: player.get_map_id() as u16,
                color: self.get_color(id),
                ..Default::default()
            };
            let _ = player.try_send_packet(&new_player_packet);
            let _ = player.try_send_packet(&colors[0]);
            let _ = player.try_send_packet(&color_packet);
            colors.push(color_packet);
            let _ =
                player.try_send_packet(&Packet::PartySetupFinish(party::PartySetupFinishPacket {
                    unk: 1,
                }));
            i += 1;
        })
        .await;
        self.players
            .push((np_lock.get_user_id(), Arc::downgrade(&new_id)));
        np_lock.send_packet(&Packet::PartyInit(party_init)).await?;
        np_lock
            .send_packet(&Packet::PartySettings(self.settings.clone()))
            .await?;
        for packet in colors {
            np_lock.send_packet(&packet).await?;
        }
        np_lock
            .send_packet(&Packet::PartySetupFinish(party::PartySetupFinishPacket {
                unk: 0,
            }))
            .await?;
        Ok(())
    }
    // called by block
    pub async fn send_invite(
        inviter: Arc<Mutex<User>>,
        invitee: Arc<Mutex<User>>,
    ) -> Result<(), Error> {
        let (target_party, inviter_name, inviter_id) = {
            let mut lock = inviter.lock().await;
            let _ = lock
                .send_packet(&Packet::PartyInviteResult(Default::default()))
                .await;
            let Some(character) = &lock.character else {
                unreachable!("User should be in state >= `InGame`")
            };
            (
                lock.party.clone().unwrap(),
                character.character.name.clone(),
                lock.get_user_id(),
            )
        };
        let mut invitee = invitee.lock().await;
        if invitee.party_ignore == party::RejectStatus::Reject {
            return Ok(());
        }
        for invite_id in invitee
            .party_invites
            .iter()
            .map(|PartyInvite { id, .. }| id)
        {
            if *invite_id == target_party.read().await.id.id {
                return Ok(());
            }
        }
        let party = target_party.read().await;
        let new_invite = party::NewInvitePacket {
            party_object: party.id,
            inviter: ObjectHeader {
                id: inviter_id,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
            name: party.settings.name.clone(),
            inviter_name,
            questname: party.questname.clone(),
        };
        invitee.send_packet(&Packet::NewInvite(new_invite)).await?;
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
    pub async fn remove_player(&mut self, id: u32) -> Result<Option<Arc<Mutex<User>>>, Error> {
        let (pos, removed_player) = self
            .players
            .iter()
            .enumerate()
            .find(|(_, (pid, _))| *pid == id)
            .map(|(pos, (_, p))| (pos, p.clone()))
            .ok_or(Error::InvalidInput("remove_player"))?;
        self.players.swap_remove(pos);
        if let Some((pos, _)) = self.colors.iter().enumerate().find(|(_, (i, _))| *i == id) {
            self.colors.swap_remove(pos);
        }
        if let Some(player) = removed_player.upgrade() {
            let mut rem_player_lock = player.lock().await;
            for (id, _) in self.players.iter() {
                let _ = rem_player_lock
                    .send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                        target: ObjectHeader {
                            id: *id,
                            entity_type: ObjectType::Player,
                            ..Default::default()
                        },
                        in_party: 0,
                        ..Default::default()
                    }))
                    .await;
            }
        }
        let removed_obj = ObjectHeader {
            id,
            entity_type: ObjectType::Player,
            ..Default::default()
        };
        let mut remove_packet = Packet::RemoveMember(party::RemoveMemberPacket {
            removed_member: removed_obj,
            receiver: ObjectHeader {
                id: 0,
                entity_type: ObjectType::Player,
                ..Default::default()
            },
        });
        let mut leader_changed = false;
        exec_users(&self.players, |id, mut player| {
            if let Packet::RemoveMember(ref mut data) = remove_packet {
                data.receiver.id = player.get_user_id();
            }
            if self.leader.id == removed_obj.id {
                self.leader.id = id;
                leader_changed = true;
            }
            if leader_changed {
                let _ = player.try_send_packet(&Packet::NewLeader(party::NewLeaderPacket {
                    leader: self.leader,
                }));
            }
            let _ = player.try_send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                target: removed_obj,
                in_party: 0,
                ..Default::default()
            }));
            if self.players.len() == 1 {
                let _ =
                    player.try_send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                        target: removed_obj,
                        in_party: 0,
                        ..Default::default()
                    }));
            }
            let _ = player.try_send_packet(&remove_packet);
        })
        .await;

        Ok(removed_player.upgrade())
    }
    pub async fn set_settings(
        &mut self,
        settings: party::NewPartySettingsPacket,
    ) -> Result<(), Error> {
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
            let _ = user.try_send_packet(&Packet::PartySettings(self.settings.clone()));
        })
        .await;
        Ok(())
    }
    pub async fn get_info(
        player: &mut crate::User,
        packet: party::GetPartyInfoPacket,
    ) -> Result<(), Error> {
        let info_reqs: Vec<_> = packet
            .parties
            .iter()
            .map(|ObjectHeader { id, .. }| id)
            .collect();
        if info_reqs.is_empty() {
            player
                .send_packet(&Packet::PartyInfoStopper(Default::default()))
                .await?;
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
            player
                .send_packet(&Packet::PartyInfoStopper(Default::default()))
                .await?;
            return Ok(());
        }
        let mut packet = party::PartyInfoPacket::default();
        for (party, time) in user_invites.iter() {
            if packet.num_of_infos >= 10 {
                player.send_packet(&Packet::PartyInfo(packet)).await?;
                packet = Default::default()
            }
            let Some(party) = party.upgrade() else {
                continue;
            };
            let party = party.read().await;
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
            player.send_packet(&Packet::PartyInfo(packet)).await?;
        }
        player
            .send_packet(&Packet::PartyInfoStopper(Default::default()))
            .await?;

        Ok(())
    }
    pub async fn get_details(
        mut player: MutexGuard<'_, User>,
        packet: party::GetPartyDetailsPacket,
    ) -> Result<(), Error> {
        let info_reqs: Vec<_> = packet
            .parties
            .iter()
            .map(|ObjectHeader { id, .. }| id)
            .collect();
        if info_reqs.is_empty() {
            player.send_packet(&Packet::PartyDetailsStopper).await?;
            return Ok(());
        }
        let user_invites: Vec<_> = player
            .party_invites
            .iter()
            .filter(|x| info_reqs.contains(&&x.id))
            .map(|PartyInvite { party, .. }| party.clone())
            .collect();
        if user_invites.is_empty() {
            player.send_packet(&Packet::PartyDetailsStopper).await?;
            return Ok(());
        }
        let mut packet = party::PartyDetailsPacket::default();
        for party in user_invites.iter() {
            if packet.num_of_details >= 0xC {
                player.send_packet(&Packet::PartyDetails(packet)).await?;
                packet = Default::default()
            }
            let Some(party) = party.upgrade() else {
                continue;
            };
            let party = party.read().await;
            let mut detail = party::PartyDetails {
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
                let char = player
                    .character
                    .as_ref()
                    .expect("Users in parties should have characters");
                detail.unk10[player_i] = party::PartyMember {
                    char_name: char.character.name.clone(),
                    nickname: player.user_data.nickname.clone(),
                    id: ObjectHeader {
                        id: player.get_user_id(),
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    class: char.character.classes.main_class,
                    subclass: char.character.classes.sub_class,
                    level: char.character.get_level().level1 as u8,
                    sublevel: char.character.get_sublevel().level1 as u8,
                    ..Default::default()
                };
                player_i += 1;
            })
            .await;
            packet.details.push(detail);
            packet.num_of_details += 1;
        }
        if packet.num_of_details != 0 {
            player.send_packet(&Packet::PartyDetails(packet)).await?;
        }
        player.send_packet(&Packet::PartyDetailsStopper).await?;

        Ok(())
    }
    // called by block
    pub async fn accept_invite(player: Arc<Mutex<User>>, partyid: u32) -> Result<(), Error> {
        let mut target_player = player.lock().await;
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
        let p_id = target_player.get_user_id();
        target_player.party_invites.swap_remove(i);
        target_player.party = Some(party.clone());
        drop(target_player);
        if let Some(party) = orig_party {
            let _ = party.write().await.remove_player(p_id).await;
        }
        party.write().await.add_player(player).await?;

        Ok(())
    }
    pub async fn change_leader(&mut self, leader: ObjectHeader) -> Result<(), Error> {
        self.leader = leader;
        let packet = Packet::NewLeader(party::NewLeaderPacket { leader });
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
        Ok(())
    }
    // called by block
    pub async fn disband_party(&mut self, partyid: u32) -> Result<(), Error> {
        let players = self.players.clone();
        exec_users(&players, |_, mut player| {
            player.party = None;
            let _ = player.try_send_packet(&Packet::PartyDisbandedMarker);
        })
        .await;
        for (id, player) in players {
            if let Some(player) = player.upgrade() {
                let _ = self.remove_player(id).await;
                let _ = Self::init_player(player.clone(), partyid).await;
            }
        }
        Ok(())
    }
    // called by player
    pub async fn kick_player(&mut self, kick_id: u32, block: &BlockData) -> Result<(), Error> {
        let kicked_player = self
            .players
            .iter()
            .find(|(id, _)| *id == kick_id)
            .map(|(_, p)| p.clone())
            .ok_or(Error::InvalidInput("kick_player"))?;
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&Packet::KickedMember(party::KickedMemberPacket {
                member: ObjectHeader {
                    id: kick_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
            }));
        })
        .await;
        self.remove_player(kick_id).await?;
        if let Some(player) = kicked_player.upgrade() {
            let party_id = block
                .latest_partyid
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            player.lock().await.party = None;
            Self::init_player(player, party_id).await?;
        }
        Ok(())
    }
    // called by block
    pub async fn set_busy_state(&self, state: BusyState, sender_id: u32) {
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&Packet::NewBusyState(NewBusyStatePacket {
                object: ObjectHeader {
                    id: sender_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                },
                state,
            }));
        })
        .await;
    }
    // called by block
    pub async fn set_chat_status(&self, packet: ChatStatusPacket, sender_id: u32) {
        let mut packet = Packet::ChatStatus(packet);
        exec_users(&self.players, |_, mut player| {
            if let Packet::ChatStatus(ref mut packet) = packet {
                packet.object = ObjectHeader {
                    id: sender_id,
                    entity_type: ObjectType::Player,
                    ..Default::default()
                };
            }
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }
    pub async fn set_quest(&mut self, quest: PartyQuest) {
        let mut set_packet = quest.set_party_packet();
        set_packet.player = self.leader;
        let mut info_packet = quest.set_info_packet();
        info_packet.player = self.leader;
        let packet1 = Packet::SetPartyQuest(set_packet);
        let packet2 = Packet::SetQuestInfo(info_packet);
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet2);
            let _ = player.try_send_packet(&packet1);
        })
        .await;
        self.quest = Some(quest)
    }
    pub fn get_quest_map(&self) -> Option<Arc<Mutex<Map>>> {
        self.quest.as_ref().map(|q| q.get_map())
    }

    pub async fn send_message(&self, mut packet: Packet, id: u32) {
        if let Packet::ChatMessage(ref mut data) = packet {
            data.object = ObjectHeader {
                id,
                entity_type: ObjectType::Player,
                ..Default::default()
            };
        }
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }

    pub async fn send_sa(&self, data: SendSymbolArtPacket, id: u32) {
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
        exec_users(&self.players, |_, mut player| {
            let _ = player.try_send_packet(&packet);
        })
        .await;
    }

    pub async fn abandon(&mut self) {
        self.quest = None;
        self.questname.clear();
        for (id, user) in self
            .players
            .iter()
            .filter_map(|(i, p)| p.upgrade().map(|p| (*i, p)))
        {
            //TODO: there is some packet missing, because abandoning in lobby doesn't remove the
            //quest from the client
            let mut lock = user.lock().await;
            let _ = lock
                .send_packet(&Packet::Unknown((
                    pso2packetlib::protocol::PacketHeader {
                        id: 0xE,
                        subid: 0x13,
                        flag: Default::default(),
                    },
                    vec![0, 0, 0, 0],
                )))
                .await;
            let current_map = lock
                .get_current_map()
                .expect("Player should have a map assigned");
            drop(lock);
            let _ = current_map.lock().await.move_to_lobby(id).await;
        }
    }
    pub const fn get_obj(&self) -> ObjectHeader {
        self.id
    }
}

async fn exec_users<F>(users: &[(u32, Weak<Mutex<User>>)], mut f: F)
where
    F: FnMut(u32, MutexGuard<User>) + Send,
{
    for (id, user) in users
        .iter()
        .filter_map(|(i, p)| p.upgrade().map(|p| (*i, p)))
    {
        f(id, user.lock().await)
    }
}
