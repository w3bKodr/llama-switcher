//! Settings: load/save a single JSON file in the app data directory.

use rand::Rng;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DefaultProfileMode {
    None,
    LastUsed,
    Specific,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub scripts_folder: String,
    pub scan_pattern: String,
    pub allowed_extensions: Vec<String>,
    pub server_port: u16,
    pub health_url: String,
    pub agent_api_port: u16,
    pub agent_api_token: String,
    pub auto_rescan_on_startup: bool,
    pub auto_rescan_interval_seconds: Option<u64>,
    pub default_profile_mode: DefaultProfileMode,
    pub default_profile_id: Option<String>,
    pub last_used_profile_id: Option<String>,
    pub stop_timeout_seconds: u64,
    pub health_check_timeout_seconds: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            scripts_folder: "D:\\llama".to_string(),
            scan_pattern: "start - {model} - {feature}".to_string(),
            allowed_extensions: vec![".cmd".into(), ".bat".into(), ".ps1".into()],
            server_port: 1234,
            health_url: "http://127.0.0.1:1234/health".to_string(),
            agent_api_port: 47891,
            agent_api_token: generate_token(),
            auto_rescan_on_startup: true,
            auto_rescan_interval_seconds: None,
            default_profile_mode: DefaultProfileMode::None,
            default_profile_id: None,
            last_used_profile_id: None,
            stop_timeout_seconds: 15,
            health_check_timeout_seconds: 60,
        }
    }
}

/// Generate a random URL-safe token used to authenticate the agent control API.
pub fn generate_token() -> String {
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    let mut rng = rand::thread_rng();
    (0..40)
        .map(|_| CHARSET[rng.gen_range(0..CHARSET.len())] as char)
        .collect()
}

impl Settings {
    /// Load settings from disk, falling back to defaults (and persisting them) on
    /// first launch or if the file is missing/corrupt.
    pub fn load_or_init(path: &Path) -> Settings {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(settings) = serde_json::from_str::<Settings>(&text) {
                return settings;
            }
        }
        let settings = Settings::default();
        let _ = settings.save(path);
        settings
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}
