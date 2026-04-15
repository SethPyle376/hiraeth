use serde::{Deserialize, Serialize};

fn default_host() -> String {
    "0.0.0.0".to_string()
}

fn default_port() -> u16 {
    4566
}

fn default_database_url() -> String {
    "sqlite://data/db.sqlite".to_string()
}

fn default_web_enabled() -> bool {
    true
}

fn default_web_host() -> String {
    "127.0.0.1".to_string()
}

fn default_web_port() -> u16 {
    4567
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_database_url")]
    pub database_url: String,
    #[serde(default = "default_web_enabled")]
    pub web_enabled: bool,
    #[serde(default = "default_web_host")]
    pub web_host: String,
    #[serde(default = "default_web_port")]
    pub web_port: u16,
}
