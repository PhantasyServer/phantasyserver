use std::env;

use pso2server::map::MapData;

fn main() {
    let mut args = env::args();
    args.next();
    let filename = args.next().unwrap();
    let mut filename = std::path::PathBuf::from(filename);
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
