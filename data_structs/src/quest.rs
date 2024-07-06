use crate::map::{MapData, MapId};
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
    pub map: MapData,
    pub enemies: Vec<EnemyData>,
    pub immediate_move: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EnemyData {
    pub difficulty: u16,
    pub mapid: MapId,
    pub data: EnemySpawnPacket,
}
