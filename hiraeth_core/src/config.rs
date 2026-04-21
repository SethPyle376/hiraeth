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

fn default_auth_mode() -> AuthMode {
    AuthMode::Audit
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Enforce,
    Audit,
    Off,
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
    #[serde(default = "default_auth_mode")]
    pub auth_mode: AuthMode,
}

#[cfg(test)]
mod tests {
    use super::{AuthMode, Config};

    #[test]
    fn config_defaults_auth_mode_to_audit() {
        let config: Config =
            serde_json::from_str("{}").expect("config should deserialize with defaults");

        assert_eq!(config.auth_mode, AuthMode::Audit);
    }

    #[test]
    fn config_deserializes_snake_case_auth_mode() {
        let config: Config = serde_json::from_str(
            r#"{
                "auth_mode": "enforce"
            }"#,
        )
        .expect("config should deserialize auth mode");

        assert_eq!(config.auth_mode, AuthMode::Enforce);
    }
}
