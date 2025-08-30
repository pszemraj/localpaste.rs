use serde::Deserialize;
use std::env;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub db_path: String,
    pub port: u16,
    pub max_paste_size: usize,
    #[allow(dead_code)]
    pub auto_save_interval: u64,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            db_path: env::var("DB_PATH").unwrap_or_else(|_| "./data/localpaste.db".to_string()),
            port: env::var("PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3030),
            max_paste_size: env::var("MAX_PASTE_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10 * 1024 * 1024), // 10MB default
            auto_save_interval: env::var("AUTO_SAVE_INTERVAL")
                .ok()
                .and_then(|i| i.parse().ok())
                .unwrap_or(2000), // 2 seconds
        }
    }
}
