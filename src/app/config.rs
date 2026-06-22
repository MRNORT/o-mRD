use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub client_id: String,
    pub client_secret: String,
    pub download_dir: PathBuf,
    pub auto_check: bool,
    pub check_interval_minutes: u64,
    pub minimize_to_tray: bool,
    pub start_minimized: bool,
    pub notification_on_new: bool,
    pub auto_download_new: bool,
    pub last_known_ranked_id: Option<u64>,
    pub known_beatmapset_ids: Vec<u64>,
    
    // Auto-download & Import settings
    pub osu_dir: Option<PathBuf>,
    pub auto_import: bool,
    pub download_4k_only: bool,
    #[serde(default)]
    pub prefer_no_video: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        let download_dir = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("downloads");

        Self {
            client_id: String::new(),
            client_secret: String::new(),
            download_dir,
            auto_check: false,
            check_interval_minutes: 30,
            minimize_to_tray: true,
            start_minimized: false,
            notification_on_new: true,
            auto_download_new: false,
            last_known_ranked_id: None,
            known_beatmapset_ids: Vec::new(),
            
            osu_dir: dirs::data_local_dir().map(|d| d.join("osu!")),
            auto_import: true,
            download_4k_only: true,
            prefer_no_video: false,
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let path = config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
                Err(_) => Self::default(),
            }
        } else {
            Self::default()
        }
    }

    pub fn save(&self) {
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(s) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, s);
        }
    }
}

fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("osu-mania-dl")
        .join("config.json")
}
