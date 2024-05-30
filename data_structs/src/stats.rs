use pso2packetlib::protocol::models::character::Class;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct LevelStats {
    pub exp_to_next: u64,
    pub hp: f32,
    pub pp: f32,
    pub mel_pow: f32,
    pub rng_pow: f32,
    pub tec_pow: f32,
    pub dex: f32,
    pub mel_def: f32,
    pub rng_def: f32,
    pub tec_def: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ClassStatsStored {
    pub class: Class,
    pub stats: Vec<LevelStats>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct PlayerStats {
    pub stats: Vec<Vec<LevelStats>>,
    pub modifiers: Vec<StatMultipliers>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct StatMultipliers {
    pub hp: i8,
    pub mel_pow: i8,
    pub rng_pow: i8,
    pub tec_pow: i8,
    pub dex: i8,
    pub mel_def: i8,
    pub rng_def: i8,
    pub tec_def: i8,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct RaceModifierStored {
    pub human_male: StatMultipliers,
    pub human_female: StatMultipliers,
    pub newman_male: StatMultipliers,
    pub newman_female: StatMultipliers,
    pub cast_male: StatMultipliers,
    pub cast_female: StatMultipliers,
    pub deuman_male: StatMultipliers,
    pub deuman_female: StatMultipliers,
}

