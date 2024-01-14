use crate::{Action, Error, User};
use pso2packetlib::{
    ppac::Direction,
    protocol::{
        self,
        login::{self, BlockListPacket},
        ObjectHeader, Packet, PacketType,
    },
};
use std::time::{SystemTime, UNIX_EPOCH};

use super::HResult;

pub fn encryption_request(user: &mut User, _: login::EncryptionRequestPacket) -> HResult {
    let key = user.connection.get_key();
    user.send_packet(&Packet::EncryptionResponse(
        login::EncryptionResponsePacket { data: key },
    ))?;
    Ok(Action::Nothing)
}

pub async fn login_request(user: &mut User, packet: Packet) -> HResult {
    let (mut id, mut status, mut error) = Default::default();
    let ip = user.get_ip()?;
    match packet {
        Packet::SegaIDLogin(packet) => {
            user.packet_type = PacketType::JP;
            user.connection.change_packet_type(PacketType::JP);
            let sega_user = user
                .blockdata
                .sql
                .get_sega_user(&packet.username, &packet.password, ip)
                .await;
            match sega_user {
                Ok(x) => {
                    id = x.id;
                    user.nickname = x.nickname;
                    user.text_lang = packet.text_lang;
                    user.send_packet(&Packet::ChallengeRequest(login::ChallengeRequestPacket {
                        data: vec![0x0C, 0x47, 0x29, 0x91, 0x27, 0x8E, 0x52, 0x22],
                    }))?;
                    user.accountflags = x.accountflags;
                    user.isgm = x.isgm;
                }
                Err(Error::InvalidPassword) => {
                    status = login::LoginStatus::Failure;
                    error = "Invalid username or password".to_string();
                }
                Err(Error::InvalidInput(_)) => {
                    status = login::LoginStatus::Failure;
                    error = "Empty username or password".to_string();
                }
                Err(e) => return Err(e),
            }
        }
        Packet::VitaLogin(packet) => {
            user.packet_type = PacketType::Vita;
            user.connection.change_packet_type(PacketType::Vita);
            let user_psn = user
                .blockdata
                .sql
                .get_psn_user(&packet.username, ip)
                .await?;
            user.nickname = user_psn.nickname;
            id = user_psn.id;
            user.accountflags = user_psn.accountflags;
            user.isgm = user_psn.isgm;
        }
        _ => unreachable!(),
    }
    user.player_id = id;

    user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
        status,
        error,
        blockname: user.blockdata.block_name.clone(),
        player: ObjectHeader {
            id,
            entity_type: protocol::EntityType::Player,
            ..Default::default()
        },
        ..Default::default()
    }))?;
    if let login::LoginStatus::Failure = status {
        return Ok(Action::Nothing);
    }
    user.connection
        .create_ppac(format!("{}.pac", id), Direction::ToClient)
        .unwrap();
    user.send_item_attrs().await?;
    let info = user.blockdata.sql.get_user_info(id).await?;
    user.send_packet(&Packet::UserInfo(info))?;

    Ok(Action::Nothing)
}

pub async fn block_list(user: &mut User) -> HResult {
    let mut blocks = BlockListPacket {
        blocks: vec![],
        unk: 0,
    };
    let lock = user.blockdata.blocks.read().await;
    for block in lock.iter() {
        blocks.blocks.push(login::BlockInfo {
            block_id: block.id as u16,
            blockname: block.name.to_string(),
            ip: block.ip,
            port: block.port,
            cur_capacity: block.players as f32 / block.max_players as f32,
            unk4: 26,
            unk5: 4,
            unk6: 1,
            unk8: 19,
            unk10: 3,
            unk11: 164,
            ..Default::default()
        })
    }
    drop(lock);
    let pos = blocks
        .blocks
        .iter()
        .enumerate()
        .find(|(_, b)| b.block_id as u32 == user.blockdata.block_id)
        .unwrap()
        .0;
    blocks.blocks[pos].unk1 = 8;
    blocks.blocks.swap(pos, 0);
    user.send_packet(&Packet::BlockList(blocks))?;
    Ok(Action::Nothing)
}

pub async fn challenge_login(user: &mut User, packet: login::BlockLoginPacket) -> HResult {
    let user_id = packet.player_id as u32;
    let challenge = packet.challenge;
    let pso_user = user.blockdata.sql.login_challenge(user_id, challenge).await;
    let (mut id, mut status, mut error) = Default::default();
    match pso_user {
        Ok(x) => {
            id = x.id;
            user.nickname = x.nickname;
            user.connection.change_packet_type(x.packet_type);
            user.packet_type = x.packet_type;
            user.text_lang = x.lang;
            user.send_packet(&Packet::ChallengeRequest(login::ChallengeRequestPacket {
                data: vec![0x0C, 0x47, 0x29, 0x91, 0x27, 0x8E, 0x52, 0x22],
            }))?;
            user.accountflags = x.accountflags;
            user.isgm = x.isgm;
        }
        Err(Error::NoUser) => {
            status = login::LoginStatus::Failure;
            error = "Invalid user".to_string();
        }

        Err(e) => return Err(e),
    }
    user.player_id = id;
    user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
        status,
        error,
        blockname: user.blockdata.block_name.clone(),
        player: ObjectHeader {
            id,
            entity_type: protocol::EntityType::Player,
            ..Default::default()
        },
        ..Default::default()
    }))?;
    if let login::LoginStatus::Failure = status {
        return Ok(Action::Nothing);
    }
    user.send_item_attrs().await?;
    let info = user.blockdata.sql.get_user_info(id).await?;
    user.send_packet(&Packet::UserInfo(info))?;

    Ok(Action::Nothing)
}
pub async fn switch_block(user: &mut User, packet: login::BlockSwitchRequestPacket) -> HResult {
    let lock = user.blockdata.blocks.read().await;
    if let Some(block) = lock.iter().find(|b| b.id == packet.block_id as u32) {
        let challenge = user
            .blockdata
            .sql
            .new_challenge(user.player_id, user.text_lang, user.packet_type)
            .await?;
        let packet = Packet::BlockSwitchResponse(login::BlockSwitchResponsePacket {
            unk1: packet.unk1,
            unk2: packet.unk2,
            unk3: packet.unk3,
            block_id: packet.block_id,
            ip: block.ip,
            port: block.port,
            unk4: 1,
            challenge,
            user_id: user.player_id,
        });
        drop(lock);
        user.send_packet(&packet)?;
    }
    Ok(Action::Nothing)
}

pub fn client_ping(user: &mut User, packet: login::ClientPingPacket) -> HResult {
    let response = login::ClientPongPacket {
        client_time: packet.time,
        server_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
        unk1: 0,
    };
    user.send_packet(&Packet::ClientPong(response))?;
    Ok(Action::Nothing)
}

pub async fn character_list(user: &mut User) -> HResult {
    user.send_packet(&Packet::CharacterListResponse(login::CharacterListPacket {
        characters: user.blockdata.sql.get_characters(user.player_id).await?,
        // deletion_flags: [(1, 0); 30],
        ..Default::default()
    }))?;
    Ok(Action::Nothing)
}

pub fn character_create1(user: &mut User) -> HResult {
    user.send_packet(&Packet::CreateCharacter1Response(
        login::CreateCharacter1ResponsePacket::default(),
    ))?;
    Ok(Action::Nothing)
}

pub fn character_create2(user: &mut User) -> HResult {
    user.send_packet(&Packet::CreateCharacter2Response(
        login::CreateCharacter2ResponsePacket { unk: 1 },
    ))?;
    Ok(Action::Nothing)
}

pub fn delete_request(user: &mut User, _: login::CharacterDeletionRequestPacket) -> HResult {
    let packet = login::CharacterDeletionPacket {
        status: login::DeletionStatus::Success,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterDeletion(packet))?;
    Ok(Action::Nothing)
}

pub fn undelete_request(user: &mut User, _: login::CharacterUndeletionRequestPacket) -> HResult {
    let packet = login::CharacterUndeletionPacket {
        status: login::UndeletionStatus::Success,
    };
    user.send_packet(&Packet::CharacterUndeletion(packet))?;
    Ok(Action::Nothing)
}

pub fn move_request(user: &mut User, _: login::CharacterMoveRequestPacket) -> HResult {
    let packet = login::CharacterMovePacket {
        status: 0,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterMove(packet))?;
    Ok(Action::Nothing)
}

pub fn rename_request(user: &mut User, _: login::CharacterRenameRequestPacket) -> HResult {
    let packet = login::CharacterRenamePacket {
        status: login::RenameRequestStatus::Allowed,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterRename(packet))?;
    Ok(Action::Nothing)
}

pub async fn newname_request(
    user: &mut User,
    packet: login::CharacterNewNameRequestPacket,
) -> HResult {
    let char = user
        .blockdata
        .sql
        .get_character(user.player_id, packet.char_id)
        .await?;
    let mut char = char.character;
    char.name = packet.name.clone();
    user.blockdata.sql.update_character(&char).await?;
    let packet_out = login::CharacterNewNamePacket {
        status: login::NewNameStatus::Success,
        char_id: packet.char_id,
        name: packet.name,
    };
    user.send_packet(&Packet::CharacterNewName(packet_out))?;
    Ok(Action::Nothing)
}

pub async fn new_character(user: &mut User, packet: login::CharacterCreatePacket) -> HResult {
    user.char_id = user
        .blockdata
        .sql
        .put_character(user.player_id, &packet.character)
        .await?;
    let mut character = packet.character;
    character.character_id = user.char_id;
    character.player_id = user.player_id;
    user.character = Some(character);
    user.inventory = user
        .blockdata
        .sql
        .get_inventory(user.char_id, user.player_id)
        .await?;
    user.palette = user.blockdata.sql.get_palette(user.char_id).await?;
    user.send_packet(&Packet::LoadingScreenTransition)?;
    Ok(Action::Nothing)
}

pub async fn start_game(user: &mut User, packet: login::StartGamePacket) -> HResult {
    user.char_id = packet.char_id;
    let char = user
        .blockdata
        .sql
        .get_character(user.player_id, user.char_id)
        .await?;
    user.character = Some(char.character);
    user.charflags = char.flags;
    user.inventory = user
        .blockdata
        .sql
        .get_inventory(user.char_id, user.player_id)
        .await?;
    user.palette = user.blockdata.sql.get_palette(user.char_id).await?;
    user.send_packet(&Packet::LoadingScreenTransition)?;
    Ok(Action::Nothing)
}

pub async fn login_history(user: &mut User) -> HResult {
    let attempts = user.blockdata.sql.get_logins(user.player_id).await?;
    user.send_packet(&Packet::LoginHistoryResponse(login::LoginHistoryPacket {
        attempts,
    }))?;
    Ok(Action::Nothing)
}
