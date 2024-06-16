use data_structs::{
    inventory::ItemParameters,
    map::MapData,
    quest::QuestData,
    stats::{
        AllEnemyStats, ClassStatsStored, EnemyBaseStats, NamedEnemyStats, PlayerStats,
        RaceModifierStored,
    },
    SerDeFile as _, ServerData,
};
use std::{
    env,
    error::Error,
    fs,
    path::{Path, PathBuf},
};

fn main() {
    let mut args = env::args();
    args.next();
    let filename = args.next().expect("Input filename");
    let filename = PathBuf::from(filename);

    let mut server_data = ServerData::default();

    // parse maps
    println!("Parsing maps...");
    let mut map_dir = filename.to_path_buf();
    map_dir.push("maps");
    find_data_dir(&map_dir, parse_map, &mut server_data).unwrap();

    // parse quests
    println!("Parsing quests...");
    let mut quest_dir = filename.to_path_buf();
    quest_dir.push("quests");
    find_data_dir(&quest_dir, parse_quest, &mut server_data).unwrap();

    // parse item names
    println!("Parsing item names...");
    let mut names_file = filename.to_path_buf();
    names_file.push("item_names.json");
    if names_file.is_file() {
        let data = ItemParameters::load_from_json_file(&names_file).unwrap();
        server_data.item_params = data;
    }

    // parse player stats
    println!("Parsing player stats...");
    let mut player_stats_dir = filename.to_path_buf();
    player_stats_dir.push("class_stats");
    server_data.player_stats = parse_player_stats(&player_stats_dir).unwrap();

    // parse enemy stats
    println!("Parsing enemy stats...");
    let mut base_enemy_stats_dir = filename.to_path_buf();
    let mut enemy_stats_dir = filename.to_path_buf();
    base_enemy_stats_dir.push("base_enemy_stats.json");
    enemy_stats_dir.push("enemies");
    server_data.enemy_stats = parse_enemy_stats(&base_enemy_stats_dir, &enemy_stats_dir).unwrap();

    println!("Saving data...");
    let mut out_filename = filename.to_path_buf();
    out_filename.push("com_data.mp");
    server_data.save_to_mp_file(out_filename).unwrap();
}

fn parse_map(path: &Path, srv_data: &mut ServerData) -> Result<(), Box<dyn Error>> {
    let mut data_file = path.to_path_buf();
    data_file.push("data.json");
    println!("\tParsing map data {}...", data_file.display());
    let mut data = MapData::load_from_json_file(&data_file)?;

    collect_map_data(path, &mut data)?;

    data_file.pop();
    let map_name = data_file.file_stem().unwrap().to_string_lossy().to_string();
    srv_data.maps.insert(map_name, data);
    Ok(())
}

fn collect_map_data(map_path: &Path, map: &mut MapData) -> Result<(), Box<dyn Error>> {
    // load lua files
    let mut lua_dir = map_path.to_path_buf();
    lua_dir.push("luas");
    if lua_dir.exists() {
        println!("\t\tParsing lua directory {}...", lua_dir.display());
        traverse_data_dir(lua_dir, &mut |p| {
            let lua = fs::read_to_string(p)?;
            println!("\t\t\tParsing lua {}...", p.display());
            let filename = p.file_stem().unwrap().to_string_lossy().to_string();
            map.luas.insert(filename, lua);
            Ok(())
        })?;
    }

    // load object files
    let mut object_dir = map_path.to_path_buf();
    object_dir.push("objects");
    if object_dir.exists() {
        println!("\t\tParsing object directory {}...", object_dir.display());
        traverse_data_dir(object_dir, &mut |p| {
            println!("\t\t\tParsing object {}...", p.display());
            let mut objects = Vec::load_from_json_file(p)?;
            map.objects.append(&mut objects);
            Ok(())
        })?;
    }

    // load transporters files
    let mut transporter_dir = map_path.to_path_buf();
    transporter_dir.push("transporters");
    if transporter_dir.exists() {
        println!(
            "\t\tParsing transporter directory {}...",
            transporter_dir.display()
        );
        traverse_data_dir(transporter_dir, &mut |p| {
            println!("\t\t\tParsing transporter {}...", p.display());
            let mut objects = Vec::load_from_json_file(p)?;
            map.transporters.append(&mut objects);
            Ok(())
        })?;
    }

    // load event files
    let mut event_dir = map_path.to_path_buf();
    event_dir.push("events");
    if event_dir.exists() {
        println!("\t\tParsing event directory {}...", event_dir.display());
        traverse_data_dir(event_dir, &mut |p| {
            println!("\t\t\tParsing event {}...", p.display());
            let mut objects = Vec::load_from_json_file(p)?;
            map.events.append(&mut objects);
            Ok(())
        })?;
    }

    // load npc files
    let mut npc_dir = map_path.to_path_buf();
    npc_dir.push("npcs");
    if npc_dir.exists() {
        println!("\t\tParsing NPC directory {}...", npc_dir.display());
        traverse_data_dir(npc_dir, &mut |p| {
            println!("\t\t\tParsing NPC {}...", p.display());
            let mut objects = Vec::load_from_json_file(p)?;
            map.npcs.append(&mut objects);
            Ok(())
        })?;
    }
    Ok(())
}

fn parse_quest(path: &Path, srv_data: &mut ServerData) -> Result<(), Box<dyn Error>> {
    let mut data_file = path.to_path_buf();
    data_file.push("data.json");
    println!("\tParsing quest data {}...", data_file.display());
    let mut data = QuestData::load_from_json_file(&data_file)?;

    // load map
    let mut map_dir = path.to_path_buf();
    map_dir.push("map");
    if map_dir.exists() {
        map_dir.push("map.json");
        println!("\t\tParsing quest map data {}...", data_file.display());
        data.map = MapData::load_from_json_file(&map_dir)?;
        map_dir.pop();
        collect_map_data(&map_dir, &mut data.map)?;
    }
    // load enemy files
    let mut enemy_dir = path.to_path_buf();
    enemy_dir.push("enemies");
    if enemy_dir.exists() {
        println!("\t\tParsing enemy directory {}...", enemy_dir.display());
        traverse_data_dir(enemy_dir, &mut |p| {
            println!("\t\t\tParsing enemy {}...", p.display());
            let mut objects = Vec::load_from_json_file(p)?;
            data.enemies.append(&mut objects);
            Ok(())
        })?;
    }

    srv_data.quests.push(data);
    Ok(())
}

fn parse_player_stats(path: &Path) -> Result<PlayerStats, Box<dyn Error>> {
    let mut data = PlayerStats::default();

    // load level modifiers
    let mut level_mod_path = path.to_path_buf();
    level_mod_path.push("level_modifiers.json");
    if level_mod_path.is_file() {
        println!(
            "\tParsing level modifier data {}...",
            level_mod_path.display()
        );
        let mod_data = RaceModifierStored::load_from_json_file(&level_mod_path)?;
        data.modifiers.push(mod_data.human_male);
        data.modifiers.push(mod_data.human_female);
        data.modifiers.push(mod_data.newman_male);
        data.modifiers.push(mod_data.newman_female);
        data.modifiers.push(mod_data.cast_male);
        data.modifiers.push(mod_data.cast_female);
        data.modifiers.push(mod_data.deuman_male);
        data.modifiers.push(mod_data.deuman_female);
    }

    // load class stats
    let mut max_class = 0;
    traverse_data_dir(path, &mut |p| {
        if path.file_name().unwrap().to_string_lossy() == "level_modifiers.json" {
            return Ok(());
        }
        println!("\tParsing class stats data {}...", p.display());
        let stats = ClassStatsStored::load_from_json_file(p)?;
        let class_int = stats.class as usize;
        if class_int >= max_class {
            max_class = class_int;
            data.stats.resize(class_int + 1, Default::default());
        }
        data.stats[class_int] = stats.stats;
        Ok(())
    })?;

    Ok(data)
}

fn parse_enemy_stats(
    base_stats_path: &Path,
    stats_path: &Path,
) -> Result<AllEnemyStats, Box<dyn Error>> {
    let mut data = AllEnemyStats::default();

    // load base stats
    if base_stats_path.is_file() {
        println!(
            "\tParsing base enemy stats data {}...",
            base_stats_path.display()
        );
        data.base = EnemyBaseStats::load_from_json_file(base_stats_path)?;
    }

    // load class stats
    traverse_data_dir(stats_path, &mut |p| {
        println!("\tParsing enemy stats data {}...", p.display());
        let stats = NamedEnemyStats::load_from_json_file(p)?;
        data.enemies.insert(stats.name, stats.stats);
        Ok(())
    })?;

    Ok(data)
}

fn find_data_dir<P, F>(
    path: P,
    callback: F,
    srv_data: &mut ServerData,
) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    F: Fn(&Path, &mut ServerData) -> Result<(), Box<dyn Error>> + Copy,
{
    // find data.json
    if fs::read_dir(&path)?.any(|p| p.unwrap().file_name().to_str().unwrap() == "data.json") {
        return callback(path.as_ref(), srv_data);
    }

    let dir = fs::read_dir(path)?;
    for entry in dir {
        let entry = entry?.path();
        if entry.is_dir() {
            find_data_dir(entry, callback, srv_data)?;
        }
    }
    Ok(())
}

fn traverse_data_dir<P, F>(path: P, callback: &mut F) -> Result<(), Box<dyn Error>>
where
    P: AsRef<Path>,
    F: FnMut(&Path) -> Result<(), Box<dyn Error>>,
{
    for entry in fs::read_dir(path)? {
        let entry = entry?.path();
        if entry.is_dir() {
            traverse_data_dir(entry, callback)?;
        } else if entry.is_file() {
            callback(&entry)?;
        }
    }
    Ok(())
}
