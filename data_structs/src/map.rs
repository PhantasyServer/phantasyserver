use pso2packetlib::protocol::{
    models::Position,
    server::LoadLevelPacket,
    spawn::{EventSpawnPacket, NPCSpawnPacket, ObjectSpawnPacket, TransporterSpawnPacket},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type MapId = u32;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct MapData {
    pub map_data: LoadLevelPacket,
    pub objects: Vec<ObjectData>,
    pub events: Vec<EventData>,
    pub npcs: Vec<NPCData>,
    pub transporters: Vec<TransporterData>,
    pub default_location: Vec<(MapId, Position)>,
    pub luas: HashMap<String, String>,
    pub init_map: MapId,
    pub map_names: HashMap<String, MapId>,
    pub chunks: Vec<ZoneChunk>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ObjectData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: ObjectSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct EventData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: EventSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct NPCData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: NPCSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct TransporterData {
    pub mapid: MapId,
    pub is_active: bool,
    pub data: TransporterSpawnPacket,
    pub lua_data: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct ZoneChunk {
    pub mapid: MapId,
    pub chunk_ids: Vec<u32>,
    pub enemy_spawn_enabled: bool,
    pub enemy_spawn_points: Vec<Position>,
}
