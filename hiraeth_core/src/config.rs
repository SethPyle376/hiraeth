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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_database_url")]
    pub database_url: String,
}
