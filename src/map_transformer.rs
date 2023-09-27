use pso2server::{inventory::ItemParameters, map::MapData};
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
                let data = MapData::load_from_json_file(&filename).unwrap();
                filename.set_extension("mp");
                data.save_to_mp_file(&filename).unwrap();
            } else if filename.extension().unwrap() == "mp" {
                let data = MapData::load_from_mp_file(&filename).unwrap();
                filename.set_extension("json");
                data.save_to_json_file(&filename).unwrap();
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
        _ => panic!("Invalid type"),
    }
}
