use crate::{MapId, NewMapData};
use pso2packetlib::protocol::{
    questlist::{Quest, QuestDifficulty},
    spawn::EnemySpawnPacket,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct QuestData {
    pub definition: Quest,
    pub difficulties: QuestDifficulty,
    pub map: NewMapData,
    pub enemies: Vec<EnemyData>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyData {
    pub difficulty: u16,
    pub mapid: MapId,
    pub data: EnemySpawnPacket,
}
