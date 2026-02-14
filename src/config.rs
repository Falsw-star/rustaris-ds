use std::{collections::HashMap, fs, io::{Read, Write}, path::PathBuf, str::FromStr};

use serde::{Deserialize, Serialize};
use smart_default::SmartDefault;

#[derive(Serialize, Deserialize, SmartDefault)]
pub struct NetworkConfig {
    #[default("ws://127.0.0.1:5500")]
    pub websocket: String,
    #[default("######################")]
    pub login_token: String,
    #[default("http://127.0.0.1:5500/v1")]
    pub http: String
}

#[derive(Serialize, Deserialize, SmartDefault)]
pub struct LoggerConfig {
    #[default(true)] pub info: bool,
    #[default(true)] pub warning: bool,
    #[default(true)] pub error: bool,
    #[default(true)] pub chat: bool,
    #[default(true)] pub debug: bool,
    #[default(false)] pub generate_file: bool,
    #[default(None)] pub save_path: Option<String>
}

#[derive(Serialize, Deserialize, SmartDefault)]
pub struct PermissionConfig {
    #[default(0)] pub default: i32,
    #[default(0)] pub private: i32,
    pub admins: Vec<String>,
    pub other: HashMap<String, i32>
}

#[derive(Serialize, Deserialize, SmartDefault)]
pub struct Config {
    #[default(0.5)]
    pub heart_beat: f32,
    pub network: NetworkConfig,
    pub logger: LoggerConfig,
    pub permission: PermissionConfig
}
impl Config {
    pub fn init() -> Self {
        let config_path = PathBuf::from_str("config.json").unwrap();
        if config_path.exists() {
            let mut buf = String::new();
            fs::File::open(&config_path).expect("Cannot open config file.")
                .read_to_string(&mut buf).expect("Cannot read config file");
            serde_json::from_str(&buf).expect("Cannot parse config file")
        }
        else {
            let mut config_file = fs::File::create_new(&config_path).unwrap();
            write!(config_file, "{}", serde_json::to_string_pretty(&Self::default())
                .expect("Failed to generate default config"))
                .expect("Failed to write default config file");
            panic!("Created default config file, please edit it and reboot.")
        }
    }
}