pub(crate) mod handlers;
use crate::{
    invites::PartyInvite,
    map::Map,
    mutex::{Mutex, MutexGuard, RwLock},
    party::{self, Party},
    sql::CharData,
    Action, BlockData, Error,
};
use data_structs::flags::Flags;
use pso2packetlib::{
    protocol::{
        self as Pr, login::Language, models::Position, party::BusyState,
        spawn::CharacterSpawnPacket, Packet, PacketType,
    },
    Connection,
};
use std::{fmt::Display, net::Ipv4Addr, sync::Arc, time::Instant};

pub struct User {
    // ideally all of these should be private
    connection: Connection,
    blockdata: Arc<BlockData>,
    player_id: u32,
    char_id: u32,
    pub position: Position,
    text_lang: Language,
    map: Option<Arc<Mutex<Map>>>,
    pub party: Option<Arc<RwLock<Party>>>,
    pub character: Option<CharData>,
    last_ping: Instant,
    failed_pings: u32,
    pub packet_type: PacketType,
    ready_to_shutdown: bool,
    pub nickname: String,
    pub party_invites: Vec<PartyInvite>,
    pub party_ignore: Pr::party::RejectStatus,
    pub mapid: u32,
    firstload: bool,
    accountflags: Flags,
    pub isgm: bool,
    uuid: u64,
    pub state: UserState,
}

impl User {
    pub(crate) fn new(
        stream: std::net::TcpStream,
        blockdata: Arc<BlockData>,
    ) -> Result<User, Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let mut con = Connection::new(stream, PacketType::Classic, blockdata.key.clone());
        match con.write_packet(&Packet::ServerHello(Pr::server::ServerHelloPacket {
            unk1: 3,
            blockid: blockdata.block_id as u16,
            unk2: 68833280,
        })) {
            Ok(_) => {}
            Err(x) if x.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(User {
            connection: con,
            blockdata,
            player_id: 0,
            char_id: 0,
            character: None,
            map: None,
            party: None,
            position: Default::default(),
            text_lang: Language::Japanese,
            last_ping: Instant::now(),
            failed_pings: 0,
            packet_type: PacketType::Classic,
            ready_to_shutdown: false,
            nickname: String::new(),
            party_invites: vec![],
            party_ignore: Default::default(),
            mapid: 0,
            firstload: true,
            accountflags: Default::default(),
            isgm: false,
            uuid: 0,
            state: UserState::LoggingIn,
        })
    }
    // I hope async guard won't cause me troubles in the future
    pub async fn tick(mut s: MutexGuard<'_, Self>) -> Result<Action, Error> {
        let _ = s.connection.flush();
        if s.ready_to_shutdown && s.last_ping.elapsed().as_millis() >= 500 {
            return Ok(Action::Disconnect);
        }
        if s.failed_pings >= 5 {
            return Ok(Action::Disconnect);
        }
        if s.last_ping.elapsed().as_secs() >= 10 {
            s.last_ping = Instant::now();
            s.failed_pings += 1;
            let _ = s.send_packet(&Packet::ServerPing);
        }
        let packet = s.connection.read_packet()?;
        packet_handler(s, packet).await
    }
    // Helper functions
    pub fn get_ip(&self) -> Result<Ipv4Addr, Error> {
        Ok(self.connection.get_ip()?)
    }
    pub fn send_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        match self.connection.write_packet(packet) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e.into()),
        }
        Ok(())
    }
    pub fn spawn_character(&mut self, packet: CharacterSpawnPacket) -> Result<(), Error> {
        self.send_packet(&Packet::CharacterSpawn(packet))?;
        Ok(())
    }
    pub fn get_current_map(&self) -> Option<Arc<Mutex<Map>>> {
        self.map.clone()
    }
    pub fn get_current_party(&self) -> Option<Arc<RwLock<Party>>> {
        self.party.clone()
    }
    pub fn set_map(&mut self, map: Arc<Mutex<Map>>) {
        self.map = Some(map)
    }
    pub const fn get_user_id(&self) -> u32 {
        self.player_id
    }
    pub const fn get_map_id(&self) -> u32 {
        self.mapid
    }
    pub async fn send_item_attrs(&mut self) -> Result<(), Error> {
        let blockdata = self.blockdata.clone();
        let item_attrs = blockdata.item_attrs.read().await;
        let data = match self.packet_type {
            PacketType::Vita => &item_attrs.vita_attrs,
            _ => &item_attrs.pc_attrs,
        };
        let size = data.len();
        for (id, chunk) in data.chunks(0x32000).enumerate() {
            let packet = Pr::items::ItemAttributesPacket {
                id: 0,
                segment: id as u16,
                total_size: size as u32,
                data: chunk.to_vec(),
            };
            self.send_packet(&Packet::LoadItemAttributes(packet))?;
        }
        Ok(())
    }
    pub fn send_system_msg(
        &mut self,
        msg: &(impl std::fmt::Display + ?Sized),
    ) -> Result<(), Error> {
        self.send_packet(&Packet::SystemMessage(Pr::unk19::SystemMessagePacket {
            message: msg.to_string(),
            msg_type: Pr::unk19::MessageType::SystemMessage,
            ..Default::default()
        }))?;
        Ok(())
    }
    pub fn send_error(&mut self, msg: &(impl std::fmt::Display + ?Sized)) -> Result<(), Error> {
        self.send_packet(&Packet::SystemMessage(Pr::unk19::SystemMessagePacket {
            message: msg.to_string(),
            msg_type: Pr::unk19::MessageType::AdminMessageInstant,
            ..Default::default()
        }))?;
        Ok(())
    }
    pub async fn send_position(
        user: MutexGuard<'_, User>,
        packet: Packet,
    ) -> Result<Action, Error> {
        let id = user.get_user_id();
        let map = user.get_current_map();
        drop(user);
        if let Some(map) = map {
            map.lock().await.send_movement(packet, id).await;
        }
        Ok(Action::Nothing)
    }
}

async fn packet_handler(
    mut user_guard: MutexGuard<'_, User>,
    packet: Packet,
) -> Result<Action, Error> {
    let user: &mut User = &mut user_guard;
    let state = user.state;
    // sidestep borrow checker
    let match_unit = (state, packet);
    use {handlers as H, Packet as P, UserState as US};

    match match_unit {
        // Server packets
        (US::PreInGame, P::InitialLoad) => Ok(Action::InitialLoad),
        (_, P::ServerPong) => {
            user.failed_pings = 0;
            Ok(Action::Nothing)
        }
        (US::InGame, P::MapLoaded(data)) => H::server::map_loaded(user, data).await,
        (US::InGame, P::ToCampship(data)) => H::server::to_campship(user_guard, data).await,
        (US::InGame, P::CampshipDown(data)) => H::server::campship_down(user_guard, data).await,
        (US::InGame, P::CasinoToLobby(data)) => H::server::move_from_casino(user_guard, data).await,
        (US::InGame, P::CasinoTransport(data)) => H::server::move_to_casino(user_guard, data).await,
        (US::InGame, P::BridgeToLobby(data)) => H::server::move_from_bridge(user_guard, data).await,
        (US::InGame, P::BridgeTransport(data)) => H::server::move_to_bridge(user_guard, data).await,
        (US::InGame, P::CafeToLobby(data)) => H::server::move_from_cafe(user_guard, data).await,
        (US::InGame, P::CafeTransport(data)) => H::server::move_to_cafe(user_guard, data).await,

        // Object packets
        (US::InGame, P::Movement(data)) => H::object::movement(user_guard, data).await,
        (US::InGame, P::MovementAction(..)) => User::send_position(user_guard, match_unit.1).await,
        (US::InGame, P::Interact(data)) => H::object::action(user_guard, data).await,
        (US::InGame, P::ActionUpdate(..)) => User::send_position(user_guard, match_unit.1).await,
        (US::InGame, P::MovementEnd(ref data)) => {
            user.position = data.cur_pos;
            User::send_position(user_guard, match_unit.1).await
        }

        // Chat packets
        (US::InGame, P::ChatMessage(..)) => H::chat::send_chat(user_guard, match_unit.1).await,

        // Quest List packets
        (US::InGame, P::AvailableQuestsRequest(data)) => H::quest::avaliable_quests(user, data),
        (US::InGame, P::QuestCategoryRequest(data)) => H::quest::quest_category(user, data),
        (US::InGame, P::QuestDifficultyRequest(data)) => H::quest::quest_difficulty(user, data),
        (US::InGame, P::AcceptQuest(data)) => H::quest::set_quest(user_guard, data).await,
        (US::InGame, P::QuestCounterRequest) => H::quest::counter_request(user),

        // Party packets
        (US::InGame, P::PartyInviteRequest(data)) => Ok(Action::SendPartyInvite(data.invitee.id)),
        (US::InGame, P::AcceptInvite(data)) => H::party::accept_invite(user_guard, data).await,
        (US::InGame, P::LeaveParty) => H::party::leave_party(user_guard).await,
        (US::InGame, P::NewPartySettings(data)) => {
            H::party::set_party_settings(user_guard, data).await
        }
        (US::InGame, P::TransferLeader(data)) => H::party::transfer_leader(user_guard, data).await,
        (US::InGame, P::KickMember(data)) => H::party::kick_player(user_guard, data).await,
        (US::InGame, P::DisbandParty(..)) => H::party::disband_party(user_guard).await,
        (US::InGame, P::ChatStatus(data)) => H::party::set_chat_state(user_guard, data).await,
        (US::InGame, P::GetPartyDetails(data)) => {
            party::Party::get_details(user_guard, data).await?;
            Ok(Action::Nothing)
        }
        (US::InGame, P::SetBusy) => H::party::set_busy_state(user_guard, BusyState::Busy).await,
        (US::InGame, P::SetNotBusy) => {
            H::party::set_busy_state(user_guard, BusyState::NotBusy).await
        }
        (US::InGame, P::SetInviteDecline(data)) => {
            user.party_ignore = data.decline_status;
            Ok(Action::Nothing)
        }
        (US::InGame, P::GetPartyInfo(data)) => {
            party::Party::get_info(user, data).await?;
            Ok(Action::Nothing)
        }

        // Item packets
        (US::InGame, P::MoveToStorageRequest(data)) => H::item::move_to_storage(user, data),
        (US::InGame, P::MoveToInventoryRequest(data)) => H::item::move_to_inventory(user, data),
        (US::InGame, P::MoveMeseta(data)) => H::item::move_meseta(user, data),
        (US::InGame, P::DiscardItemRequest(data)) => H::item::discard_inventory(user, data),
        (US::InGame, P::MoveStoragesRequest(data)) => H::item::move_storages(user, data),
        (US::InGame, P::GetItemDescription(data)) => H::item::get_description(user, data).await,
        (US::InGame, P::DiscardStorageItemRequest(data)) => H::item::discard_storage(user, data),

        // Login packets
        (US::LoggingIn, P::SegaIDLogin(..)) => H::login::login_request(user, match_unit.1).await,
        (US::CharacterSelect, P::CharacterListRequest) => H::login::character_list(user).await,
        (US::CharacterSelect, P::StartGame(data)) => H::login::start_game(user, data).await,
        (US::CharacterSelect, P::CharacterCreate(data)) => {
            H::login::new_character(user, data).await
        }
        (US::CharacterSelect, P::CharacterDeletionRequest(data)) => {
            H::login::delete_request(user, data)
        }
        (_, P::EncryptionRequest(data)) => H::login::encryption_request(user, data),
        (_, P::ClientPing(data)) => H::login::client_ping(user, data),
        (_, P::BlockListRequest) => H::login::block_list(user).await,
        (US::InGame, P::BlockSwitchRequest(data)) => H::login::switch_block(user, data).await,
        (US::LoggingIn, P::BlockLogin(data)) => H::login::challenge_login(user, data).await,
        (US::NewUsername, P::NicknameResponse(data)) => H::login::set_username(user, data).await,
        (_, P::ClientGoodbye) => {
            user.ready_to_shutdown = true;
            user.last_ping = Instant::now();
            Ok(Action::Nothing)
        }
        (_, P::SystemInformation(..)) => Ok(Action::Nothing),
        (US::CharacterSelect, P::CreateCharacter1) => H::login::character_create1(user),
        (US::CharacterSelect, P::CreateCharacter2) => H::login::character_create2(user),
        (US::LoggingIn, P::VitaLogin(..)) => H::login::login_request(user, match_unit.1).await,
        (_, P::ChallengeResponse(..)) => {
            user.packet_type = PacketType::NA;
            user.connection.change_packet_type(PacketType::NA);
            Ok(Action::Nothing)
        }
        (US::CharacterSelect, P::LoginHistoryRequest) => H::login::login_history(user).await,
        (US::CharacterSelect, P::CharacterUndeletionRequest(data)) => {
            H::login::undelete_request(user, data)
        }
        (US::CharacterSelect, P::CharacterRenameRequest(data)) => {
            H::login::rename_request(user, data)
        }
        (US::CharacterSelect, P::CharacterNewNameRequest(data)) => {
            H::login::newname_request(user, data).await
        }
        (US::CharacterSelect, P::CharacterMoveRequest(data)) => H::login::move_request(user, data),

        // Friends packets
        (US::InGame, P::FriendListRequest(data)) => H::friends::get_friends(user, data),

        // Palette packets
        (US::InGame, P::FullPaletteInfoRequest) => H::palette::send_full_palette(user),
        (US::InGame, P::SetPalette(data)) => H::palette::set_palette(user_guard, data).await,
        (US::InGame, P::UpdateSubPalette(data)) => H::palette::update_subpalette(user, data),
        (US::InGame, P::UpdatePalette(data)) => H::palette::update_palette(user_guard, data).await,
        (US::InGame, P::SetSubPalette(data)) => H::palette::set_subpalette(user, data),
        (US::InGame, P::SetDefaultPAs(data)) => H::palette::set_default_pa(user, data),

        // Flag packets
        (US::InGame, P::SetFlag(data)) => H::server::set_flag(user, data).await,
        (US::InGame, P::SkitItemAddRequest(data)) => H::quest::questwork(user_guard, data).await,

        // Settings packets
        (_, P::SettingsRequest) if state >= US::NewUsername => {
            H::settings::settings_request(user).await
        }
        (_, P::SaveSettings(data)) if state >= US::NewUsername => {
            H::settings::save_settings(user, data).await
        }

        // SA packets
        (US::InGame, P::SymbolArtClientDataRequest(data)) => {
            H::symbolart::data_request(user, data).await
        }
        (US::InGame, P::SymbolArtData(data)) => H::symbolart::add_sa(user, data).await,
        (US::InGame, P::ChangeSymbolArt(data)) => H::symbolart::change_sa(user, data).await,
        (US::InGame, P::SymbolArtListRequest) => H::symbolart::list_sa(user).await,
        (US::InGame, P::SendSymbolArt(data)) => H::symbolart::send_sa(user_guard, data).await,

        // ARKS Missions packets
        (US::InGame, P::MissionListRequest) => H::arksmission::mission_list(user),

        // Mission Pass packets
        (US::InGame, P::MissionPassInfoRequest) => H::missionpass::mission_pass_info(user),
        (US::InGame, P::MissionPassRequest) => H::missionpass::mission_pass(user),

        (state, data) => {
            log::debug!(
                "Client {} in state ({state}) sent unhandled packet: {data:?}",
                user.player_id
            );
            Ok(Action::Nothing)
        }
    }
    // Packet::SalonEntryRequest => {
    //     user.send_packet(&Packet::SalonEntryResponse(login::SalonResponse {
    //         ..Default::default()
    //     }))?;
    // }
    // Packet::SegaIDInfoRequest => {
    //     let mut dataout = vec![];
    //     for _ in 0..0x30 {
    //         dataout.push(0x42);
    //         dataout.push(0x41);
    //     }
    //     for _ in 0..0x12 {
    //         dataout.write_u32::<LittleEndian>(1)?;
    //     }
    //     user.send_packet(&Packet::Unknown((
    //         PacketHeader {
    //             id: 0x11,
    //             subid: 108,
    //             ..Default::default()
    //         },
    //         dataout,
    //     )))?;
    // }
}

impl Drop for User {
    fn drop(&mut self) {
        let player_id = self.player_id;
        log::debug!("Dropping user {player_id}");
        if let Some(char) = self.character.take() {
            let sql = self.blockdata.sql.clone();
            let acc_flags = std::mem::take(&mut self.accountflags);
            let uuid = self.uuid;
            tokio::spawn(async move {
                let _ = sql.update_character(&char).await;
                let _ = sql.update_account_storage(player_id, &char.inventory).await;
                let _ = sql.put_account_flags(player_id, acc_flags).await;
                let _ = sql.put_uuid(player_id, uuid).await;
            });
        }
        if let Some(party) = self.party.take() {
            tokio::spawn(async move { party.write().await.remove_player(player_id).await });
        }
        if let Some(map) = self.map.take() {
            tokio::spawn(async move { map.lock().await.remove_player(player_id).await });
        }
        log::debug!("User {player_id} dropped");
    }
}

#[derive(PartialEq, Clone, Copy, PartialOrd, Debug)]
pub enum UserState {
    /// User is logging in, nothing is set up.
    LoggingIn,
    /// User is logged in, but no username was set, only user id is set.
    NewUsername,
    /// User is logged in, account stuff is set up, but no character info.
    CharacterSelect,
    /// User has selected the character, but map and party aren't set up yet.
    PreInGame,
    /// User is in the game, character, map, party are set up.
    InGame,
}

impl Display for UserState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let str = match self {
            UserState::LoggingIn => "Logging in",
            UserState::NewUsername => "Inputting username",
            UserState::CharacterSelect => "Selecting a character",
            UserState::PreInGame => "Waiting for client to load",
            UserState::InGame => "Playing",
        };
        f.write_str(str)
    }
}

#[cfg(test)]
mod test {
    use std::cmp::Ordering;

    use crate::user::UserState;

    #[test]
    fn test_userstate() {
        assert_eq!(
            UserState::InGame
                .partial_cmp(&UserState::CharacterSelect)
                .unwrap(),
            Ordering::Greater
        );
        assert_eq!(
            UserState::CharacterSelect
                .partial_cmp(&UserState::InGame)
                .unwrap(),
            Ordering::Less
        );
        assert!(UserState::InGame > UserState::LoggingIn);
    }
}
