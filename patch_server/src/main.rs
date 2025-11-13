use actix_web::{App, HttpRequest, HttpResponse, HttpServer, Responder, get, web};
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fmt::Write,
    hash::{DefaultHasher, Hash, Hasher},
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Deserialize, Default)]
struct Settings {
    domain: String,
    master_url: String,
    master_folder: String,
    patch_url: String,
    patch_folder: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    output_port: bool,
}

#[derive(Serialize, Deserialize)]
struct FileInfo {
    path_hash: u64,
    path: String,
    hash: String,
    size: usize,
    modify_date: SystemTime,
}

struct AppState {
    settings: Settings,
    master_files: Vec<FileInfo>,
    patch_files: Vec<FileInfo>,
}

impl Settings {
    fn load(path: &str) -> Result<Self, Box<dyn Error>> {
        Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
    }
}

impl FileInfo {
    fn load(path: &str) -> Result<Vec<Self>, Box<dyn Error>> {
        Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
    }
    fn save(this: &[Self], path: &str) -> Result<(), Box<dyn Error>> {
        std::fs::write(path, serde_json::to_string(this)?)?;
        Ok(())
    }
}

async fn management_file(data: web::Data<AppState>, req: HttpRequest) -> impl Responder {
    log::debug!("Got management file request from {:?}", req.peer_addr());
    let mut domain = data.settings.domain.clone();
    if data.settings.output_port {
        domain.push_str(&format!(":{}", data.settings.port.unwrap_or(4040)));
    }
    let mut output = String::new();
    let _ = writeln!(&mut output, "IsInMaintenance=0");
    let _ = writeln!(&mut output, "IsExpired=0");
    let _ = writeln!(&mut output, "IsLeavePrecede=1");
    let _ = writeln!(&mut output, "IsThread=0");
    let _ = writeln!(&mut output, "ThreadNum=1");
    let _ = writeln!(&mut output, "TimeOut=30000");
    let _ = writeln!(&mut output, "ParallelFileSize=10485760");
    let _ = writeln!(&mut output, "ParallelThreadNum=1");
    let _ = writeln!(&mut output, "CrcThreadNum=1");
    let _ = writeln!(&mut output, "FileCacheSize=10485760");
    let _ = writeln!(&mut output, "RetryNum=10");
    let _ = writeln!(&mut output, "IsPGO=1");
    let _ = writeln!(&mut output, "CloudThreadControl=1");
    let _ = writeln!(&mut output, "MemoryOptimize=1");
    let _ = writeln!(&mut output, "IsWin10StoreUpdateEnable=0");
    let _ = writeln!(
        &mut output,
        "MasterURL=http://{}/patch_prod/{}/patches/",
        domain, data.settings.master_url
    );
    let _ = writeln!(
        &mut output,
        "PatchURL=http://{}/patch_prod/{}/patches/",
        domain, data.settings.patch_url
    );
    let _ = writeln!(&mut output, "ForceBootParamMSStore=0");
    let _ = writeln!(&mut output, "ForceBootParamEpic=0");
    let _ = writeln!(&mut output, "ForceBootParam=1");
    let _ = writeln!(&mut output, "ForceBootParamSteam=0");
    HttpResponse::Ok().body(output)
}

#[get("/patch_prod/{folder}/patches/{filename:.*}")]
async fn return_file(
    data: web::Data<AppState>,
    url_path: web::Path<(String, String)>,
    req: HttpRequest,
) -> impl Responder {
    let (folder, filename) = url_path.into_inner();
    log::debug!(
        "Got file request from {:?} to {folder}/{filename}",
        req.peer_addr()
    );
    let filename = PathBuf::from(filename);
    let mut clear_filename = PathBuf::new();
    for c in filename.components() {
        match c {
            std::path::Component::Prefix(_) => {}
            std::path::Component::RootDir => {}
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                clear_filename.pop();
            }
            std::path::Component::Normal(os_str) => clear_filename.push(os_str),
        }
    }
    let filename = clear_filename.to_string_lossy().to_string();
    if filename == "patchlist.txt" {
        if folder == data.settings.master_url {
            print_patchlist(&data.master_files)
        } else if folder == data.settings.patch_url {
            print_patchlist(&data.patch_files)
        } else {
            HttpResponse::Forbidden().finish()
        }
    } else {
        let folder = if folder == data.settings.master_url {
            &data.settings.master_folder
        } else if folder == data.settings.patch_url {
            &data.settings.patch_folder
        } else {
            return HttpResponse::Forbidden().finish();
        };
        let mut filename = PathBuf::from(folder);
        filename.push(clear_filename);
        let Ok(file) = std::fs::read(filename) else {
            return HttpResponse::Forbidden().finish();
        };
        HttpResponse::Ok().body(file)
    }
}

fn print_patchlist(patches: &[FileInfo]) -> HttpResponse {
    let mut output = String::new();
    for f in patches {
        let _ = writeln!(&mut output, "{}\t{}\t{}\tp", f.path, f.hash, f.size);
    }
    HttpResponse::Ok().body(output)
}

fn build_filestate(state: &mut Vec<FileInfo>, folder: &str) -> Result<(), Box<dyn Error>> {
    fn traverse<T: AsRef<Path>, Tp: AsRef<Path>>(
        prefix: Tp,
        folder: T,
        files: &mut Vec<FileInfo>,
    ) -> Result<(), Box<dyn Error>> {
        let folder = folder.as_ref();
        let prefix = prefix.as_ref();
        for e in std::fs::read_dir(folder)? {
            let e = e?;
            let meta = e.metadata()?;
            let path = e.path();
            if meta.is_dir() {
                traverse(prefix, path, files)?;
                continue;
            } else if !meta.is_file() {
                continue;
            }
            let modify_date = meta.modified()?;
            let open_path = &path;
            let path = path.strip_prefix(prefix)?;
            let mut hasher = DefaultHasher::new();
            path.hash(&mut hasher);
            let path_hash = hasher.finish();
            let entry_ptr = files.iter_mut().find(|f| f.path_hash == path_hash);
            if let Some(entry) = &entry_ptr {
                let path = path.to_string_lossy().to_string();
                if entry.modify_date == modify_date && entry.path == path {
                    // file exist and not changed, skip hashing
                    continue;
                }
            }

            let data = std::fs::read(open_path)?;
            let mut hasher = md5::Context::new();
            hasher.consume(&data);
            let size = data.len();
            let hash = hasher.finalize();
            let info = FileInfo {
                path_hash,
                path: path.to_string_lossy().to_string(),
                hash: format!("{:X}", hash),
                size,
                modify_date,
            };
            if let Some(entry) = entry_ptr {
                *entry = info;
            } else {
                files.push(info);
            }
        }
        Ok(())
    }

    traverse(folder, folder, state)?;
    Ok(())
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let settings = Settings::load("patch_server.toml")?;
    // setup logging
    {
        use simplelog::*;
        CombinedLogger::init(vec![TermLogger::new(
            log::LevelFilter::Debug,
            Config::default(),
            TerminalMode::Mixed,
            ColorChoice::Auto,
        )])
        .unwrap();
    }

    log::info!("Builing file state for master");
    let mut master_files = FileInfo::load("master_files.json").unwrap_or_default();
    build_filestate(&mut master_files, &settings.master_folder)?;
    log::info!("Builing file state for patch");
    let mut patch_files = FileInfo::load("patch_files.json").unwrap_or_default();
    build_filestate(&mut patch_files, &settings.patch_folder)?;
    FileInfo::save(&master_files, "master_files.json")?;
    FileInfo::save(&patch_files, "patch_files.json")?;

    let data = web::Data::new(AppState {
        settings,
        master_files,
        patch_files,
    });
    let data_copy = data.clone();
    log::info!("Starting server");
    HttpServer::new(move || {
        App::new()
            .app_data(data_copy.clone())
            .route(
                "patch_prod/patches/management_beta.txt",
                web::get().to(management_file),
            )
            .service(return_file)
    })
    .bind(("0.0.0.0", data.settings.port.unwrap_or(4040)))?
    .run()
    .await?;
    Ok(())
}
