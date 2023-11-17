pub(crate) mod handlers;
use crate::{
    inventory::{Inventory, ItemParameters},
    invites::PartyInvite,
    map::Map,
    palette::Palette,
    party::{self, Party},
    sql::Sql,
    Action, Error,
};
use parking_lot::RwLock;
use pso2packetlib::{
    protocol::{
        self,
        login::Language,
        models::{character::Character, Position},
        party::BusyState,
        spawn::CharacterSpawnPacket,
        Packet, PacketType,
    },
    Connection,
};
use std::{cell::RefCell, io, net::Ipv4Addr, rc::Rc, sync::Arc, time::Instant};

pub struct User {
    pub(crate) connection: Connection,
    pub(crate) sql: Arc<RwLock<Sql>>,
    pub(crate) player_id: u32,
    pub(crate) char_id: u32,
    pub(crate) position: Position,
    pub(crate) text_lang: Language,
    pub(crate) map: Option<Rc<RefCell<Map>>>,
    pub(crate) party: Option<Rc<RefCell<Party>>>,
    pub(crate) character: Option<Character>,
    pub(crate) last_ping: Instant,
    pub(crate) failed_pings: u32,
    pub(crate) packet_type: PacketType,
    pub(crate) ready_to_shutdown: bool,
    pub(crate) blockname: String,
    pub(crate) nickname: String,
    pub(crate) party_invites: Vec<PartyInvite>,
    pub(crate) party_ignore: protocol::party::RejectStatus,
    pub(crate) inventory: Inventory,
    pub(crate) palette: Palette,
    pub(crate) item_attrs: Arc<RwLock<ItemParameters>>,
}

impl User {
    pub fn new(
        stream: std::net::TcpStream,
        sql: Arc<RwLock<Sql>>,
        blockname: String,
        blockid: u16,
        item_attrs: Arc<RwLock<ItemParameters>>,
    ) -> Result<User, Error> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        let mut con = Connection::new(
            stream,
            PacketType::Classic,
            Some("keypair.pem".into()),
            None,
        );
        match con.write_packet(&Packet::ServerHello(protocol::server::ServerHelloPacket {
            blockid,
        })) {
            Ok(_) => {}
            Err(x) if x.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(User {
            connection: con,
            sql,
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
            blockname,
            nickname: String::new(),
            party_invites: vec![],
            party_ignore: Default::default(),
            inventory: Default::default(),
            palette: Default::default(),
            item_attrs,
        })
    }
    pub fn get_ip(&self) -> Result<Ipv4Addr, Error> {
        Ok(self.connection.get_ip()?)
    }
    pub fn tick(&mut self) -> Result<Action, Error> {
        let _ = self.connection.flush();
        if self.ready_to_shutdown && self.last_ping.elapsed().as_millis() >= 500 {
            return Err(Error::IOError(std::io::ErrorKind::ConnectionAborted.into()));
        }
        if self.failed_pings >= 5 {
            return Err(Error::IOError(std::io::ErrorKind::ConnectionAborted.into()));
        }
        if self.last_ping.elapsed().as_secs() >= 10 {
            self.last_ping = Instant::now();
            self.failed_pings += 1;
            let _ = self.connection.write_packet(&Packet::ServerPing);
        }
        match self.connection.read_packet() {
            Ok(packet) => match packet_handler(self, packet) {
                Ok(action) => return Ok(action),
                Err(Error::IOError(x)) if x.kind() == io::ErrorKind::WouldBlock => {}
                Err(x) => return Err(x),
            },
            Err(x) if x.kind() == io::ErrorKind::WouldBlock => {}
            Err(x) => return Err(x.into()),
        }
        Ok(Action::Nothing)
    }
    pub fn send_packet(&mut self, packet: &Packet) -> Result<(), Error> {
        self.connection.write_packet(packet)?;
        Ok(())
    }
    pub fn spawn_character(&mut self, packet: CharacterSpawnPacket) -> Result<(), Error> {
        self.send_packet(&Packet::CharacterSpawn(packet))?;
        Ok(())
    }
    pub fn get_current_map(&self) -> Option<Rc<RefCell<Map>>> {
        self.map.clone()
    }
    pub fn get_current_party(&self) -> Option<Rc<RefCell<Party>>> {
        self.party.clone()
    }
    pub fn set_map(&mut self, map: Rc<RefCell<Map>>) {
        self.map = Some(map)
    }
    pub fn set_party(&mut self, party: Rc<RefCell<Party>>) {
        self.party = Some(party)
    }
    pub fn get_user_id(&self) -> u32 {
        self.player_id
    }
    pub fn save_inventory(&mut self) -> Result<(), Error> {
        if self.character.is_some() {
            let mut sql = self.sql.write();
            sql.update_inventory(self.char_id, self.player_id, &self.inventory)
        } else {
            Ok(())
        }
    }
    pub fn save_palette(&mut self) -> Result<(), Error> {
        if self.character.is_some() {
            let mut sql = self.sql.write();
            sql.update_palette(self.char_id, &self.palette)
        } else {
            Ok(())
        }
    }
    pub fn send_item_attrs(&mut self) -> Result<(), Error> {
        let item_attrs = self.item_attrs.read();
        let data = match self.packet_type {
            PacketType::Vita => &item_attrs.vita_attrs,
            _ => &item_attrs.pc_attrs,
        };
        let size = data.len();
        for (id, chunk) in data.chunks(0x32000).enumerate() {
            let packet = protocol::items::ItemAttributesPacket {
                id: 0,
                segment: id as u16,
                total_size: size as u32,
                data: chunk.to_vec(),
            };
            self.connection
                .write_packet(&Packet::LoadItemAttributes(packet))?;
        }
        Ok(())
    }
    pub fn send_system_msg(
        &mut self,
        msg: &(impl std::fmt::Display + ?Sized),
    ) -> Result<(), Error> {
        self.send_packet(&Packet::SystemMessage(protocol::SystemMessagePacket {
            message: msg.to_string(),
            msg_type: pso2packetlib::protocol::MessageType::SystemMessage,
            ..Default::default()
        }))?;
        Ok(())
    }
    pub fn send_error(&mut self, msg: &(impl std::fmt::Display + ?Sized)) -> Result<(), Error> {
        self.send_packet(&Packet::SystemMessage(protocol::SystemMessagePacket {
            message: msg.to_string(),
            msg_type: pso2packetlib::protocol::MessageType::AdminMessageInstant,
            ..Default::default()
        }))?;
        Ok(())
    }
}

fn packet_handler(user: &mut User, packet: Packet) -> Result<Action, Error> {
    match packet {
        Packet::EncryptionRequest(data) => handlers::login::encryption_request(user, data),
        Packet::SegaIDLogin(..) => handlers::login::login_request(user, packet),
        Packet::VitaLogin(..) => handlers::login::login_request(user, packet),
        Packet::ServerPong => {
            user.failed_pings = 0;
            Ok(Action::Nothing)
        }
        Packet::SettingsRequest => handlers::settings::settings_request(user),
        Packet::SaveSettings(data) => handlers::settings::save_settings(user, data),
        Packet::ClientPing(data) => handlers::login::client_ping(user, data),
        Packet::ClientGoodbye => {
            user.ready_to_shutdown = true;
            user.last_ping = Instant::now();
            Ok(Action::Nothing)
        }
        Packet::FriendListRequest(data) => handlers::friends::get_friends(user, data),
        Packet::CharacterListRequest => handlers::login::character_list(user),
        Packet::CreateCharacter1 => handlers::login::character_create1(user),
        Packet::CreateCharacter2 => handlers::login::character_create2(user),
        Packet::CharacterCreate(data) => handlers::login::new_character(user, data),
        Packet::CharacterDeletionRequest(data) => handlers::login::delete_request(user, data),
        Packet::CharacterUndeletionRequest(data) => handlers::login::undelete_request(user, data),
        Packet::CharacterMoveRequest(data) => handlers::login::move_request(user, data),
        Packet::CharacterRenameRequest(data) => handlers::login::rename_request(user, data),
        Packet::CharacterNewNameRequest(data) => handlers::login::newname_request(user, data),
        Packet::StartGame(data) => handlers::login::start_game(user, data),
        Packet::LoginHistoryRequest => handlers::login::login_history(user),
        Packet::BlockListRequest => handlers::login::block_list(user),
        Packet::ChallengeResponse(..) => {
            user.packet_type = PacketType::NA;
            user.connection.change_packet_type(PacketType::NA);
            Ok(Action::Nothing)
        }
        Packet::SystemInformation(..) => Ok(Action::Nothing),
        Packet::InitialLoad => Ok(Action::InitialLoad),
        Packet::Movement(data) => handlers::object::movement(user, data),
        Packet::MovementEnd(ref data) => {
            user.position = data.cur_pos;
            Ok(Action::SendPosition(packet))
        }
        Packet::MovementAction(..) => Ok(Action::SendPosition(packet)),
        Packet::ActionUpdate(..) => Ok(Action::SendPosition(packet)),
        Packet::Interact(data) => Ok(Action::Interact(data)),
        Packet::ChatMessage(..) => handlers::chat::send_chat(user, packet),
        Packet::SymbolArtListRequest => handlers::symbolart::list_sa(user),
        Packet::ChangeSymbolArt(data) => handlers::symbolart::change_sa(user, data),
        Packet::SymbolArtData(data) => handlers::symbolart::add_sa(user, data),
        Packet::SymbolArtClientDataRequest(data) => handlers::symbolart::data_request(user, data),
        Packet::SendSymbolArt(data) => handlers::symbolart::send_sa(user, data),
        Packet::MapLoaded(data) => handlers::server::map_loaded(user, data),
        Packet::QuestCounterRequest => handlers::quest::counter_request(user),
        Packet::AvailableQuestsRequest(data) => handlers::quest::avaliable_quests(user, data),
        Packet::MissionListRequest => handlers::arksmission::mission_list(user),
        Packet::MissionPassInfoRequest => handlers::missionpass::mission_pass_info(user),
        Packet::MissionPassRequest => handlers::missionpass::mission_pass(user),

        Packet::PartyInviteRequest(data) => Ok(Action::SendPartyInvite(data.invitee.id)),
        Packet::GetPartyInfo(data) => {
            party::Party::get_info(user, data)?;
            Ok(Action::Nothing)
        }
        Packet::GetPartyDetails(data) => Ok(Action::GetPartyDetails(data)),
        Packet::AcceptInvite(protocol::party::AcceptInvitePacket { party_object, .. }) => {
            Ok(Action::AcceptPartyInvite(party_object.id))
        }
        Packet::NewPartySettings(data) => Ok(Action::SetPartySettings(data)),
        Packet::LeaveParty => Ok(Action::LeaveParty),
        Packet::TransferLeader(data) => Ok(Action::TransferLeader(data.target)),
        Packet::DisbandParty(..) => Ok(Action::DisbandParty),
        Packet::KickMember(data) => Ok(Action::KickPartyMember(data.member)),
        Packet::SetInviteDecline(protocol::party::InviteDeclinePacket { decline_status }) => {
            user.party_ignore = decline_status;
            Ok(Action::Nothing)
        }
        Packet::MoveToStorageRequest(data) => handlers::item::move_to_storage(user, data),
        Packet::MoveToInventoryRequest(data) => handlers::item::move_to_inventory(user, data),
        Packet::MoveStoragesRequest(data) => handlers::item::move_storages(user, data),
        Packet::MoveMeseta(data) => handlers::item::move_meseta(user, data),
        Packet::DiscardItemRequest(data) => handlers::item::discard_inventory(user, data),
        Packet::DiscardStorageItemRequest(data) => handlers::item::discard_storage(user, data),
        Packet::GetItemDescription(data) => handlers::item::get_description(user, data),
        Packet::SetBusy => Ok(Action::SetBusyState(BusyState::Busy)),
        Packet::SetNotBusy => Ok(Action::SetBusyState(BusyState::NotBusy)),
        Packet::ChatStatus(data) => Ok(Action::SetChatState(data)),
        Packet::FullPaletteInfoRequest => handlers::palette::send_full_palette(user),
        Packet::SetPalette(data) => handlers::palette::set_palette(user, data),
        Packet::SetSubPalette(data) => handlers::palette::set_subpalette(user, data),
        Packet::UpdatePalette(data) => handlers::palette::update_palette(user, data),
        Packet::UpdateSubPalette(data) => handlers::palette::update_subpalette(user, data),
        Packet::SetDefaultPAs(data) => handlers::palette::set_default_pa(user, data),
        data => {
            println!("{data:?}");
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
