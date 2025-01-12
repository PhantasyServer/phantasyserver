use data_structs::{
    map::{self, MapData},
    quest::{EnemyData, QuestData},
};
use pso2packetlib::{
    ppac::{OutputType, PPACReader, PacketData},
    protocol::Packet,
};
use std::{env, fs::File, io::Write};

fn main() {
    let mut args = env::args();
    args.next();
    let filename = args.next().unwrap();

    let mut map_data: Option<MapData> = None;
    let mut quest_data: Vec<QuestData> = vec![];
    let mut mapid = 0;
    let mut quest_id = 0;
    let mut quest_diff = 0;
    let mut populated = vec![];
    let mut accepting_objs = false;

    let out_dir = filename.replace('.', "");
    let _ = std::fs::create_dir(&out_dir);
    let mut ppac = PPACReader::open(File::open(&filename).unwrap()).unwrap();
    ppac.set_out_type(OutputType::Both);

    let mut item_names = {
        let out_name = format!("{out_dir}/item_names.txt");
        File::create(out_name).unwrap()
    };
    let mut item_descs = {
        let out_name = format!("{out_dir}/item_descriptions.txt");
        File::create(out_name).unwrap()
    };

    while let Ok(Some(PacketData {
        time, packet, data, ..
    })) = ppac.read()
    {
        let packet = match packet {
            Some(x) => x,
            None => pso2packetlib::protocol::Packet::Raw(data.unwrap()),
        };
        let time = time.as_nanos();
        match packet {
            Packet::None => break,
            Packet::QuestCategory(p) => {
                for quest in p.quests {
                    if quest_data
                        .iter()
                        .any(|d| d.definition.quest_obj == quest.quest_obj)
                    {
                        continue;
                    }
                    quest_data.push(QuestData {
                        definition: quest,
                        ..Default::default()
                    })
                }
            }
            Packet::QuestDifficulty(p) => {
                for quest in p.quests {
                    if let Some(e_quest) = quest_data
                        .iter_mut()
                        .find(|d| d.definition.quest_obj == quest.quest_obj)
                    {
                        e_quest.difficulties = quest;
                    }
                }
            }
            Packet::AcceptQuest(p) => {
                quest_id = p.quest_obj.id;
                quest_diff = p.diff;
            }
            Packet::EnemySpawn(p) => {
                if let Some(quest) = quest_data
                    .iter_mut()
                    .find(|d| d.definition.quest_obj.id == quest_id)
                {
                    if !quest.enemies.iter().any(|e| e.data.name == p.name) {
                        quest.enemies.push(EnemyData {
                            difficulty: quest_diff,
                            mapid,
                            data: p,
                        })
                    }
                }
            }
            Packet::LoadLevel(p) => {
                if let Some(data) = map_data {
                    let out_name =
                        format!("{out_dir}/map_{}_{}.json", time, data.map_data.unk7.clone());
                    serde_json::to_writer_pretty(&File::create(out_name).unwrap(), &data).unwrap();
                    populated.clear();
                }
                mapid = p.settings.map_id;
                map_data = Some(MapData {
                    map_data: p,
                    objects: vec![],
                    npcs: vec![],
                    init_map: mapid,
                    ..Default::default()
                });
                accepting_objs = true;
            }
            Packet::MapTransfer(p) => {
                populated.push(mapid);
                mapid = p.settings.map_id;
                accepting_objs = true;
            }
            // Packet::CharacterSpawnNGS(p) => {
            //     if let Some(ref mut map) = map_data {
            //         let mut exists = false;
            //         for (id, _) in &map.default_location {
            //             if *id == mapid {
            //                 exists = true;
            //                 break;
            //             }
            //         }
            //         if !exists {
            //             map.default_location.push((mapid, p.position));
            //         }
            //     }
            // }
            Packet::ObjectSpawn(p) => {
                if let Some(ref mut data) = map_data {
                    if populated.contains(&mapid) {
                        continue;
                    }
                    if !accepting_objs {
                        continue;
                    }
                    if data
                        .objects
                        .iter()
                        .map(|o| (o.zone_id, o.data.object.id))
                        .any(|(m, i)| m == mapid && i == p.object.id)
                    {
                        continue;
                    }
                    data.objects.push(map::ObjectData {
                        zone_id: mapid,
                        is_active: true,
                        data: p,
                        lua_data: None,
                    });
                }
            }
            Packet::NPCSpawn(p) => {
                if let Some(ref mut data) = map_data {
                    if populated.contains(&mapid) {
                        continue;
                    }
                    if !accepting_objs {
                        continue;
                    }
                    if data
                        .npcs
                        .iter()
                        .map(|o| (o.zone_id, o.data.object.id))
                        .any(|(m, i)| m == mapid && i == p.object.id)
                    {
                        continue;
                    }
                    data.npcs.push(map::NPCData {
                        zone_id: mapid,
                        is_active: true,
                        data: p,
                        lua_data: None,
                    });
                }
            }
            Packet::EventSpawn(p) => {
                if let Some(ref mut data) = map_data {
                    if populated.contains(&mapid) {
                        continue;
                    }
                    if !accepting_objs {
                        continue;
                    }
                    if data
                        .npcs
                        .iter()
                        .map(|o| (o.zone_id, o.data.object.id))
                        .any(|(m, i)| m == mapid && i == p.object.id)
                    {
                        continue;
                    }
                    data.events.push(map::EventData {
                        zone_id: mapid,
                        is_active: true,
                        data: p,
                        lua_data: None,
                    });
                }
            }
            Packet::TransporterSpawn(p) => {
                if let Some(ref mut data) = map_data {
                    if populated.contains(&mapid) {
                        continue;
                    }
                    if !accepting_objs {
                        continue;
                    }
                    if data
                        .transporters
                        .iter()
                        .map(|o| (o.zone_id, o.data.object.id))
                        .any(|(m, i)| m == mapid && i == p.object.id)
                    {
                        continue;
                    }
                    data.transporters.push(map::TransporterData {
                        zone_id: mapid,
                        is_active: true,
                        data: p,
                        lua_data: None,
                    });
                }
            }
            Packet::LoadItem(p) => {
                for item in p.items {
                    writeln!(
                        &mut item_names,
                        "{}, {}, {} - {}",
                        item.id.item_type, item.id.id, item.id.subid, item.name
                    )
                    .unwrap();
                }
            }
            Packet::LoadItemDescription(p) => {
                writeln!(
                    &mut item_descs,
                    "{}, {}, {} - {}",
                    p.item.item_type, p.item.id, p.item.subid, p.desc,
                )
                .unwrap();
            }
            Packet::FinishLoading => {
                accepting_objs = false;
            }
            _ => {}
        }
    }
    if let Some(data) = map_data {
        let out_name = format!("{out_dir}/map_final_{}.json", data.map_data.unk7.clone());
        serde_json::to_writer_pretty(&File::create(out_name).unwrap(), &data).unwrap();
    }
    for quest in quest_data {
        let out_name = format!("{out_dir}/quest_{}.json", quest.definition.name_id);
        serde_json::to_writer_pretty(&File::create(out_name).unwrap(), &quest).unwrap();
    }
}
