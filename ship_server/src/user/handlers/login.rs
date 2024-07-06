use super::HResult;
use crate::{battle_stats::PlayerStats, user::UserState, Action, Error, User};
use data_structs::master_ship::SetNicknameResult;
use pso2packetlib::protocol::{
    self,
    items::Item,
    login::{self, BlockListPacket, NicknameRequestPacket, NicknameResponsePacket},
    models::character::Race,
    ObjectHeader, Packet, PacketType,
};
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn encryption_request(user: &mut User, _: login::EncryptionRequestPacket) -> HResult {
    let key = user.connection.get_key();
    user.send_packet(&Packet::EncryptionResponse(
        login::EncryptionResponsePacket { data: key },
    ))
    .await?;
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
                Ok(data) => {
                    id = data.id;
                    user.nickname = data.nickname;
                    user.text_lang = packet.text_lang;
                    user.send_packet(&Packet::ChallengeRequest(login::ChallengeRequestPacket {
                        data: vec![0x0C, 0x47, 0x29, 0x91, 0x27, 0x8E, 0x52, 0x22],
                    }))
                    .await?;
                    user.accountflags = data.accountflags;
                    user.isgm = data.isgm;
                    user.uuid = data.last_uuid;
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
            user.uuid = user_psn.last_uuid;
        }
        _ => unreachable!(),
    }
    user.player_id = id;

    if status == login::LoginStatus::Failure {
        user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
            status,
            error,
            blockname: user.blockdata.block_name.clone(),
            ..Default::default()
        }))
        .await?;
        return Ok(Action::Disconnect);
    }

    if user.nickname.is_empty() {
        user.state = UserState::NewUsername;
        user.send_packet(&Packet::NicknameRequest(Default::default()))
            .await?;
        Ok(Action::Nothing)
    } else {
        on_successful_login(user).await
    }
}

pub async fn on_successful_login(user: &mut User) -> HResult {
    let id = user.get_user_id();
    user.send_packet(&Packet::LoginResponse(login::LoginResponsePacket {
        status: login::LoginStatus::Success,
        error: String::new(),
        blockname: user.blockdata.block_name.clone(),
        player: ObjectHeader {
            id,
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        ..Default::default()
    }))
    .await?;
    user.send_item_attrs().await?;
    let info = user.blockdata.sql.get_user_info(id).await?;
    user.send_packet(&Packet::UserInfo(info)).await?;
    user.send_packet(&Packet::SecondPwdOperation(
        pso2packetlib::protocol::login::SecondPwdOperationPacket {
            unk2: 0,
            is_set: 1,
            is_unlocked: 1,
            unk5: 1,
            ..Default::default()
        },
    ))
    .await?;
    user.state = UserState::CharacterSelect;

    Ok(Action::Nothing)
}

pub async fn set_username(user: &mut User, packet: NicknameResponsePacket) -> HResult {
    let sql = user.blockdata.sql.clone();
    let result = sql.set_username(user.player_id, &packet.nickname).await?;
    //FIXME: error code 1 is for nickname == username
    match result {
        SetNicknameResult::Ok => {}
        SetNicknameResult::AlreadyTaken => {
            user.send_packet(&Packet::NicknameRequest(NicknameRequestPacket { error: 1 }))
                .await?;
            return Ok(Action::Nothing);
        }
    }

    on_successful_login(user).await
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
    user.send_packet(&Packet::BlockList(blocks)).await?;
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
            }))
            .await?;
            user.accountflags = x.accountflags;
            user.isgm = x.isgm;
            user.uuid = x.last_uuid;
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
            entity_type: protocol::ObjectType::Player,
            ..Default::default()
        },
        ..Default::default()
    }))
    .await?;
    if let login::LoginStatus::Failure = status {
        return Ok(Action::Disconnect);
    }

    on_successful_login(user).await
}
pub async fn switch_block(user: &mut User, packet: login::BlockSwitchRequestPacket) -> HResult {
    let lock = user.blockdata.blocks.read().await;
    if let Some(block) = lock.iter().find(|b| b.id == packet.block_id as u32) {
        let challenge_data = crate::sql::ChallengeData {
            lang: user.text_lang,
            packet_type: user.packet_type,
        };
        let challenge = user
            .blockdata
            .sql
            .new_challenge(user.player_id, challenge_data)
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
        user.send_packet(&packet).await?;
    }
    Ok(Action::Nothing)
}

pub async fn client_ping(user: &mut User, packet: login::ClientPingPacket) -> HResult {
    let response = login::ClientPongPacket {
        client_time: packet.time,
        server_time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap(),
        unk1: 0,
    };
    user.send_packet(&Packet::ClientPong(response)).await?;
    Ok(Action::Nothing)
}

pub async fn character_list(user: &mut User) -> HResult {
    let mut packet = login::CharacterListPacket::default();
    let characters = user.blockdata.sql.get_characters(user.player_id).await?;
    for character in characters {
        packet.characters.push(character.character);
        let Packet::LoadEquiped(equiped) = character.inventory.send_equiped(0) else {
            unreachable!();
        };
        let mut items: [Item; 10] = Default::default();
        for item in equiped.items {
            if item.unk < 10 {
                items[item.unk as usize] = item.item;
            }
        }
        packet.equiped_items.push(items);
    }
    user.send_packet(&Packet::CharacterListResponse(packet))
        .await?;
    Ok(Action::Nothing)
}

pub async fn character_create1(user: &mut User) -> HResult {
    user.send_packet(&Packet::CreateCharacter1Response(
        login::CreateCharacter1ResponsePacket::default(),
    ))
    .await?;
    Ok(Action::Nothing)
}

pub async fn character_create2(user: &mut User) -> HResult {
    user.send_packet(&Packet::CreateCharacter2Response(
        login::CreateCharacter2ResponsePacket { referral_flag: 1 },
    ))
    .await?;
    Ok(Action::Nothing)
}

pub async fn delete_request(user: &mut User, _: login::CharacterDeletionRequestPacket) -> HResult {
    let packet = login::CharacterDeletionPacket {
        status: login::DeletionStatus::Success,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterDeletion(packet)).await?;
    Ok(Action::Nothing)
}

pub async fn undelete_request(
    user: &mut User,
    _: login::CharacterUndeletionRequestPacket,
) -> HResult {
    let packet = login::CharacterUndeletionPacket {
        status: login::UndeletionStatus::Success,
    };
    user.send_packet(&Packet::CharacterUndeletion(packet))
        .await?;
    Ok(Action::Nothing)
}

pub async fn move_request(user: &mut User, _: login::CharacterMoveRequestPacket) -> HResult {
    let packet = login::CharacterMovePacket {
        status: 0,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterMove(packet)).await?;
    Ok(Action::Nothing)
}

pub async fn rename_request(user: &mut User, _: login::CharacterRenameRequestPacket) -> HResult {
    let packet = login::CharacterRenamePacket {
        status: login::RenameRequestStatus::Allowed,
        ..Default::default()
    };
    user.send_packet(&Packet::CharacterRename(packet)).await?;
    Ok(Action::Nothing)
}

pub async fn newname_request(
    user: &mut User,
    packet: login::CharacterNewNameRequestPacket,
) -> HResult {
    let mut char = user
        .blockdata
        .sql
        .get_character(user.player_id, packet.char_id)
        .await?;
    char.character.name.clone_from(&packet.name);
    user.blockdata.sql.update_character(&char).await?;
    let packet_out = login::CharacterNewNamePacket {
        status: login::NewNameStatus::Success,
        char_id: packet.char_id,
        name: packet.name,
    };
    user.send_packet(&Packet::CharacterNewName(packet_out))
        .await?;
    Ok(Action::Nothing)
}

pub async fn new_character(user: &mut User, packet: login::CharacterCreatePacket) -> HResult {
    let mut char_data = crate::sql::CharData {
        character: packet.character.clone(),
        ..Default::default()
    };
    if !matches!(char_data.character.look.race, Race::Cast) {
        let clothes = user
            .blockdata
            .server_data
            .item_params
            .attrs
            .human_costumes
            .iter()
            .find(|a| a.model == char_data.character.look.costume_id)
            .cloned()
            .ok_or(Error::NoClothes(char_data.character.look.costume_id))?;
        let uuid = user.uuid;
        user.uuid += 1;
        let item = Item {
            uuid,
            id: protocol::items::ItemId {
                item_type: 2,
                id: clothes.id,
                unk3: 0,
                subid: clothes.subid,
            },
            data: protocol::items::ItemType::Clothing(protocol::items::ClothingItem {
                color: char_data.character.look.costume_color.clone(),
                ..Default::default()
            }),
        };
        char_data.inventory.add_item(item);
        char_data.inventory.equip_item(uuid, 3)?;
    }
    let char_id = user
        .blockdata
        .sql
        .put_character(user.player_id, char_data)
        .await?;
    user.send_packet(&Packet::CharacterCreateResponse(
        login::CharacterCreateResponsePacket {
            status: login::CharacterCreationStatus::Success,
            char_id,
        },
    ))
    .await?;
    Ok(Action::Nothing)
}

pub async fn start_game(user: &mut User, packet: login::StartGamePacket) -> HResult {
    let char = user
        .blockdata
        .sql
        .get_character(user.player_id, packet.char_id)
        .await?;
    user.character = Some(char);
    user.send_packet(&Packet::LoadingScreenTransition).await?;
    user.state = UserState::PreInGame;
    user.battle_stats = PlayerStats::build(user)?;
    Ok(Action::Nothing)
}

pub async fn login_history(user: &mut User) -> HResult {
    let attempts = user.blockdata.sql.get_logins(user.player_id).await?;
    user.send_packet(&Packet::LoginHistoryResponse(login::LoginHistoryPacket {
        attempts,
    }))
    .await?;
    Ok(Action::Nothing)
}
