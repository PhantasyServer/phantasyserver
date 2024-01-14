use data_structs::{quest::QuestData, ItemParameters, MapData, NewMapData, SerDeFile as _};
use std::env;

fn main() {
    let mut args = env::args();
    args.next();
    let filename = args.next().expect("Input filename");
    let data_type = args.next().expect("Input data type");
    let mut filename = std::path::PathBuf::from(filename);
    match data_type.as_str() {
        "map" => {
            if filename.extension().unwrap() == "json" {
                let data = NewMapData::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = NewMapData::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        "oldmap" => {
            if filename.extension().unwrap() == "json" {
                let data = MapData::load_from_json_file(&filename).unwrap();
                let mapid = data.map_data.settings.map_id;
                let mut newdata = NewMapData {
                    map_data: data.map_data,
                    objects: vec![],
                    npcs: vec![],
                    default_location: vec![(mapid, data.default_location)],
                    luas: data.luas,
                    init_map: mapid,
                    ..Default::default()
                };
                for object in data.objects {
                    let id = object.object.id;
                    newdata.objects.push(data_structs::ObjectData {
                        mapid,
                        is_active: true,
                        data: object,
                        lua_data: data.object_data.get(&id).cloned(),
                    })
                }
                for npc in data.npcs {
                    let id = npc.object.id;
                    newdata.npcs.push(data_structs::NPCData {
                        mapid,
                        is_active: true,
                        data: npc,
                        lua_data: data.object_data.get(&id).cloned(),
                    })
                }
                filename.set_extension("mp");
                newdata.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = MapData::load_from_mp_file(&filename).unwrap();
                let mapid = data.map_data.settings.map_id;
                let mut newdata = NewMapData {
                    map_data: data.map_data,
                    objects: vec![],
                    npcs: vec![],
                    default_location: vec![(mapid, data.default_location)],
                    luas: data.luas,
                    init_map: mapid,
                    ..Default::default()
                };
                for object in data.objects {
                    let id = object.object.id;
                    newdata.objects.push(data_structs::ObjectData {
                        mapid,
                        is_active: true,
                        data: object,
                        lua_data: data.object_data.get(&id).cloned(),
                    })
                }
                for npc in data.npcs {
                    let id = npc.object.id;
                    newdata.npcs.push(data_structs::NPCData {
                        mapid,
                        is_active: true,
                        data: npc,
                        lua_data: data.object_data.get(&id).cloned(),
                    })
                }
                filename.set_extension("json");
                newdata.save_to_json_file(&filename).unwrap();
            }
        }
        "item_name" => {
            if filename.extension().unwrap() == "json" {
                let data = ItemParameters::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = ItemParameters::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        "quest" => {
            if filename.extension().unwrap() == "json" {
                let data = QuestData::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = QuestData::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
            }
        }
        _ => panic!("Invalid type"),
    }
}
