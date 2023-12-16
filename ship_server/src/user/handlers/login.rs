use crate::{Action, Error, User};
use pso2packetlib::{
    ppac::Direction,
    protocol::{self, login, ObjectHeader, Packet, PacketType},
};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
                }
                Err(Error::InvalidPassword(_)) => {
                    status = login::LoginStatus::Failure;
                    error = "Invalid username or password".to_string();
                }
                Err(Error::InvalidInput) => {
                    status = login::LoginStatus::Failure;
                    error = "Empty username or password".to_string();
                }
                Err(e) => return Err(e),
            }
        }
        Packet::VitaLogin(packet) => {
            user.packet_type = PacketType::Vita;
            user.connection.change_packet_type(PacketType::Vita);
            let user_psn = user.sql.get_psn_user(&packet.username, ip).await?;
            user.nickname = user_psn.nickname;
            id = user_psn.id;
        }
        _ => unreachable!(),
    }
    user.player_id = id;

    user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
        status,
        error,
        blockname: user.blockname.clone(),
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
    user.send_item_attrs()?;
    let packett = protocol::login::UserInfoPacket {
        fun: 1,
        free_sg: protocol::models::SGValue(4.0),
        pq_expiration: Duration::from_secs(1889272266),
        material_storage_expiration: Duration::from_secs(1989272266),
        ..Default::default()
    };
    user.send_packet(&Packet::UserInfo(packett))?;

    Ok(Action::Nothing)
}

pub fn block_list(user: &mut User) -> HResult {
    let packet = serde_json::from_str(&std::fs::read_to_string("block.json")?)?;
    println!("{:?}", packet);
    user.send_packet(&Packet::BlockList(packet))?;
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
        characters: user.sql.get_characters(user.player_id).await?,
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
    let mut char = user
        .sql
        .get_character(user.player_id, packet.char_id)
        .await?;
    char.name = packet.name.clone();
    user.sql.update_character(&char).await?;
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
        .sql
        .put_character(user.player_id, &packet.character)
        .await?;
    let mut character = packet.character;
    character.character_id = user.char_id;
    character.player_id = user.player_id;
    user.character = Some(character);
    user.inventory = user.sql.get_inventory(user.char_id, user.player_id).await?;
    user.palette = user.sql.get_palette(user.char_id).await?;
    user.send_packet(&Packet::LoadingScreenTransition)?;
    Ok(Action::Nothing)
}

pub async fn start_game(user: &mut User, packet: login::StartGamePacket) -> HResult {
    user.char_id = packet.char_id;
    user.character = Some(user.sql.get_character(user.player_id, user.char_id).await?);
    user.inventory = user.sql.get_inventory(user.char_id, user.player_id).await?;
    user.palette = user.sql.get_palette(user.char_id).await?;
    user.send_packet(&Packet::LoadingScreenTransition)?;
    Ok(Action::Nothing)
}

pub async fn login_history(user: &mut User) -> HResult {
    let attempts = user.sql.get_logins(user.player_id).await?;
    user.send_packet(&Packet::LoginHistoryResponse(login::LoginHistoryPacket {
        attempts,
    }))?;
    Ok(Action::Nothing)
}
