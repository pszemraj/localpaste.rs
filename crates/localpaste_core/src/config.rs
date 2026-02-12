//! Configuration loading from environment variables.

use serde::Deserialize;
use std::env;
use std::path::PathBuf;

/// Runtime configuration for LocalPaste.
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

/// Parse a boolean-like environment flag value.
///
/// # Supported Values
/// - Truthy: `1`, `true`, `yes`, `on`
/// - Falsy: `0`, `false`, `no`, `off`, empty string
///
/// Matching is case-insensitive and ignores surrounding whitespace.
///
/// # Returns
/// `Some(bool)` when the value is recognized, otherwise `None`.
pub fn parse_env_flag(value: &str) -> Option<bool> {
    let normalized = value.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "" | "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Read a boolean flag from the environment.
///
/// Missing or unrecognized values are treated as `false`.
///
/// # Arguments
/// - `name`: Environment variable name.
///
/// # Returns
/// `true` when the value is a recognized truthy value.
pub fn env_flag_enabled(name: &str) -> bool {
    env::var(name)
        .ok()
        .and_then(|value| parse_env_flag(&value))
        .unwrap_or(false)
}

impl Config {
    /// Load configuration from environment variables.
    ///
    /// # Returns
    /// A populated [`Config`] with defaults applied when env vars are missing.
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
                .unwrap_or(38411),
            max_paste_size: env::var("MAX_PASTE_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10 * 1024 * 1024), // 10MB default
            auto_save_interval: env::var("AUTO_SAVE_INTERVAL")
                .ok()
                .and_then(|i| i.parse().ok())
                .unwrap_or(2000), // 2 seconds
            auto_backup: env_flag_enabled("AUTO_BACKUP"), // Default to false - backups should be explicit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::parse_env_flag;

    #[test]
    fn parse_env_flag_accepts_truthy_values() {
        for value in ["1", "true", "TRUE", " yes ", "on"] {
            assert_eq!(parse_env_flag(value), Some(true), "value: {}", value);
        }
    }

    #[test]
    fn parse_env_flag_accepts_falsy_values() {
        for value in ["", "0", "false", "FALSE", " no ", "off"] {
            assert_eq!(parse_env_flag(value), Some(false), "value: {}", value);
        }
    }

    #[test]
    fn parse_env_flag_rejects_unknown_values() {
        assert_eq!(parse_env_flag("maybe"), None);
        assert_eq!(parse_env_flag("enabled"), None);
    }
}
