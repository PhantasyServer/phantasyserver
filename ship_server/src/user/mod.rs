pub(crate) mod handlers;
use crate::{
    inventory::Inventory,
    invites::PartyInvite,
    map::Map,
    mutex::{Mutex, MutexGuard, RwLock},
    palette::Palette,
    party::{self, Party},
    Action, BlockData, Error,
};
use data_structs::flags::Flags;
use pso2packetlib::{
    protocol::{
        self as Pr,
        login::Language,
        models::{character::Character, Position},
        party::BusyState,
        spawn::CharacterSpawnPacket,
        Packet, PacketType,
    },
    Connection, PublicKey,
};
use std::{io, net::Ipv4Addr, sync::Arc, time::Instant};

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
    pub character: Option<Character>,
    last_ping: Instant,
    failed_pings: u32,
    pub packet_type: PacketType,
    ready_to_shutdown: bool,
    pub nickname: String,
    pub party_invites: Vec<PartyInvite>,
    pub party_ignore: Pr::party::RejectStatus,
    pub inventory: Inventory,
    pub palette: Palette,
    pub mapid: u32,
    firstload: bool,
    accountflags: Flags,
    charflags: Flags,
    pub isgm: bool,
}

impl User {
    pub(crate) fn new(
        stream: std::net::TcpStream,
        blockdata: Arc<BlockData>,
    ) -> Result<User, Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let mut con = Connection::new(
            stream,
            PacketType::Classic,
            blockdata.key.clone(),
            PublicKey::None,
        );
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
            inventory: Default::default(),
            palette: Default::default(),
            mapid: 0,
            firstload: true,
            accountflags: Default::default(),
            charflags: Default::default(),
            isgm: false,
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
        match s.connection.read_packet() {
            Ok(packet) => match packet_handler(s, packet).await {
                Ok(action) => return Ok(action),
                Err(Error::IOError(x)) if x.kind() == io::ErrorKind::WouldBlock => {}
                Err(Error::IOError(x)) if x.kind() == io::ErrorKind::ConnectionAborted => {
                    return Ok(Action::Disconnect)
                }
                Err(x) => {
                    return Err(x);
                }
            },
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) if x.kind() == io::ErrorKind::ConnectionAborted => {
                return Ok(Action::Disconnect)
            }
            Err(x) => return Err(x.into()),
        }
        Ok(Action::Nothing)
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
    pub fn get_user_id(&self) -> u32 {
        self.player_id
    }
    pub fn get_map_id(&self) -> u32 {
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
    use handlers as H;
    match packet {
        // Server packets
        Packet::InitialLoad => Ok(Action::InitialLoad),
        Packet::ServerPong => {
            user.failed_pings = 0;
            Ok(Action::Nothing)
        }
        Packet::MapLoaded(data) => H::server::map_loaded(user, data).await,
        Packet::ToCampship(data) => H::server::to_campship(user_guard, data).await,
        Packet::CampshipDown(data) => H::server::campship_down(user_guard, data).await,
        Packet::CasinoToLobby(data) => H::server::move_from_casino(user_guard, data).await,
        Packet::CasinoTransport(data) => H::server::move_to_casino(user_guard, data).await,
        Packet::BridgeToLobby(data) => H::server::move_from_bridge(user_guard, data).await,
        Packet::BridgeTransport(data) => H::server::move_to_bridge(user_guard, data).await,
        Packet::CafeToLobby(data) => H::server::move_from_cafe(user_guard, data).await,
        Packet::CafeTransport(data) => H::server::move_to_cafe(user_guard, data).await,

        // Object packets
        Packet::Movement(data) => H::object::movement(user_guard, data).await,
        Packet::MovementAction(..) => User::send_position(user_guard, packet).await,
        Packet::Interact(data) => H::object::action(user_guard, data).await,
        Packet::ActionUpdate(..) => User::send_position(user_guard, packet).await,
        Packet::MovementEnd(ref data) => {
            user.position = data.cur_pos;
            User::send_position(user_guard, packet).await
        }

        // Chat packets
        Packet::ChatMessage(..) => H::chat::send_chat(user_guard, packet).await,

        // Quest List packets
        Packet::AvailableQuestsRequest(data) => H::quest::avaliable_quests(user, data),
        Packet::QuestCategoryRequest(data) => H::quest::quest_category(user, data),
        Packet::QuestDifficultyRequest(data) => H::quest::quest_difficulty(user, data),
        Packet::AcceptQuest(data) => H::quest::set_quest(user_guard, data).await,
        Packet::QuestCounterRequest => H::quest::counter_request(user),

        // Party packets
        Packet::PartyInviteRequest(data) => Ok(Action::SendPartyInvite(data.invitee.id)),
        Packet::AcceptInvite(data) => H::party::accept_invite(user_guard, data).await,
        Packet::LeaveParty => H::party::leave_party(user_guard).await,
        Packet::NewPartySettings(data) => H::party::set_party_settings(user_guard, data).await,
        Packet::TransferLeader(data) => H::party::transfer_leader(user_guard, data).await,
        Packet::KickMember(data) => H::party::kick_player(user_guard, data).await,
        Packet::DisbandParty(..) => H::party::disband_party(user_guard).await,
        Packet::ChatStatus(data) => H::party::set_chat_state(user_guard, data).await,
        Packet::GetPartyDetails(data) => {
            party::Party::get_details(user_guard, data).await?;
            Ok(Action::Nothing)
        }
        Packet::SetBusy => H::party::set_busy_state(user_guard, BusyState::Busy).await,
        Packet::SetNotBusy => H::party::set_busy_state(user_guard, BusyState::NotBusy).await,
        Packet::SetInviteDecline(data) => {
            user.party_ignore = data.decline_status;
            Ok(Action::Nothing)
        }
        Packet::GetPartyInfo(data) => {
            party::Party::get_info(user, data).await?;
            Ok(Action::Nothing)
        }

        // Item packets
        Packet::MoveToStorageRequest(data) => H::item::move_to_storage(user, data).await,
        Packet::MoveToInventoryRequest(data) => H::item::move_to_inventory(user, data).await,
        Packet::MoveMeseta(data) => H::item::move_meseta(user, data),
        Packet::DiscardItemRequest(data) => H::item::discard_inventory(user, data),
        Packet::MoveStoragesRequest(data) => H::item::move_storages(user, data).await,
        Packet::GetItemDescription(data) => H::item::get_description(user, data).await,
        Packet::DiscardStorageItemRequest(data) => H::item::discard_storage(user, data),

        // Login packets
        Packet::SegaIDLogin(..) => H::login::login_request(user, packet).await,
        Packet::CharacterListRequest => H::login::character_list(user).await,
        Packet::StartGame(data) => H::login::start_game(user, data).await,
        Packet::CharacterCreate(data) => H::login::new_character(user, data).await,
        Packet::CharacterDeletionRequest(data) => H::login::delete_request(user, data),
        Packet::EncryptionRequest(data) => H::login::encryption_request(user, data),
        Packet::ClientPing(data) => H::login::client_ping(user, data),
        Packet::BlockListRequest => H::login::block_list(user).await,
        Packet::BlockSwitchRequest(data) => H::login::switch_block(user, data).await,
        Packet::BlockLogin(data) => H::login::challenge_login(user, data).await,
        Packet::ClientGoodbye => {
            user.ready_to_shutdown = true;
            user.last_ping = Instant::now();
            Ok(Action::Nothing)
        }
        Packet::SystemInformation(..) => Ok(Action::Nothing),
        Packet::CreateCharacter1 => H::login::character_create1(user),
        Packet::CreateCharacter2 => H::login::character_create2(user),
        Packet::VitaLogin(..) => H::login::login_request(user, packet).await,
        Packet::ChallengeResponse(..) => {
            user.packet_type = PacketType::NA;
            user.connection.change_packet_type(PacketType::NA);
            Ok(Action::Nothing)
        }
        Packet::LoginHistoryRequest => H::login::login_history(user).await,
        Packet::CharacterUndeletionRequest(data) => H::login::undelete_request(user, data),
        Packet::CharacterRenameRequest(data) => H::login::rename_request(user, data),
        Packet::CharacterNewNameRequest(data) => H::login::newname_request(user, data).await,
        Packet::CharacterMoveRequest(data) => H::login::move_request(user, data),

        // Friends packets
        Packet::FriendListRequest(data) => H::friends::get_friends(user, data),

        // Palette packets
        Packet::FullPaletteInfoRequest => H::palette::send_full_palette(user),
        Packet::SetPalette(data) => H::palette::set_palette(user_guard, data).await,
        Packet::UpdateSubPalette(data) => H::palette::update_subpalette(user, data),
        Packet::UpdatePalette(data) => H::palette::update_palette(user_guard, data).await,
        Packet::SetSubPalette(data) => H::palette::set_subpalette(user, data),
        Packet::SetDefaultPAs(data) => H::palette::set_default_pa(user, data),

        // Flag packets
        Packet::SetFlag(data) => H::server::set_flag(user, data).await,
        Packet::SkitItemAddRequest(data) => H::quest::questwork(user_guard, data).await,

        // Settings packets
        Packet::SettingsRequest => H::settings::settings_request(user).await,
        Packet::SaveSettings(data) => H::settings::save_settings(user, data).await,

        // SA packets
        Packet::SymbolArtClientDataRequest(data) => H::symbolart::data_request(user, data).await,
        Packet::SymbolArtData(data) => H::symbolart::add_sa(user, data).await,
        Packet::ChangeSymbolArt(data) => H::symbolart::change_sa(user, data).await,
        Packet::SymbolArtListRequest => H::symbolart::list_sa(user).await,
        Packet::SendSymbolArt(data) => H::symbolart::send_sa(user_guard, data).await,

        // ARKS Missions packets
        Packet::MissionListRequest => H::arksmission::mission_list(user),

        // Mission Pass packets
        Packet::MissionPassInfoRequest => H::missionpass::mission_pass_info(user),
        Packet::MissionPassRequest => H::missionpass::mission_pass(user),

        data => {
            println!("Client {}: {data:?}", user.player_id);
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
        if self.character.is_some() {
            let sql = self.blockdata.sql.clone();
            let inventory = std::mem::take(&mut self.inventory);
            let palette = std::mem::take(&mut self.palette);
            let char_id = self.char_id;
            let acc_flags = std::mem::take(&mut self.accountflags);
            let char_flags = std::mem::take(&mut self.charflags);
            tokio::spawn(async move {
                let _ = sql.update_inventory(char_id, player_id, &inventory).await;
                let _ = sql.update_palette(char_id, &palette).await;
                let _ = sql.put_account_flags(player_id, acc_flags).await;
                let _ = sql.update_char_flags(char_id, char_flags).await;
            });
        }
        if let Some(party) = self.party.take() {
            tokio::spawn(async move { party.write().await.remove_player(player_id).await });
        }
        if let Some(map) = self.map.take() {
            tokio::spawn(async move { map.lock().await.remove_player(player_id).await });
        }
        println!("User {} dropped", self.player_id);
    }
}
