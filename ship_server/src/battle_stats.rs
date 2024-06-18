use crate::{Error, User};

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

impl PlayerStats {
    pub fn build(user: &User) -> Result<Self, Error> {
        let Some(char) = &user.character else {
            unreachable!("User should be in state >= `PreInGame`")
        };
        let mut resulting_stats = Self::default();
        let block_data = user.get_blockdata();
        let player_stats = &block_data.server_data.player_stats;
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

        let equiped_item = char.palette.get_current_item(&char.inventory)?.id;
        let weapon_stats = block_data
            .server_data
            .item_params
            .attrs
            .weapons
            .iter()
            .find(|a| a.id == equiped_item.id && a.subid == equiped_item.subid)
            .cloned()
            .unwrap_or_default();
        resulting_stats.weapon_mel_pwr = weapon_stats.melee_dmg as _;
        resulting_stats.weapon_rng_pwr = weapon_stats.range_dmg as _;
        resulting_stats.weapon_tec_pwr = weapon_stats.gender_force_dmg.force_dmg as _;

        Ok(resulting_stats)
    }
}
