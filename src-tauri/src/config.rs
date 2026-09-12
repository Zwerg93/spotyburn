use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_CLIENT_ID: &str = "";
pub const DEFAULT_CLIENT_SECRET: &str = "";

pub use crate::burner::{calculate_capacity_usage, BurnMode, CapacityUsage};

fn try_load_dotenv() {
    for candidate in &[".env", "../.env", "../../.env"] {
        if let Ok(content) = fs::read_to_string(candidate) {
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let k = k.trim();
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    if std::env::var(k).is_err() {
                        std::env::set_var(k, v);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: String,
    pub client_secret: String,
    pub cache_dir: PathBuf,
    pub default_burn_mode: BurnMode,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub user_display_name: Option<String>,
    #[serde(default)]
    pub user_access_token: Option<String>,
}

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Could not determine user home directory")]
    HomeDirNotFound,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl AppConfig {
    pub fn default_app_dir() -> Result<PathBuf, ConfigError> {
        let home = dirs::home_dir().ok_or(ConfigError::HomeDirNotFound)?;
        Ok(home.join(".spotyburn"))
    }

    pub fn default_config_path() -> Result<PathBuf, ConfigError> {
        Ok(Self::default_app_dir()?.join("config.json"))
    }

    pub fn default_cache_dir() -> Result<PathBuf, ConfigError> {
        Ok(Self::default_app_dir()?.join("cache"))
    }

    pub fn load() -> Result<Self, ConfigError> {
        let config_path = Self::default_config_path()?;
        if config_path.exists() {
            Self::load_from(&config_path)
        } else {
            Ok(Self::default())
        }
    }

    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(path)?;
        let config = serde_json::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let config_path = Self::default_config_path()?;
        self.save_to(&config_path)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json_data = serde_json::to_string_pretty(self)?;
        fs::write(path, json_data)?;
        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        try_load_dotenv();
        let client_id =
            std::env::var("SPOTIFY_CLIENT_ID").unwrap_or_else(|_| DEFAULT_CLIENT_ID.to_string());
        let client_secret = std::env::var("SPOTIFY_CLIENT_SECRET")
            .unwrap_or_else(|_| DEFAULT_CLIENT_SECRET.to_string());
        let cache_dir =
            Self::default_cache_dir().unwrap_or_else(|_| PathBuf::from(".spotyburn/cache"));
        Self {
            client_id,
            client_secret,
            cache_dir,
            default_burn_mode: BurnMode::default(),
            refresh_token: None,
            user_display_name: None,
            user_access_token: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.default_burn_mode, BurnMode::AudioCdRedBook);
        assert_eq!(config.refresh_token, None);
        assert_eq!(config.user_display_name, None);
        assert_eq!(config.user_access_token, None);
        assert!(config.cache_dir.ends_with(".spotyburn/cache"));
    }

    #[test]
    fn test_config_save_and_load() {
        let temp_dir = env::temp_dir().join("spotyburn_test_config");
        let config_path = temp_dir.join("test_config.json");

        let config = AppConfig {
            client_id: "custom_id".to_string(),
            client_secret: "custom_secret".to_string(),
            default_burn_mode: BurnMode::DataMp3Cd,
            refresh_token: Some("rt_xyz".to_string()),
            user_display_name: Some("SpotifyUser".to_string()),
            ..AppConfig::default()
        };

        config.save_to(&config_path).expect("failed to save config");
        assert!(config_path.exists());

        let loaded = AppConfig::load_from(&config_path).expect("failed to load config");
        assert_eq!(config, loaded);

        // cleanup
        let _ = fs::remove_file(&config_path);
        let _ = fs::remove_dir(&temp_dir);
    }

    #[test]
    fn test_config_backward_compatibility() {
        let old_json = r#"{
            "client_id": "test_id",
            "client_secret": "test_secret",
            "cache_dir": "/tmp/cache",
            "default_burn_mode": "AudioCdRedBook"
        }"#;

        let parsed: AppConfig = serde_json::from_str(old_json).expect("parse old config");
        assert_eq!(parsed.client_id, "test_id");
        assert_eq!(parsed.refresh_token, None);
        assert_eq!(parsed.user_display_name, None);
    }

    #[test]
    fn test_config_serialization() {
        let config = AppConfig::default();
        let serialized = serde_json::to_string(&config).expect("serialization failed");
        let deserialized: AppConfig =
            serde_json::from_str(&serialized).expect("deserialization failed");
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_export_only_config_serialization() {
        let mut config = AppConfig::default();
        config.default_burn_mode = BurnMode::ExportOnly;
        let serialized = serde_json::to_string(&config).expect("serialization failed");
        assert!(serialized.contains("\"ExportOnly\""));
        let deserialized: AppConfig =
            serde_json::from_str(&serialized).expect("deserialization failed");
        assert_eq!(deserialized.default_burn_mode, BurnMode::ExportOnly);
    }
}
