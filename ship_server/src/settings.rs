use crate::Error;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub server_name: String,
    pub db_name: String,
    pub blocks: Vec<BlockSettings>,
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            server_name: String::from("phantasyserver"),
            db_name: String::from("ship.db"),
            balance_port: 12000,
            blocks: vec![BlockSettings::default()],
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
