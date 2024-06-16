use crate::Error;
use rsa::{
    pkcs8::{DecodePrivateKey, EncodePrivateKey},
    RsaPrivateKey,
};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server_name: String,
    pub db_name: String,
    pub min_ship_id: u32,
    pub max_ship_id: u32,
    pub blocks: Vec<BlockSettings>,
    pub key_file: Option<String>,
    pub balance_port: u16,
    pub master_ship: String,
    pub master_ship_psk: String,
    pub data_file: String,
    pub log_dir: String,
    pub file_log_level: log::LevelFilter,
    pub console_log_level: log::LevelFilter,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct BlockSettings {
    pub port: Option<u16>,
    pub name: String,
    pub max_players: u32,
    pub lobby_map: String,
}

impl Settings {
    pub async fn load(path: &str) -> Result<Self, Error> {
        match tokio::fs::read_to_string(path).await {
            Ok(s) => Ok(toml::from_str(&s)?),
            Err(_) => Self::create_default(path).await,
        }
    }
    pub fn load_key(&self) -> Result<RsaPrivateKey, Error> {
        log::info!("Loading keypair");
        let key = match &self.key_file {
            Some(keyfile_path) => match std::fs::metadata(keyfile_path) {
                Ok(..) => RsaPrivateKey::read_pkcs8_pem_file(keyfile_path)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    log::warn!("Keyfile doesn't exist, creating...");
                    let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024)?;
                    key.write_pkcs8_pem_file(keyfile_path, rsa::pkcs8::LineEnding::default())?;
                    log::info!("Keyfile created.");
                    key
                }
                Err(e) => {
                    log::error!("Failed to load keypair: {e}");
                    return Err(e.into());
                }
            },
            None => {
                let key = RsaPrivateKey::new(&mut rand::thread_rng(), 1024)?;
                log::info!("Keyfile created.");
                key
            }
        };
        log::info!("Loaded keypair");
        Ok(key)
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_name: String::from("phantasyserver"),
            db_name: String::from("ship.db"),
            balance_port: 12000,
            min_ship_id: 1,
            max_ship_id: 10,
            blocks: vec![BlockSettings::default()],
            key_file: None,
            master_ship: String::from("localhost:15000"),
            master_ship_psk: String::from("master_ship_psk"),
            data_file: String::from("data/com_data.mp"),
            log_dir: String::from("logs"),
            file_log_level: log::LevelFilter::Info,
            console_log_level: log::LevelFilter::Debug,
        }
    }
}
impl Default for BlockSettings {
    fn default() -> Self {
        Self {
            port: None,
            name: "Block 1".to_string(),
            max_players: 32,
            lobby_map: "lobby".to_string(),
        }
    }
}

impl Settings {
    pub async fn create_default(path: &str) -> Result<Self, Error> {
        let mut settings = Self::default();
        settings.blocks.push(BlockSettings {
            port: Some(13002),
            name: "Block 2".into(),
            ..Default::default()
        });

        let toml_doc = toml::to_string_pretty(&settings)?;
        tokio::fs::write(path, toml_doc).await?;
        Ok(settings)
    }
}
