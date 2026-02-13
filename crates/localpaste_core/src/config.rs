//! Configuration loading from environment variables.

use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::warn;

use crate::constants::{
    DEFAULT_AUTO_SAVE_INTERVAL_MS, DEFAULT_MAX_PASTE_SIZE, DEFAULT_PORT,
};

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

/// Parse a boolean-like environment value with a fallback default.
///
/// # Arguments
/// - `name`: environment variable name.
/// - `default`: value when missing or unrecognized.
///
/// # Returns
/// Parsed boolean when recognized, otherwise `default`.
pub fn parse_bool_env(name: &str, default: bool) -> bool {
    let Ok(value) = env::var(name) else {
        return default;
    };
    match parse_env_flag(&value) {
        Some(enabled) => enabled,
        None => {
            warn!(
                "Invalid value for {}='{}'; expected 1/0/true/false/yes/no/on/off. Using default {}.",
                name,
                value,
                default
            );
            default
        }
    }
}

fn parse_env_number<T>(name: &str, default: T) -> T
where
    T: FromStr + Copy + std::fmt::Display,
    <T as FromStr>::Err: std::fmt::Display,
{
    let Ok(value) = env::var(name) else {
        return default;
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        warn!(
            "Environment variable {} is empty; using default {}",
            name, default
        );
        return default;
    }
    match trimmed.parse::<T>() {
        Ok(parsed) => parsed,
        Err(err) => {
            warn!(
                "Invalid value for {}='{}': {}. Using default {}",
                name, value, err, default
            );
            default
        }
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
    parse_bool_env(name, false)
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
            port: parse_env_number("PORT", DEFAULT_PORT),
            max_paste_size: parse_env_number("MAX_PASTE_SIZE", DEFAULT_MAX_PASTE_SIZE),
            auto_save_interval: parse_env_number("AUTO_SAVE_INTERVAL", DEFAULT_AUTO_SAVE_INTERVAL_MS), // 2 seconds
            auto_backup: env_flag_enabled("AUTO_BACKUP"), // Default to false - backups should be explicit
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{env_flag_enabled, parse_env_flag, Config};
    use crate::constants::{DEFAULT_CLI_SERVER_URL, DEFAULT_PORT};
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            // SAFETY: Tests coordinate env mutation via `env_lock` to avoid races.
            unsafe {
                std::env::set_var(key, value);
            }
            Self { key, previous }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(previous) = &self.previous {
                // SAFETY: Tests coordinate env mutation via `env_lock` to avoid races.
                unsafe {
                    std::env::set_var(self.key, previous);
                }
            } else {
                // SAFETY: Tests coordinate env mutation via `env_lock` to avoid races.
                unsafe {
                    std::env::remove_var(self.key);
                }
            }
        }
    }

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

    #[test]
    fn config_from_env_invalid_numeric_values_fall_back_to_defaults() {
        let _lock = env_lock().lock().expect("env lock");
        let _port = EnvGuard::set("PORT", "not-a-number");
        let _max = EnvGuard::set("MAX_PASTE_SIZE", "-1");
        let _interval = EnvGuard::set("AUTO_SAVE_INTERVAL", "wat");

        let config = Config::from_env();
        assert_eq!(config.port, DEFAULT_PORT);
        assert_eq!(config.max_paste_size, DEFAULT_MAX_PASTE_SIZE);
        assert_eq!(config.auto_save_interval, DEFAULT_AUTO_SAVE_INTERVAL_MS);
    }

    #[test]
    fn env_flag_enabled_treats_invalid_value_as_false() {
        let _lock = env_lock().lock().expect("env lock");
        let _flag = EnvGuard::set("LOCALPASTE_TEST_FLAG", "maybe");
        assert!(!env_flag_enabled("LOCALPASTE_TEST_FLAG"));
    }

    #[test]
    fn parse_bool_env_matrix_covers_missing_invalid_and_truthy_falsy() {
        let _lock = env_lock().lock().expect("env lock");

        let cases = [
            ("LOCALPASTE_TEST_FLAG", "", Some(false)),
            ("LOCALPASTE_TEST_FLAG", "1", Some(true)),
            ("LOCALPASTE_TEST_FLAG", "true", Some(true)),
            ("LOCALPASTE_TEST_FLAG", "TRUE", Some(true)),
            ("LOCALPASTE_TEST_FLAG", " yes ", Some(true)),
            ("LOCALPASTE_TEST_FLAG", "on", Some(true)),
            ("LOCALPASTE_TEST_FLAG", "0", Some(false)),
            ("LOCALPASTE_TEST_FLAG", "false", Some(false)),
            ("LOCALPASTE_TEST_FLAG", "FALSE", Some(false)),
            ("LOCALPASTE_TEST_FLAG", " no ", Some(false)),
            ("LOCALPASTE_TEST_FLAG", "off", Some(false)),
            ("LOCALPASTE_TEST_FLAG", "maybe", None),
        ];

        assert_eq!(parse_bool_env("LOCALPASTE_TEST_FLAG", false), false);
        assert_eq!(parse_bool_env("LOCALPASTE_TEST_FLAG", true), true);

        for (key, value, expected) in cases {
            let _guard = EnvGuard::set(key, value);
            match expected {
                Some(expected) => assert_eq!(parse_bool_env(key, false), expected),
                None => {
                    assert_eq!(parse_bool_env(key, false), false);
                    assert_eq!(parse_bool_env(key, true), true);
                }
            }
        }
    }

    #[test]
    fn config_auto_backup_obeys_bool_matrix_values() {
        let _lock = env_lock().lock().expect("env lock");
        let backup_key = "AUTO_BACKUP";
        let values = [("1", true), ("0", false), ("true", true), ("false", false), ("", false)];

        for (value, expected) in values {
            let _flag = EnvGuard::set(backup_key, value);
            let config = Config::from_env();
            assert_eq!(config.auto_backup, expected, "value: {value}");
        }
    }

    #[test]
    fn cli_server_url_matches_default_port_constant() {
        assert_eq!(
            DEFAULT_CLI_SERVER_URL,
            format!("http://localhost:{}", DEFAULT_PORT)
        );
    }
}
