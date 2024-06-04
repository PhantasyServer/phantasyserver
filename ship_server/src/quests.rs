use std::sync::{atomic::AtomicU32, Arc};

use crate::{map::Map, mutex::Mutex, Error};
use data_structs::quest::QuestData;
use pso2packetlib::protocol::{
    party::{SetPartyQuestPacket, SetQuestInfoPacket},
    questlist::{
        AcceptQuestPacket, AvailableQuestType, AvailableQuestsPacket, QuestCategoryPacket, QuestDifficulty, QuestType
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
    pub fn load(quests: Vec<QuestData>) -> Self {
        let mut amount = AvailableQuestsPacket::default();
        for quest in &quests {
            match quest.definition.quest_type {
                QuestType::Unk0 => {
                    amount.unk1 += 1;
                    amount.available_types |= AvailableQuestType::UNK1;
                }
                QuestType::Extreme => {
                    amount.extreme_count += 1;
                    amount.available_types |= AvailableQuestType::EXTREME;
                }
                QuestType::ARKS => {
                    amount.arks_count += 1;
                    amount.available_types |= AvailableQuestType::ARKS;
                }
                QuestType::LimitedTime => {
                    amount.limited_time_count += 1;
                    amount.available_types |= AvailableQuestType::LIMITED_TIME;
                }
                QuestType::ExtremeDebug => {
                    amount.extreme_debug_count += 1;
                    amount.available_types |= AvailableQuestType::EXTREME_DEBUG;
                }
                QuestType::Blank1 => {
                    amount.blank1_count += 1;
                    amount.available_types |= AvailableQuestType::BLANK1;
                }
                QuestType::NetCafe => {
                    amount.net_cafe_count += 1;
                    amount.available_types |= AvailableQuestType::NET_CAFE;
                }
                QuestType::WarmingDebug => {
                    amount.warming_debug_count += 1;
                    amount.available_types |= AvailableQuestType::WARMING_DEBUG;
                }
                QuestType::Blank2 => {
                    amount.blank2_count += 1;
                    amount.available_types |= AvailableQuestType::BLANK2;
                }
                QuestType::Advance => {
                    amount.advance_count += 1;
                    amount.available_types |= AvailableQuestType::ADVANCE;
                }
                QuestType::Expedition => {
                    amount.expedition_count += 1;
                    amount.available_types |= AvailableQuestType::EXPEDITION;
                }
                QuestType::FreeDebug => {
                    amount.expedition_debug_count += 1;
                    amount.available_types |= AvailableQuestType::FREE_DEBUG;
                }
                QuestType::ArksDebug => {
                    amount.arks_debug_count += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG;
                }
                QuestType::Challenge => {
                    amount.challenge_count += 1;
                    amount.available_types |= AvailableQuestType::CHALLENGE;
                }
                QuestType::Urgent => {
                    amount.urgent_count += 1;
                    amount.available_types |= AvailableQuestType::URGENT;
                }
                QuestType::UrgentDebug => {
                    amount.urgent_debug_count += 1;
                    amount.available_types |= AvailableQuestType::URGENT_DEBUG;
                }
                QuestType::TimeAttack => {
                    amount.time_attack_count += 1;
                    amount.available_types |= AvailableQuestType::TIME_ATTACK;
                }
                QuestType::TimeDebug => {
                    amount.time_attack_debug_count += 1;
                    amount.available_types |= AvailableQuestType::TIME_DEBUG;
                }
                QuestType::ArksDebug2 => {
                    amount.arks_debug2_count[0] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG2;
                }
                QuestType::ArksDebug3 => {
                    amount.arks_debug2_count[1] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG3;
                }
                QuestType::ArksDebug4 => {
                    amount.arks_debug2_count[2] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG4;
                }
                QuestType::ArksDebug5 => {
                    amount.arks_debug2_count[3] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG5;
                }
                QuestType::ArksDebug6 => {
                    amount.arks_debug2_count[4] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG6;
                }
                QuestType::ArksDebug7 => {
                    amount.arks_debug2_count[5] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG7;
                }
                QuestType::ArksDebug8 => {
                    amount.arks_debug2_count[6] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG8;
                }
                QuestType::ArksDebug9 => {
                    amount.arks_debug2_count[7] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG9;
                }
                QuestType::ArksDebug10 => {
                    amount.arks_debug2_count[8] += 1;
                    amount.available_types |= AvailableQuestType::ARKS_DEBUG10;
                }
                QuestType::Blank3 => {
                    amount.blank3_count += 1;
                    amount.available_types |= AvailableQuestType::BLANK3;
                }
                QuestType::Recommended => {
                    amount.recommended_count += 1;
                    amount.available_types |= AvailableQuestType::RECOMMENDED;
                }
                QuestType::Ultimate => {
                    amount.unk6 += 1;
                    amount.available_types |= AvailableQuestType::ULTIMATE;
                }
                QuestType::UltimateDebug => {
                    amount.ultimate_debug_count += 1;
                    amount.available_types |= AvailableQuestType::ULTIMATE_DEBUG;
                }
                QuestType::AGP => {
                    amount.agp_count += 1;
                    amount.available_types |= AvailableQuestType::AGP;
                }
                QuestType::Bonus => {
                    amount.bonus_count += 1;
                    amount.available_types |= AvailableQuestType::BONUS;
                }
                QuestType::StandardTraining => {
                    amount.training_count[0] += 1;
                    amount.available_types |= AvailableQuestType::STANDARD_TRAINING;
                }
                QuestType::HunterTraining => {
                    amount.training_count[1] += 1;
                    amount.available_types |= AvailableQuestType::HUNTER_TRAINING;
                }
                QuestType::RangerTraining => {
                    amount.training_count[2] += 1;
                    amount.available_types |= AvailableQuestType::RANGER_TRAINING;
                }
                QuestType::ForceTraining => {
                    amount.training_count[3] += 1;
                    amount.available_types |= AvailableQuestType::FORCE_TRAINING;
                }
                QuestType::FighterTraining => {
                    amount.training_count[4] += 1;
                    amount.available_types |= AvailableQuestType::FIGHTER_TRAINING;
                }
                QuestType::GunnerTraining => {
                    amount.training_count[5] += 1;
                    amount.available_types |= AvailableQuestType::GUNNER_TRAINING;
                }
                QuestType::TechterTraining => {
                    amount.training_count[6] += 1;
                    amount.available_types |= AvailableQuestType::TECHTER_TRAINING;
                }
                QuestType::BraverTraining => {
                    amount.training_count[7] += 1;
                    amount.available_types |= AvailableQuestType::BRAVER_TRAINING;
                }
                QuestType::BouncerTraining => {
                    amount.training_count[8] += 1;
                    amount.available_types |= AvailableQuestType::BOUNCER_TRAINING;
                }
                QuestType::SummonerTraining => {
                    amount.training_count[9] += 1;
                    amount.available_types |= AvailableQuestType::SUMMONER_TRAINING;
                }
                QuestType::AutoAccept => {
                    amount.available_types |= AvailableQuestType::AUTO_ACCEPT;
                }
                QuestType::Ridroid => {
                    amount.ridroid_count += 1;
                    amount.available_types |= AvailableQuestType::RIDROID;
                }
                QuestType::CafeAGP => {
                    amount.net_cafe_agp_count += 1;
                    amount.available_types |= AvailableQuestType::NET_CAFE_AGP;
                }
                QuestType::BattleBroken => {
                    amount.battle_broken_count += 1;
                    amount.available_types |= AvailableQuestType::BATTLE_BROKEN;
                }
                QuestType::BusterDebug => {
                    amount.buster_debug_count += 1;
                    amount.available_types |= AvailableQuestType::BUSTER_DEBUG;
                }
                QuestType::Poka12 => {
                    amount.poka12_count += 1;
                    amount.available_types |= AvailableQuestType::POKA12;
                }
                QuestType::StoryEP1 => {
                    amount.unk9 += 1;
                    amount.available_types |= AvailableQuestType::STORY_EP1;
                }
                QuestType::Buster => {
                    amount.buster_count += 1;
                    amount.available_types |= AvailableQuestType::BUSTER;
                }
                QuestType::HeroTraining => {
                    amount.hero_training_count += 1;
                    amount.available_types |= AvailableQuestType::HERO_TRAINING;
                }
                QuestType::Amplified => {
                    amount.amplified_count += 1;
                    amount.available_types |= AvailableQuestType::AMPLIFIED;
                }
                QuestType::DarkBlastTraining => {
                    amount.dark_blast_training_count += 1;
                    amount.available_types |= AvailableQuestType::DARK_BLAST_TRAINING;
                }
                QuestType::Endless => {
                    amount.endless_count += 1;
                    amount.available_types |= AvailableQuestType::ENDLESS;
                }
                QuestType::Blank4 => {
                    amount.unk13 += 1;
                    amount.available_types |= AvailableQuestType::BLANK4;
                }
                QuestType::PhantomTraining => {
                    amount.phantom_training_count += 1;
                    amount.available_types |= AvailableQuestType::PHANTOM_TRAINING;
                }
                QuestType::AISTraining => {
                    amount.ais_training_count += 1;
                    amount.available_types |= AvailableQuestType::AIS_TRAINING;
                }
                QuestType::DamageCalculation => {
                    amount.damage_calc_count += 1;
                    amount.available_types |= AvailableQuestType::DAMAGE_CALC;
                }
                QuestType::EtoileTraining => {
                    amount.etoile_training_count += 1;
                    amount.available_types |= AvailableQuestType::ETOILE_TRAINING;
                }
                QuestType::Divide => {
                    amount.divide_count += 1;
                    amount.available_types |= AvailableQuestType::DIVIDE;
                }
                QuestType::Stars1 => {
                    amount.stars1_count += 1;
                    amount.available_types |= AvailableQuestType::STARS1;
                }
                QuestType::Stars2 => {
                    amount.stars2_count += 1;
                    amount.available_types |= AvailableQuestType::STARS2;
                }
                QuestType::Stars3 => {
                    amount.stars3_count += 1;
                    amount.available_types |= AvailableQuestType::STARS3;
                }
                QuestType::Stars4 => {
                    amount.unk15[0] += 1;
                    amount.available_types |= AvailableQuestType::STARS4;
                }
                QuestType::Stars5 => {
                    amount.unk15[1] += 1;
                    amount.available_types |= AvailableQuestType::STARS5;
                }
                QuestType::Stars6 => {
                    amount.unk16[0] += 1;
                    amount.available_types |= AvailableQuestType::STARS6;
                }
            }
        }
        amount.unk19 = amount.available_types.clone();
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
        let Some(quest) = self
            .quests
            .iter()
            .find(|q| q.definition.quest_obj.id == packet.quest_obj.id)
        else {
            return Err(Error::InvalidInput("get_quest"));
        };
        let map = Arc::new(Mutex::new(Map::new_from_data(
            quest.map.clone(),
            map_obj_id,
        )?));
        Ok(PartyQuest {
            quest: quest.clone(),
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

