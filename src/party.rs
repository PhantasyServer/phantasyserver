use crate::{invites::PartyInvite, Error};
use pso2packetlib::protocol::{
    party::{self, BusyState, ChatStatusPacket, NewBusyStatePacket},
    EntityType, ObjectHeader, Packet,
};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Party {
    id: ObjectHeader,
    leader: ObjectHeader,
    players: Vec<u32>,
    settings: party::PartySettingsPacket,
    questname: String,
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
    pub fn init_player(
        players: &mut [crate::User],
        new_id: u32,
        partyid: &mut u32,
    ) -> Result<(), Error> {
        let old_party = players
            .iter_mut()
            .find(|p| p.player_id == new_id && p.character.is_some())
            .map(|p| p.party.take())
            .ok_or(Error::NoCharacter)?;
        match old_party {
            Some(party) => party.borrow_mut().remove_player(players, new_id)?,
            None => {}
        }
        let mut party = Self::new(partyid);
        party.add_player(players, new_id)?;
        let new_player = players
            .iter_mut()
            .find(|p| p.player_id == new_id && p.character.is_some())
            .ok_or(Error::NoCharacter)?;
        new_player.party = Some(Rc::new(RefCell::new(party)));
        Ok(())
    }
    // called by block
    pub fn add_player(&mut self, players: &mut [crate::User], new_id: u32) -> Result<(), Error> {
        if self.players.len() >= 4 {
            return Ok(());
        }
        let new_player = players
            .iter_mut()
            .find(|p| p.player_id == new_id && p.character.is_some())
            .ok_or(Error::NoCharacter)?;
        let new_player_obj = ObjectHeader {
            id: new_id,
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
            nickname: new_player.nickname.clone(),
            char_name: new_player.character.as_ref().unwrap().name.clone(),
            class: new_player.character.as_ref().unwrap().classes.main_class,
            subclass: new_player.character.as_ref().unwrap().classes.sub_class,
            hp: [100, 100, 100],
            level: new_player.character.as_ref().unwrap().get_level().level1 as u8,
            sublevel: new_player.character.as_ref().unwrap().get_sublevel().level1 as u8,
            map_id: new_player
                .map
                .as_ref()
                .map(|x| x.borrow().get_mapid() as u16)
                .unwrap_or(0),
            ..Default::default()
        };

        let other_players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id) && p.character.is_some())
            .take(3)
            .enumerate()
            .map(|(x, y)| (x + 1, y));
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
        for (i, player) in other_players {
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
                    .map(|x| x.borrow().get_mapid() as u16)
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
        }
        self.players.push(new_id);
        let new_player = players
            .iter_mut()
            .find(|p| p.player_id == new_id && p.character.is_some())
            .ok_or(Error::NoCharacter)?;
        new_player.send_packet(&Packet::PartyInit(party_init))?;
        new_player.send_packet(&Packet::PartySettings(self.settings.clone()))?;
        for packet in colors {
            new_player.send_packet(&packet)?;
        }
        new_player.send_packet(&Packet::PartySetupFinish(party::PartySetupFinishPacket {
            unk: 0,
        }))?;
        Ok(())
    }
    // called by block
    pub fn send_invite(
        players: &mut [crate::User],
        inviter: u32,
        invitee: u32,
    ) -> Result<(), Error> {
        let (target_party, inviter_name) = players
            .iter_mut()
            .find(|p| p.player_id == inviter && p.party.is_some() && p.character.is_some())
            .map(|p| -> Result<_, Error> {
                let _ = p.send_packet(&Packet::PartyInviteResult(Default::default()));
                Ok((
                    p.party.clone().unwrap(),
                    p.character.as_ref().unwrap().name.clone(),
                ))
            })
            .ok_or(Error::NoCharacter)??;
        let invitee = players
            .iter_mut()
            .find(|p| p.player_id == invitee)
            .ok_or(Error::InvalidInput)?;
        if invitee.party_ignore == party::RejectStatus::Reject {
            return Ok(());
        }
        if invitee
            .party_invites
            .iter()
            .map(|PartyInvite { id, .. }| id)
            .any(|&x| x == target_party.borrow().id.id)
        {
            return Ok(());
        };
        let party = target_party.borrow();
        let new_invite = party::NewInvitePacket {
            party_object: party.id,
            inviter: ObjectHeader {
                id: inviter,
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
            party: target_party.clone(),
            invite_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as u32,
        });
        Ok(())
    }
    // called by block
    pub fn remove_player(&mut self, players: &mut [crate::User], id: u32) -> Result<(), Error> {
        let remover_player = players
            .iter_mut()
            .find(|p| p.player_id == id)
            .ok_or(Error::InvalidInput)?;
        for id in &self.players {
            let _ =
                remover_player.send_packet(&Packet::SetPartyColor(party::SetPartyColorPacket {
                    target: ObjectHeader {
                        id: *id,
                        entity_type: EntityType::Player,
                        ..Default::default()
                    },
                    in_party: 0,
                    ..Default::default()
                }));
        }
        let pos = self
            .players
            .iter()
            .enumerate()
            .find(|(_, &x)| x == id)
            .map(|(x, _)| x)
            .ok_or(Error::InvalidInput)?;
        self.players.swap_remove(pos);
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
        let other_players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        for player in other_players {
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
        }

        Ok(())
    }
    // called by block
    pub fn set_settings(
        &mut self,
        players: &mut [crate::User],
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
        let users = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id) && p.character.is_some());
        for user in users {
            let _ = user.send_packet(&Packet::PartySettings(self.settings.clone()));
        }
        Ok(())
    }
    // called by player
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
        for (i, (party, time)) in user_invites.iter().enumerate() {
            if i >= 10 {
                player.send_packet(&Packet::PartyInfo(packet))?;
                packet = Default::default()
            }
            packet.num_of_infos += 1;
            let party = party.borrow();
            packet.infos[i] = party::PartyInfo {
                invite_time: *time,
                party_object: party.id,
                name: party.settings.name.clone(),
                unk2: [0, 0, 4, 1, 0, 0, 0, 0, 0],
                unk4: 21,
                unk6: 4294967295,
                ..Default::default()
            };
        }
        if packet.num_of_infos != 0 {
            player.send_packet(&Packet::PartyInfo(packet))?;
        }
        player.send_packet(&Packet::PartyInfoStopper(Default::default()))?;

        Ok(())
    }
    // called by block
    pub fn get_details(
        players: &mut [crate::User],
        caller_pos: usize,
        packet: party::GetPartyDetailsPacket,
    ) -> Result<(), Error> {
        let player = &mut players[caller_pos];
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
            if i >= 0xC {
                let player = &mut players[caller_pos];
                player.send_packet(&Packet::PartyDetails(packet))?;
                packet = Default::default()
            }
            packet.num_of_details += 1;
            let party = party.borrow();
            packet.details[i] = party::PartyDetails {
                party_id: party.id,
                party_desc: party.settings.comments.clone(),
                unk5: 4294967295,
                unk6: 3,
                unk7: 1,
                unk9: [0, 0, 1, 100, 1, 0, 0, 255, 0, 0, 0, 0],
                ..Default::default()
            };
            for (player_i, player) in players
                .iter()
                .filter(|p| party.players.contains(&p.player_id) && p.character.is_some())
                .take(4)
                .enumerate()
            {
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
                }
            }
        }
        let player = &mut players[caller_pos];
        if packet.num_of_details != 0 {
            player.send_packet(&Packet::PartyDetails(packet))?;
        }
        player.send_packet(&Packet::PartyDetailsStopper)?;

        Ok(())
    }
    // called by block
    pub fn accept_invite(players: &mut [crate::User], id: u32, partyid: u32) -> Result<(), Error> {
        let target_player = players
            .iter_mut()
            .find(|p| p.player_id == id && p.character.is_some())
            .ok_or(Error::NoCharacter)?;
        let user_invites = target_player
            .party_invites
            .iter()
            .enumerate()
            .map(|(i, PartyInvite { party, id, .. })| (party.clone(), *id, i))
            .find(|(_, x, _)| *x == partyid);
        let Some((party, _, i)) = user_invites else {
            return Ok(());
        };
        if let Some(party) = target_player.party.take() {
            party.borrow_mut().remove_player(players, id)?;
        }
        let target_player = players
            .iter_mut()
            .find(|p| p.player_id == id && p.character.is_some())
            .ok_or(Error::NoCharacter)?;
        target_player.party_invites.swap_remove(i);
        target_player.party = Some(party.clone());
        party.borrow_mut().add_player(players, id)?;

        Ok(())
    }
    // called by block
    pub fn change_leader(
        &mut self,
        players: &mut [crate::User],
        leader: ObjectHeader,
    ) -> Result<(), Error> {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        self.leader = leader;
        let packet = Packet::NewLeader(party::NewLeaderPacket { leader });
        for player in players {
            let _ = player.send_packet(&packet);
        }
        Ok(())
    }
    // called by block
    pub fn disband_party(
        players: &mut [crate::User],
        id: u32,
        partyid: &mut u32,
    ) -> Result<(), Error> {
        let party = players
            .iter_mut()
            .find(|p| p.player_id == id)
            .map(|p| p.party.take())
            .ok_or(Error::InvalidInput)?;
        let Some(party) = party else {
            return Self::init_player(players, id, partyid);
        };
        let party_members = party.borrow().players.clone();
        let players = players
            .iter_mut()
            .filter(|p| party_members.contains(&p.player_id));
        for player in players {
            player.party = None;
            let id = player.player_id;
            player.send_packet(&Packet::PartyDisbandedMarker)?;
            Self::init_player(std::slice::from_mut(player), id, partyid)?;
        }

        Ok(())
    }
    // called by block
    pub fn kick_player(
        &mut self,
        players: &mut [crate::User],
        kick_id: u32,
        partyid: &mut u32,
    ) -> Result<(), Error> {
        let players_party = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        for player in players_party {
            player.send_packet(&Packet::KickedMember(party::KickedMemberPacket {
                member: ObjectHeader {
                    id: kick_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
            }))?;
        }
        self.remove_player(players, kick_id)?;
        players
            .iter_mut()
            .find(|p| p.player_id == kick_id)
            .map(|p| p.party = None)
            .ok_or(Error::InvalidInput)?;

        Self::init_player(players, kick_id, partyid)?;
        Ok(())
    }
    // called by block
    pub fn set_busy_state(&self, players: &mut [crate::User], state: BusyState, sender_id: u32) {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        for player in players {
            let _ = player.send_packet(&Packet::NewBusyState(NewBusyStatePacket {
                object: ObjectHeader {
                    id: sender_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                },
                state,
            }));
        }
    }
    // called by block
    pub fn set_chat_status(
        &self,
        players: &mut [crate::User],
        packet: ChatStatusPacket,
        sender_id: u32,
    ) {
        let players = players
            .iter_mut()
            .filter(|p| self.players.contains(&p.player_id));
        let mut packet = Packet::ChatStatus(packet);
        for player in players {
            if let Packet::ChatStatus(ref mut packet) = packet {
                packet.object = ObjectHeader {
                    id: sender_id,
                    entity_type: EntityType::Player,
                    ..Default::default()
                };
            }
            let _ = player.send_packet(&packet);
        }
    }
}
