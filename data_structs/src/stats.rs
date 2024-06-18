use pso2packetlib::protocol::models::character::Class;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

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

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyLevelBaseStats {
    pub level: u32,
    pub exp: f32,
    pub hp: f32,
    pub max_mel_dmg: f32,
    pub min_mel_dmg: f32,
    pub max_rng_dmg: f32,
    pub min_rng_dmg: f32,
    pub max_tec_dmg: f32,
    pub min_tec_dmg: f32,
    pub mel_def: f32,
    pub rng_def: f32,
    pub tec_def: f32,
    pub dex: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyHitbox {
    pub name: String,
    pub hitbox_id: u32,
    pub damage_mul: f32,
    pub mel_mul: f32,
    pub rng_mul: f32,
    pub tec_mul: f32,
    pub fire_mul: f32,
    pub ice_mul: f32,
    pub thunder_mul: f32,
    pub wind_mul: f32,
    pub light_mul: f32,
    pub dark_mul: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyBaseStats {
    pub levels: Vec<EnemyLevelBaseStats>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyStats {
    pub levels: Vec<EnemyLevelBaseStats>,
    pub hitboxes: Vec<EnemyHitbox>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NamedEnemyStats {
    pub name: String,
    pub stats: EnemyStats,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AllEnemyStats {
    pub base: EnemyBaseStats,
    pub enemies: HashMap<String, EnemyStats>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AttackStatsReadable {
    pub attack_name: String,
    pub damage_name: String,
    pub attack_type: AttackType,
    pub defense_type: AttackType,
    pub damage: DamageTypeReadable,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct AttackStats {
    pub attack_id: u32,
    pub damage_id: u32,
    pub attack_type: AttackType,
    pub defense_type: AttackType,
    pub damage: DamageType,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub enum AttackType {
    #[default]
    Mel,
    Rng,
    Tec,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DamageTypeReadable {
    Generic { mul: f32 },
    PA { name: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum DamageType{
    Generic(f32),
    PA(u32),
}

impl Default for DamageTypeReadable {
    fn default() -> Self {
        Self::Generic { mul: 1.0 }
    }
}

impl Default for DamageType {
    fn default() -> Self {
        Self::Generic(1.0)
    }
}

impl From<DamageTypeReadable> for DamageType {
    fn from(value: DamageTypeReadable) -> Self {
        match value {
            DamageTypeReadable::Generic { mul } => Self::Generic(mul),
            DamageTypeReadable::PA { name } => Self::PA(super::name_to_id(&name)),
        }
    }
}
