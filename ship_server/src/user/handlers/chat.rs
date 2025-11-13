use super::HResult;
use crate::{Action, mutex::MutexGuard, user::User};
use indicatif::HumanBytes;
use memory_stats::memory_stats;
use pso2packetlib::protocol::{
    ObjectType, Packet, chat::MessageChannel, flag::FlagType, items::ItemId, playerstatus,
};

#[derive(Debug, cmd_derive::ChatCommand)]
enum ChatCommand {
    /// Returns memory usage.
    #[alias("mem")]
    MemUsage,
    /// Starts a concert with the provided ID (only for caller).
    #[alias("start_con")]
    StartConcert {
        concert_id: String,
    },
    /// Plays a cutscene with the provided ID.
    #[alias("start_cut")]
    StartCutscene {
        cutscene_id: String,
    },
    /// Sends an SetTag packet from a concert object.
    #[alias("send_con")]
    SendAsConcertObj {
        action: String,
    },
    /// Returns current position.
    #[alias("get_pos")]
    GetPosition,
    /// Returns a list of objects that are `distance` units away from the player (set to 1.0 if
    /// not provided)
    #[alias("get_close_obj")]
    #[default]
    GetCloseObjects {
        distance: f64,
    },
    /// Sets specified account flags to a specified value. Range can be in form of "flag_start-flag_end" or "flag_id".
    #[alias("set_acc_flags")]
    SetAccountFlags {
        range: String,
        value: u8,
    },
    /// Sets specified character flags to a specified value. Range can be in form of "flag_start-flag_end" or "flag_id".
    #[alias("set_char_flags")]
    SetCharacterFlags {
        range: String,
        value: u8,
    },
    /// Adds a new item to the player inventory.
    AddItem {
        item_type: u16,
        id: u16,
        subid: u16,
    },
    /// Sets a character level to a specified level.
    #[alias("change_lvl")]
    #[alias("set_lvl")]
    ChangeLevel {
        new_level: u16,
    },
    /// Displays the players current stats.
    #[alias("calc_stats")]
    CalculatePlayerStats,
    /// Forces a specified quest to start.
    ForceQuest {
        quest_id: u32,
        difficulty_id: u16,
    },
    /// Spawns a new enemy at the players location.
    SpawnEnemy {
        enemy_name: String,
    },
    /// Dumps the lua state to the 'dump.txt' file the the game root folder.
    DumpLua,
    SendLua {
        lua: String,
    },
    /// Emergency test
    TestEm,
    #[help]
    Help(String),
}

pub async fn send_chat(mut user: MutexGuard<'_, User>, packet: Packet) -> HResult {
    let Packet::ChatMessage(ref data) = packet else {
        unreachable!()
    };
    if data.message.starts_with('!') {
        let args = data.message.strip_prefix("!").unwrap();
        let cmd = ChatCommand::parse(args, user.user_data.isgm);
        let Ok(cmd) = cmd else {
            let err = cmd.unwrap_err();
            user.send_system_msg(&err).await?;
            return Ok(Action::Nothing);
        };
        match cmd {
            ChatCommand::MemUsage => {
                let mem_data_msg = if let Some(mem) = memory_stats() {
                    format!(
                        "Physical memory: {}\nVirtual memory: {}",
                        HumanBytes(mem.physical_mem as u64),
                        HumanBytes(mem.virtual_mem as u64),
                    )
                } else {
                    "Couldn't gather memory info".into()
                };
                user.send_system_msg(&mem_data_msg).await?;
            }
            ChatCommand::StartConcert { concert_id } => {
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.get_user_id(),
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    attribute: format!("Start({concert_id})").into(),
                    ..Default::default()
                });
                user.send_packet(&packet).await?;
            }
            ChatCommand::StartCutscene { cutscene_id } => {
                user.send_packet(&Packet::StartCutscene(
                    pso2packetlib::protocol::questlist::StartCutscenePacket {
                        scene_name: cutscene_id.into(),
                        ..Default::default()
                    },
                ))
                .await?;
            }
            ChatCommand::SendAsConcertObj { action } => {
                let packet = Packet::SetTag(pso2packetlib::protocol::objects::SetTagPacket {
                    receiver: pso2packetlib::protocol::ObjectHeader {
                        id: user.get_user_id(),
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    target: pso2packetlib::protocol::ObjectHeader {
                        id: 1,
                        entity_type: ObjectType::Object,
                        ..Default::default()
                    },
                    object3: pso2packetlib::protocol::ObjectHeader {
                        id: user.get_user_id(),
                        entity_type: ObjectType::Player,
                        ..Default::default()
                    },
                    attribute: action.into(),
                    ..Default::default()
                });
                user.send_packet(&packet).await?;
            }
            ChatCommand::GetPosition => {
                let pos = user.position;
                let pos: pso2packetlib::protocol::models::EulerPosition = pos.into();
                user.send_system_msg(&format!("{pos:?}")).await?;
            }
            ChatCommand::GetCloseObjects { distance } => {
                let distance = if distance == f64::default() {
                    1.0
                } else {
                    distance
                };
                let Some(map) = user.get_current_map() else {
                    unreachable!("User should be in state >= `InGame`")
                };
                let zoneid = user.zone_pos;
                let lock = map.lock().await;
                let objs = lock.get_close_objects(zoneid, |p| user.position.dist_2d(p) < distance);
                let user_pos = user.position;
                for obj in objs {
                    user.send_system_msg(&format!(
                        "Id: {}, Name: {}, Dist: {}",
                        obj.object.id,
                        obj.name,
                        user_pos.dist_2d(&obj.position)
                    ))
                    .await?;
                }
            }
            ChatCommand::SetAccountFlags { range, value } => {
                set_flag_parse(&mut user, FlagType::Account, &range, value).await?
            }
            ChatCommand::SetCharacterFlags { range, value } => {
                set_flag_parse(&mut user, FlagType::Character, &range, value).await?
            }
            ChatCommand::AddItem {
                item_type,
                id,
                subid,
            } => {
                let item_id = ItemId {
                    id,
                    subid,
                    item_type,
                    ..Default::default()
                };
                let user: &mut User = &mut user;
                let character = user.character.as_mut().unwrap();
                let packet = character
                    .inventory
                    .add_default_item(&mut user.user_data.last_uuid, item_id);
                user.send_packet(&packet).await?;
            }
            ChatCommand::ChangeLevel { new_level } => {
                let srv_data = user.blockdata.server_data.clone();
                let Some(char) = user.character.as_mut() else {
                    user.send_system_msg("No character loaded").await?;
                    return Ok(Action::Nothing);
                };
                let exp = if new_level > 1 && new_level < 100 {
                    srv_data.player_stats.stats[char.character.classes.main_class as usize]
                        [new_level as usize - 2]
                        .exp_to_next
                } else {
                    0
                };
                let stats = char.character.get_level_mut();
                let diff = (exp as i64 - stats.exp as i64).abs();
                stats.level1 = new_level;
                stats.exp = exp as _;
                let stats = char.character.get_level();
                let stats2 = char.character.get_sublevel();
                let userexp = playerstatus::EXPReceiver {
                    unk1: 1,
                    unk2: 1,
                    gained: diff as _,
                    total: stats.exp as _,
                    level2: stats.level2,
                    level: stats.level1,
                    gained_sub: 0,
                    total_sub: stats2.exp as _,
                    level2_sub: stats2.level2,
                    level_sub: stats2.level1,
                    class: char.character.classes.main_class,
                    subclass: char.character.classes.sub_class,
                    object: user.create_object_header(),
                    ..Default::default()
                };
                let packet = Packet::GainedEXP(playerstatus::GainedEXPPacket {
                    sender: Default::default(),
                    receivers: vec![userexp],
                });
                user.send_packet(&packet).await?;
            }
            ChatCommand::CalculatePlayerStats => {
                let msg = format!("Stats: {:?}", user.battle_stats);
                user.send_system_msg(&msg).await?;
            }
            ChatCommand::ForceQuest {
                quest_id,
                difficulty_id,
            } => {
                let packet = pso2packetlib::protocol::questlist::AcceptQuestPacket {
                    quest_obj: pso2packetlib::protocol::ObjectHeader {
                        id: quest_id,
                        entity_type: ObjectType::Quest,
                        ..Default::default()
                    },
                    diff: difficulty_id,
                    ..Default::default()
                };
                super::quest::set_quest(user, packet).await?;
            }
            ChatCommand::SpawnEnemy { enemy_name } => {
                let map = user.get_current_map().unwrap();
                let pos = user.position;
                let zone = user.zone_pos;
                drop(user);
                map.lock().await.spawn_enemy(zone, &enemy_name, pos).await?;
            }
            ChatCommand::Help(msg) => {
                user.send_system_msg(&msg).await?;
            }
            ChatCommand::DumpLua => {
                user.send_packet(&Packet::Unk1003(pso2packetlib::protocol::unk10::Unk1003Packet {
                unk1: 1,
                unk2: 1,
                unk3: r#"
                    System.Log.WriteLog("anc")
                    local f = assert(io.open('dump.txt', 'w'));
                    local function printFunc(fn, indent, f)
                        local dumped = string.dump(fn)
                        f:write(indent)
                        for i = 1, #dumped do
                            f:write(string.format("%02X ", dumped:byte(i)))
                            if i % 16 == 0 then f:write("\n" .. indent) end
                        end
                        f:write("\n")
                    end
                    local function printGlobals(tbl, indent, seen)
                        indent = indent or ""
                        seen = seen or {}

                        if seen[tbl] then return end
                        seen[tbl] = true

                        for key, value in pairs(tbl) do
                            local valueType = type(value)
                            f:write(indent .. tostring(key) .. " (" .. valueType .. ")" .. " = " .. tostring(value) .. "\n")
                            if valueType == "table" then
                                printGlobals(value, indent .. "  ", seen)
                            elseif valueType == "function" then
                                pcall(printFunc, value, indent .. "  ", f)
                            end
                        end
                    end

                    -- Start with the global table _G
                    printGlobals(_G)
                    f:close()
                        "#
                .into(),
            }))
            .await?;
            }
            ChatCommand::SendLua { lua } => {
                user.send_packet(&Packet::Unk1003(
                    pso2packetlib::protocol::unk10::Unk1003Packet {
                        unk1: 1,
                        unk2: 1,
                        unk3: lua.into(),
                    },
                ))
                .await?;
            }
            ChatCommand::TestEm => {
                user.send_packet(&Packet::SpawnEmergency(
                    pso2packetlib::protocol::emergency::SpawnEmergencyPacket {
                        object: pso2packetlib::protocol::ObjectHeader {
                            id: 0x2D,
                            unk: 0,
                            entity_type: ObjectType::Object,
                            map_id: 0,
                        },
                        trial_id: "ii_specified_enemy_ahl".into(),
                        unk1: vec![
                            0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 
                            // 1 - meseta
                            // 2 - boss
                            // 3 - meseta
                            // 4 - boss
                            5,
                            0, 0,
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xFF, 0,
                            0, 0xA6, 0x4A, 0xEB, 0xBE, 0xEE, 0xC5, 0, 0,
                            // 1 << 0 - change over
                            // 1 << 1 - red backdrop
                            // 1 << 2 - special
                            // 1 << 3 - voiceover
                            // 1 << 4 - blue backdrop
                            0x18, 0, 0, 0, 
                            // 0 - nothing
                            // 1 - blue
                            // 2 - purple
                            // 3 - green
                            1,
                            2,
                            // type
                            // 0 - attack
                            // 1 - protection
                            // 2 - ellimination
                            // 3 - collect
                            // 4 - duel
                            // 5 - chase
                            // 6 - arrest
                            // 7 - rescue
                            // 8 - destruction
                            // 9 - avoid
                            // 10 - capture
                            // 11 - escort
                            // 12 - present
                            // 13 - clone
                            // 14 - gesture
                            // 15 - vanish
                            // 16 - offfensive
                            // 17 - intercept
                            // 18 - follow
                            // 19 - unite
                            // 20 - search
                            // 21 - hide
                            // 22 - bash
                            // 23 - test
                            // 24 - unknown
                            // 25 - quick
                            // 26 - persona
                            // 27 - judgement
                            // 28 - fortune
                            // 29 - joker
                            // 30 - abyss
                            // 31 - explosion
                            // 32 - variant
                            // 33 - disaster
                            // 34 - challenge
                            // 35 - control
                            // 36 - defeat
                            // 37 - fear
                            // 38 - fusion
                            // 39 - jammer
                            // 40 - keep
                            // 41 - killer
                            // 42 - manage
                            // 43 - negotiation
                            // 44 - regist
                            // 45 - sacrifice
                            // 46 - snipe
                            // 0xFF - skip
                            46, 0,
                        ]
                        .into(),
                        unk2: "IncidentName".into(),
                        unk3: vec![pso2packetlib::protocol::emergency::Unk1502_1 {
                            unk1: vec![
                                0x53, 0x6F, 0x6C, 0x64, 0x69, 0x65, 0x72, 0x41, 0x6E, 0x74, 0x00,
                                0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 2, 0xC, 0, 0, 0,
                            ]
                            .into(),
                        }],
                        unk4: "TrialAbstract".into(),
                        unk5: vec![
                            pso2packetlib::protocol::emergency::Unk1502_1 {
                                unk1: vec![
                                    0x53, 0x6F, 0x6C, 0x64, 0x69, 0x65, 0x72, 0x41, 0x6E, 0x74,
                                    0x00, 0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                    0, 0, 0, 0, 2, 0xC, 0, 0, 0,
                                ]
                                .into(),
                            },
                            pso2packetlib::protocol::emergency::Unk1502_1 {
                                unk1: vec![
                                    0x0, 0x0, 0x0, 0x2, 0x69, 0x45, 0x6E, 0x65, 0x6D, 0x79, 0x4E,
                                    0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                    0, 0, 1, 0x2, 0, 0, 0,
                                ]
                                .into(),
                            },
                        ],
                        fail_conds: vec![
                            pso2packetlib::protocol::emergency::EmergencyCondition {
                                cond_name: "FCondNone".into(),
                                cond_data: vec![],
                            },
                            pso2packetlib::protocol::emergency::EmergencyCondition {
                                cond_name: "".into(),
                                cond_data: vec![],
                            },
                            pso2packetlib::protocol::emergency::EmergencyCondition {
                                cond_name: "".into(),
                                cond_data: vec![],
                            },
                        ]
                        .into(),
                        pass_conds: vec![
                            pso2packetlib::protocol::emergency::EmergencyCondition {
                                cond_name: "ProgNumEnemyKill".into(),
                                cond_data: vec![pso2packetlib::protocol::emergency::Unk1502_1 {
                                    unk1: vec![
                                        0x53, 0x6F, 0x6C, 0x64, 0x69, 0x65, 0x72, 0x41, 0x6E, 0x74,
                                        0x00, 0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                        0, 0, 0, 0, 0, 2, 0xC, 0, 0, 0,
                                    ]
                                    .into(),
                                }],
                            },
                            pso2packetlib::protocol::emergency::EmergencyCondition {
                                cond_name: "".into(),
                                cond_data: vec![],
                            },
                        ]
                        .into(),
                        unk8: 0,
                        unk9: 0,
                        unk10: "NpcComOnBegin".into(),
                        unk11: vec![pso2packetlib::protocol::emergency::Unk1502_1 {
                            unk1: vec![
                                0x53, 0x6F, 0x6C, 0x64, 0x69, 0x65, 0x72, 0x41, 0x6E, 0x74, 0x00,
                                0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 2, 0xC, 0, 0, 0,
                            ]
                            .into(),
                        }],
                        unk12: "TrialBeginMsg".into(),
                        unk13: vec![pso2packetlib::protocol::emergency::Unk1502_1 {
                            unk1: vec![
                                0x53, 0x6F, 0x6C, 0x64, 0x69, 0x65, 0x72, 0x41, 0x6E, 0x74, 0x00,
                                0x61, 0x6D, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                0, 2, 0xC, 0, 0, 0,
                            ]
                            .into(),
                        }],
                        unk14: 0x40200000,
                        unk15: vec![
                            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                            0, 0, 0, 0, 1, 0, 0, 0,
                        ]
                        .into(),
                        // 0 - brigitta
                        // 1 - hilda
                        // 2 - melita
                        // 3 - henrietta
                        // 4 - xiera (regular)
                        // 5 - xiera (omega)
                        unk16: 0,
                        unk17: 0,
                        unk18: "".into(),
                        unk19: vec![],
                        unk20: vec![],
                        unk21: 0xFFFFFFFF,
                    },
                ))
                .await?;
            }
        }
        return Ok(Action::Nothing);
    }
    let id = user.get_user_id();
    match data.channel {
        MessageChannel::Map => {
            let map = user.get_current_map();
            let zone = user.zone_pos;
            drop(user);
            if let Some(map) = map {
                map.lock().await.send_message(zone, packet, id).await;
            }
        }
        MessageChannel::Party => {
            let party = user.get_current_party();
            drop(user);
            if let Some(party) = party {
                party.read().await.send_message(packet, id).await;
            }
        }
        _ => {}
    }
    Ok(Action::Nothing)
}

async fn set_flag_parse(
    user: &mut User,
    ftype: FlagType,
    range: &str,
    val: u8,
) -> Result<(), crate::Error> {
    if range.contains('-') {
        let mut split = range.split('-');
        let lower = split.next().and_then(|r| r.parse().ok());
        let upper = split.next().and_then(|r| r.parse().ok());
        let (Some(lower), Some(upper)) = (lower, upper) else {
            user.send_system_msg("Invalid range").await?;
            return Ok(());
        };
        if lower > upper {
            user.send_system_msg("Invalid range").await?;
            return Ok(());
        }
        for i in lower..=upper {
            set_flag(user, ftype, i, val).await?;
        }
    } else {
        let id = match range.parse() {
            Ok(i) => i,
            Err(_) => {
                user.send_system_msg("Invalid id").await?;
                return Ok(());
            }
        };
        set_flag(user, ftype, id, val).await?;
    }
    Ok(())
}

async fn set_flag(
    user: &mut User,
    ftype: FlagType,
    id: usize,
    val: u8,
) -> Result<(), crate::Error> {
    let character = user.character.as_mut().unwrap();
    match ftype {
        FlagType::Account => user.user_data.accountflags.set(id, val),
        FlagType::Character => character.flags.set(id, val),
    };
    user.send_packet(&Packet::ServerSetFlag(
        pso2packetlib::protocol::flag::ServerSetFlagPacket {
            flag_type: ftype,
            id: id as u32,
            value: val as u32,
            ..Default::default()
        },
    ))
    .await?;

    Ok(())
}
