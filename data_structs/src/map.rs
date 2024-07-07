use pso2packetlib::protocol::{
    models::Position,
    server::{LoadLevelPacket, ZoneSettings},
    spawn::{EventSpawnPacket, NPCSpawnPacket, ObjectSpawnPacket, TransporterSpawnPacket},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type ZoneId = u32;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    pub map_data: LoadLevelPacket,
    pub objects: Vec<ObjectData>,
    pub events: Vec<EventData>,
    pub npcs: Vec<NPCData>,
    pub transporters: Vec<TransporterData>,
    pub luas: HashMap<String, String>,
    pub init_map: ZoneId,
    pub zones: Vec<ZoneData>,
    pub chunks: Vec<ZoneChunk>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ObjectData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: ObjectSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EventData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: EventSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NPCData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: NPCSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TransporterData {
    pub zone_id: ZoneId,
    pub is_active: bool,
    pub data: TransporterSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ZoneData {
    pub name: String,
    pub is_special_zone: bool,
    pub zone_id: ZoneId,
    pub settings: ZoneSettings,
    pub default_location: Position,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ZoneChunk {
    pub zone_id: ZoneId,
    pub chunk_id: u32,
    pub enemy_spawn_enabled: bool,
    pub enemy_spawn_points: Vec<Position>,
}
