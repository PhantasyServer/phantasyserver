use crate::{Error, User};
use data_structs::{stats::EnemyHitbox, ServerData};
use pso2packetlib::protocol::{models::Position, spawn::EnemySpawnPacket};

#[derive(Debug, Clone, Default)]
pub struct PlayerStats {
    max_hp: u32,
    hp: u32,
    dex: u32,

    base_mel_pwr: u32,
    weapon_mel_pwr: u32,
    base_rng_pwr: u32,
    weapon_rng_pwr: u32,
    base_tec_pwr: u32,
    weapon_tec_pwr: u32,

    base_mel_def: u32,
    base_rng_def: u32,
    base_tec_def: u32,
}

#[derive(Debug, Clone, Default)]
pub struct EnemyStats {
    name: String,
    level: u32,
    exp: u32,
    pos: Position,

    max_hp: u32,
    hp: u32,
    dex: u32,

    max_mel_pwr: u32,
    min_mel_pwr: u32,
    max_rng_pwr: u32,
    min_rng_pwr: u32,
    max_tec_pwr: u32,
    min_tec_pwr: u32,

    mel_def: u32,
    rng_def: u32,
    tec_def: u32,

    hitboxes: Vec<EnemyHitbox>,
}

impl PlayerStats {
    pub fn build(user: &User) -> Result<Self, Error> {
        let Some(char) = &user.character else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        let mut resulting_stats = Self::default();
        let server_data = &user.get_blockdata().server_data;
        let player_stats = &server_data.player_stats;
        //TODO: add subclass stats
        let char_data = &char.character;
        let stats = &player_stats.stats[char_data.classes.main_class as usize]
            [char_data.get_level().level1 as usize - 1];

        let modifier_offset = char_data.look.race as usize * 2 + char_data.look.gender as usize;
        let modifiers = &player_stats.modifiers[modifier_offset];

        resulting_stats.hp = (stats.hp + (stats.hp * 0.01 * modifiers.hp as f32).floor()) as _;
        resulting_stats.max_hp = resulting_stats.hp;
        resulting_stats.dex = (stats.dex + (stats.dex * 0.01 * modifiers.dex as f32).floor()) as _;
        resulting_stats.base_mel_pwr =
            (stats.mel_pow + (stats.mel_pow * 0.01 * modifiers.mel_pow as f32).floor()) as _;
        resulting_stats.base_rng_pwr =
            (stats.rng_pow + (stats.rng_pow * 0.01 * modifiers.rng_pow as f32).floor()) as _;
        resulting_stats.base_tec_pwr =
            (stats.tec_pow + (stats.tec_pow * 0.01 * modifiers.tec_pow as f32).floor()) as _;
        resulting_stats.base_mel_def =
            (stats.mel_def + (stats.mel_def * 0.01 * modifiers.mel_def as f32).floor()) as _;
        resulting_stats.base_rng_def =
            (stats.rng_def + (stats.rng_def * 0.01 * modifiers.rng_def as f32).floor()) as _;
        resulting_stats.base_tec_def =
            (stats.tec_def + (stats.tec_def * 0.01 * modifiers.tec_def as f32).floor()) as _;

        if let Some(equiped_item) = char.palette.get_current_item(&char.inventory)? {
            let ids = equiped_item.id;
            let weapon_stats = server_data
                .item_params
                .attrs
                .weapons
                .iter()
                .find(|a| a.id == ids.id && a.subid == ids.subid)
                .cloned()
                .ok_or(Error::NoItemInAttrs(ids.id, ids.subid))?;
            resulting_stats.weapon_mel_pwr = weapon_stats.melee_dmg as _;
            resulting_stats.weapon_rng_pwr = weapon_stats.range_dmg as _;
            resulting_stats.weapon_tec_pwr = weapon_stats.gender_force_dmg.force_dmg as _;
        }
        Ok(resulting_stats)
    }
    pub fn get_hp(&self) -> (u32, u32) {
        (self.hp, self.max_hp)
    }
}

impl EnemyStats {
    pub fn build(name: &str, level: u32, pos: Position, data: &ServerData) -> Result<Self, Error> {
        let mut resulting_stats = Self::default();
        resulting_stats.name = name.to_string();
        resulting_stats.pos = pos;
        let base_stats = &data.enemy_stats.base;
        let enemy_stats = &data
            .enemy_stats
            .enemies
            .get(name)
            .ok_or(Error::NoEnemyData(name.to_string()))?;
        resulting_stats.hitboxes = enemy_stats.hitboxes.clone();
        let base_level_stats = &base_stats.levels[level as usize -1];
        let level_stats = &enemy_stats.levels[level as usize-1];

        resulting_stats.level = level_stats.level;
        resulting_stats.exp = (base_level_stats.exp * level_stats.exp).floor() as _;
        resulting_stats.max_hp = (base_level_stats.hp * level_stats.hp).floor() as _;
        resulting_stats.hp = resulting_stats.max_hp;
        resulting_stats.dex = (base_level_stats.dex * level_stats.dex).floor() as _;
        resulting_stats.max_mel_pwr =
            (base_level_stats.max_mel_dmg * level_stats.max_mel_dmg).floor() as _;
        resulting_stats.min_mel_pwr =
            (base_level_stats.min_mel_dmg * level_stats.min_mel_dmg).floor() as _;
        resulting_stats.max_rng_pwr =
            (base_level_stats.max_rng_dmg * level_stats.max_rng_dmg).floor() as _;
        resulting_stats.min_rng_pwr =
            (base_level_stats.min_rng_dmg * level_stats.min_rng_dmg).floor() as _;
        resulting_stats.max_tec_pwr =
            (base_level_stats.max_tec_dmg * level_stats.max_tec_dmg).floor() as _;
        resulting_stats.min_tec_pwr =
            (base_level_stats.min_tec_dmg * level_stats.min_tec_dmg).floor() as _;
        resulting_stats.mel_def = (base_level_stats.mel_def * level_stats.mel_def).floor() as _;
        resulting_stats.rng_def = (base_level_stats.rng_def * level_stats.rng_def).floor() as _;
        resulting_stats.tec_def = (base_level_stats.tec_def * level_stats.tec_def).floor() as _;

        Ok(resulting_stats)
    }
    pub fn create_spawn_packet(&self, id: u32, map_id: u16) -> EnemySpawnPacket {
        EnemySpawnPacket {
            object: pso2packetlib::protocol::ObjectHeader {
                id,
                entity_type: pso2packetlib::protocol::ObjectType::Object,
                map_id,
                ..Default::default()
            },
            position: self.pos.clone(),
            name: self.name.to_string().into(),
            hp: self.hp,
            level: self.level,
            unk9: [0, 0, 0, 0, 0, 0, 0, 0, 0, 1062804813, 3212836864, 0, 1570802465, 0, 0, 0],
            ..Default::default()
        }
    }
}
