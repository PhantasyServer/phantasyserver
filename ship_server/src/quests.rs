use std::sync::{atomic::AtomicU32, Arc};

use crate::{map::Map, mutex::Mutex, Error};
use data_structs::quest::QuestData;
use pso2packetlib::protocol::{
    party::{SetPartyQuestPacket, SetQuestInfoPacket},
    questlist::{
        AcceptQuestPacket, AvailableQuestsPacket, QuestCategoryPacket, QuestDifficulty, QuestType,
    },
};

pub struct PartyQuest {
    quest: QuestData,
    diff: u16,
    map: Arc<Mutex<Map>>,
}

pub struct Quests {
    quests: Vec<QuestData>,
    availiable: AvailableQuestsPacket,
}

impl Quests {
    pub fn load(dir: &str) -> Self {
        let quests = load_quests(dir);
        let mut amount = AvailableQuestsPacket::default();
        for quest in &quests {
            match quest.definition.quest_type {
                QuestType::Unk0 => {
                    amount.unk1 += 1;
                    amount.available_types.unk1 = true;
                }
                QuestType::Extreme => {
                    amount.extreme_count += 1;
                    amount.available_types.extreme = true;
                }
                QuestType::ARKS => {
                    amount.arks_count += 1;
                    amount.available_types.arks = true;
                }
                QuestType::LimitedTime => {
                    amount.limited_time_count += 1;
                    amount.available_types.limited_time = true;
                }
                QuestType::ExtremeDebug => {
                    amount.extreme_debug_count += 1;
                    amount.available_types.extreme_debug = true;
                }
                QuestType::Blank1 => {
                    amount.blank1_count += 1;
                    amount.available_types.blank1 = true;
                }
                QuestType::NetCafe => {
                    amount.net_cafe_count += 1;
                    amount.available_types.net_cafe = true;
                }
                QuestType::WarmingDebug => {
                    amount.warming_debug_count += 1;
                    amount.available_types.warming_debug = true;
                }
                QuestType::Blank2 => {
                    amount.blank2_count += 1;
                    amount.available_types.blank2 = true;
                }
                QuestType::Advance => {
                    amount.advance_count += 1;
                    amount.available_types.advance = true;
                }
                QuestType::Expedition => {
                    amount.expedition_count += 1;
                    amount.available_types.expedition = true;
                }
                QuestType::FreeDebug => {
                    amount.expedition_debug_count += 1;
                    amount.available_types.free_debug = true;
                }
                QuestType::ArksDebug => {
                    amount.arks_debug_count += 1;
                    amount.available_types.arks_debug = true;
                }
                QuestType::Challenge => {
                    amount.challenge_count += 1;
                    amount.available_types.challenge = true;
                }
                QuestType::Urgent => {
                    amount.urgent_count += 1;
                    amount.available_types.urgent = true;
                }
                QuestType::UrgentDebug => {
                    amount.urgent_debug_count += 1;
                    amount.available_types.urgent_debug = true;
                }
                QuestType::TimeAttack => {
                    amount.time_attack_count += 1;
                    amount.available_types.time_attack = true;
                }
                QuestType::TimeDebug => {
                    amount.time_attack_debug_count += 1;
                    amount.available_types.time_debug = true;
                }
                QuestType::ArksDebug2 => {
                    amount.arks_debug2_count[0] += 1;
                    amount.available_types.arks_debug2 = true;
                }
                QuestType::ArksDebug3 => {
                    amount.arks_debug2_count[1] += 1;
                    amount.available_types.arks_debug3 = true;
                }
                QuestType::ArksDebug4 => {
                    amount.arks_debug2_count[2] += 1;
                    amount.available_types.arks_debug4 = true;
                }
                QuestType::ArksDebug5 => {
                    amount.arks_debug2_count[3] += 1;
                    amount.available_types.arks_debug5 = true;
                }
                QuestType::ArksDebug6 => {
                    amount.arks_debug2_count[4] += 1;
                    amount.available_types.arks_debug6 = true;
                }
                QuestType::ArksDebug7 => {
                    amount.arks_debug2_count[5] += 1;
                    amount.available_types.arks_debug7 = true;
                }
                QuestType::ArksDebug8 => {
                    amount.arks_debug2_count[6] += 1;
                    amount.available_types.arks_debug8 = true;
                }
                QuestType::ArksDebug9 => {
                    amount.arks_debug2_count[7] += 1;
                    amount.available_types.arks_debug9 = true;
                }
                QuestType::ArksDebug10 => {
                    amount.arks_debug2_count[8] += 1;
                    amount.available_types.arks_debug10 = true;
                }
                QuestType::Blank3 => {
                    amount.blank3_count += 1;
                    amount.available_types.blank3 = true;
                }
                QuestType::Recommended => {
                    amount.recommended_count += 1;
                    amount.available_types.recommended = true;
                }
                QuestType::Ultimate => {
                    amount.unk6 += 1;
                    amount.available_types.ultimate = true;
                }
                QuestType::UltimateDebug => {
                    amount.ultimate_debug_count += 1;
                    amount.available_types.ultimate_debug = true;
                }
                QuestType::AGP => {
                    amount.agp_count += 1;
                    amount.available_types.agp = true;
                }
                QuestType::Bonus => {
                    amount.bonus_count += 1;
                    amount.available_types.bonus = true;
                }
                QuestType::StandardTraining => {
                    amount.training_count[0] += 1;
                    amount.available_types.standard_training = true;
                }
                QuestType::HunterTraining => {
                    amount.training_count[1] += 1;
                    amount.available_types.hunter_training = true;
                }
                QuestType::RangerTraining => {
                    amount.training_count[2] += 1;
                    amount.available_types.ranger_training = true;
                }
                QuestType::ForceTraining => {
                    amount.training_count[3] += 1;
                    amount.available_types.force_training = true;
                }
                QuestType::FighterTraining => {
                    amount.training_count[4] += 1;
                    amount.available_types.fighter_training = true;
                }
                QuestType::GunnerTraining => {
                    amount.training_count[5] += 1;
                    amount.available_types.gunner_training = true;
                }
                QuestType::TechterTraining => {
                    amount.training_count[6] += 1;
                    amount.available_types.techter_training = true;
                }
                QuestType::BraverTraining => {
                    amount.training_count[7] += 1;
                    amount.available_types.braver_training = true;
                }
                QuestType::BouncerTraining => {
                    amount.training_count[8] += 1;
                    amount.available_types.bouncer_training = true;
                }
                QuestType::SummonerTraining => {
                    amount.training_count[9] += 1;
                    amount.available_types.summoner_training = true;
                }
                QuestType::AutoAccept => {
                    amount.available_types.auto_accept = true;
                }
                QuestType::Ridroid => {
                    amount.ridroid_count += 1;
                    amount.available_types.ridroid = true;
                }
                QuestType::CafeAGP => {
                    amount.net_cafe_agp_count += 1;
                    amount.available_types.net_cafe_agp = true;
                }
                QuestType::BattleBroken => {
                    amount.battle_broken_count += 1;
                    amount.available_types.battle_broken = true;
                }
                QuestType::BusterDebug => {
                    amount.buster_debug_count += 1;
                    amount.available_types.buster_debug = true;
                }
                QuestType::Poka12 => {
                    amount.poka12_count += 1;
                    amount.available_types.poka12 = true;
                }
                QuestType::StoryEP1 => {
                    amount.unk9 += 1;
                    amount.available_types.unk3 = true;
                }
                QuestType::Buster => {
                    amount.buster_count += 1;
                    amount.available_types.buster = true;
                }
                QuestType::HeroTraining => {
                    amount.hero_training_count += 1;
                    amount.available_types.hero_training = true;
                }
                QuestType::Amplified => {
                    amount.amplified_count += 1;
                    amount.available_types.amplified = true;
                }
                QuestType::DarkBlastTraining => {
                    amount.dark_blast_training_count += 1;
                    amount.available_types.dark_blast_training = true;
                }
                QuestType::Endless => {
                    amount.endless_count += 1;
                    amount.available_types.endless = true;
                }
                QuestType::Blank4 => {
                    amount.unk13 += 1;
                    amount.available_types2.blank4 = true;
                }
                QuestType::PhantomTraining => {
                    amount.phantom_training_count += 1;
                    amount.available_types2.phantom_training = true;
                }
                QuestType::AISTraining => {
                    amount.ais_training_count += 1;
                    amount.available_types2.ais_training = true;
                }
                QuestType::DamageCalculation => {
                    amount.damage_calc_count += 1;
                    amount.available_types2.damage_calc = true;
                }
                QuestType::EtoileTraining => {
                    amount.etoile_training_count += 1;
                    amount.available_types2.etoile_training = true;
                }
                QuestType::Divide => {
                    amount.divide_count += 1;
                    amount.available_types2.divide = true;
                }
                QuestType::Stars1 => {
                    amount.stars1_count += 1;
                    amount.available_types2.stars1 = true;
                }
                QuestType::Stars2 => {
                    amount.stars2_count += 1;
                    amount.available_types2.stars2 = true;
                }
                QuestType::Stars3 => {
                    amount.stars3_count += 1;
                    amount.available_types2.stars3 = true;
                }
                QuestType::Stars4 => {
                    amount.unk15[0] += 1;
                    amount.available_types2.stars4 = true;
                }
                QuestType::Stars5 => {
                    amount.unk15[1] += 1;
                    amount.available_types2.stars5 = true;
                }
                QuestType::Stars6 => {
                    amount.unk16[0] += 1;
                    amount.available_types2.stars6 = true;
                }
            }
        }
        amount.unk19 = amount.available_types.clone();
        amount.unk20 = amount.available_types2.clone();
        Self {
            quests,
            availiable: amount,
        }
    }
    pub fn get_availiable(&self) -> AvailableQuestsPacket {
        self.availiable.clone()
    }
    //FIXME: this will not work for limited time quests
    pub fn get_category(&self, category: QuestType) -> QuestCategoryPacket {
        QuestCategoryPacket {
            quests: self
                .quests
                .iter()
                .filter(|q| q.definition.quest_type == category)
                .map(|q| q.definition.clone())
                .collect(),
        }
    }
    pub fn get_diff(&self, id: u32) -> Option<QuestDifficulty> {
        self.quests
            .iter()
            .find(|q| q.definition.quest_obj.id == id)
            .map(|q| q.difficulties.clone())
    }
    pub fn get_quest(
        &self,
        packet: AcceptQuestPacket,
        map_obj_id: &AtomicU32,
    ) -> Result<PartyQuest, Error> {
        let quest = self
            .quests
            .iter()
            .find(|q| q.definition.quest_obj.id == packet.quest_obj.id);
        if quest.is_none() {
            return Err(Error::InvalidInput("get_quest"));
        }
        let quest = quest.unwrap().clone();
        let map = Arc::new(Mutex::new(Map::new_from_data(
            quest.map.clone(),
            map_obj_id,
        )?));
        Ok(PartyQuest {
            quest,
            diff: packet.diff,
            map,
        })
    }
}

impl PartyQuest {
    pub fn set_party_packet(&self) -> SetPartyQuestPacket {
        SetPartyQuestPacket {
            name: self.quest.definition.name_id,
            difficulty: self.diff as u32,
            quest_type: self.quest.definition.quest_type,
            quest_def: self.quest.definition.clone(),
            quest_diffs: self.quest.difficulties.clone(),
            ..Default::default()
        }
    }
    pub fn set_info_packet(&self) -> SetQuestInfoPacket {
        SetQuestInfoPacket {
            name: self.quest.definition.name_id,
            diff: self.diff as u8,
            quest_type: self.quest.definition.quest_type,
            ..Default::default()
        }
    }
    pub fn get_map(&self) -> Arc<Mutex<Map>> {
        self.map.clone()
    }
}

fn load_quests(dir: &str) -> Vec<QuestData> {
    let mut quests = vec![];
    load_quests_recursive(dir, &mut quests);
    quests
}

fn load_quests_recursive<P: AsRef<std::path::Path>>(path: P, quests: &mut Vec<QuestData>) {
    let dir = std::fs::read_dir(&path);
    if dir.is_err() {
        eprintln!(
            "Failed to read path {}: {}",
            path.as_ref().display(),
            dir.unwrap_err()
        );
        return;
    }
    let dir = dir.unwrap();
    for entry in dir {
        if entry.is_err() {
            eprintln!("Failed to read entry: {}", entry.unwrap_err(),);
            continue;
        }
        let entry = entry.unwrap().path();
        if entry.is_dir() {
            load_quests_recursive(entry, quests);
        } else if entry.is_file() {
            let file = match std::fs::read(&entry) {
                Ok(f) => f,
                Err(e) => {
                    eprintln!("Failed to read file {}: {e}", entry.display());
                    continue;
                }
            };
            let quest = match rmp_serde::from_slice(&file) {
                Ok(q) => q,
                Err(e) => {
                    eprintln!("Failed to deserialize quest {}: {e}", entry.display());
                    continue;
                }
            };
            quests.push(quest);
        }
    }
}
