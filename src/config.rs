use serde::Deserialize;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub db_path: String,
    pub port: u16,
    pub max_paste_size: usize,
    #[allow(dead_code)]
    pub auto_save_interval: u64,
    pub auto_backup: bool,
}

/// Expand tilde (~) in paths to the user's home directory
fn expand_tilde(path: String) -> String {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = resolve_home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    path
}

fn resolve_home_dir() -> Option<PathBuf> {
    // Prefer explicit HOME if set (Unix, some Windows shells)
    if let Ok(home) = env::var("HOME") {
        if !home.trim().is_empty() {
            return Some(PathBuf::from(home));
        }
    }

    // Windows USERPROFILE (standard)
    if let Ok(profile) = env::var("USERPROFILE") {
        if !profile.trim().is_empty() {
            return Some(PathBuf::from(profile));
        }
    }

    // Windows legacy HOMEDRIVE + HOMEPATH
    if let (Ok(drive), Ok(path)) = (env::var("HOMEDRIVE"), env::var("HOMEPATH")) {
        if !drive.trim().is_empty() && !path.trim().is_empty() {
            return Some(PathBuf::from(format!("{}{}", drive, path)));
        }
    }

    // Fallback to current directory if available
    std::env::current_dir().ok()
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            db_path: env::var("DB_PATH").map(expand_tilde).unwrap_or_else(|_| {
                let home = resolve_home_dir().unwrap_or_else(|| PathBuf::from("."));
                let cache_dir = home.join(".cache").join("localpaste");
                cache_dir.join("db").to_string_lossy().to_string()
            }),
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
            auto_backup: env::var("AUTO_BACKUP")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(false), // Default to false - backups should be explicit
        }
    }
}
