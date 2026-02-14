//! Configuration loading from environment variables.

use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::warn;

use crate::constants::{
    API_ADDR_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS, DEFAULT_MAX_PASTE_SIZE, DEFAULT_PORT,
};

/// Runtime configuration for LocalPaste.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub db_path: String,
    pub port: u16,
    pub max_paste_size: usize,
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

fn default_data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            if !local_app_data.trim().is_empty() {
                return PathBuf::from(local_app_data).join("localpaste");
            }
        }
    }

    let home = resolve_home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache").join("localpaste")
}

fn default_db_path() -> String {
    default_data_dir().join("db").to_string_lossy().to_string()
}

/// Resolve the configured DB path, falling back to the default path.
///
/// This uses the same env/default rules as [`Config::from_env`].
///
/// # Returns
/// The expanded `DB_PATH` value or the platform default path.
pub fn db_path_from_env_or_default() -> String {
    env::var("DB_PATH")
        .map(expand_tilde)
        .unwrap_or_else(|_| default_db_path())
}

/// Resolve the discovery-file path for a given database path.
///
/// The discovery file is stored inside the configured DB directory so
/// different DB paths cannot overwrite each other's discovery state.
///
/// # Arguments
/// - `db_path`: Database path used by LocalPaste.
///
/// # Returns
/// Path to the `.api-addr` discovery file.
pub fn api_addr_file_path_for_db_path(db_path: &str) -> PathBuf {
    let db_path = PathBuf::from(expand_tilde(db_path.to_string()));
    if db_path.as_os_str().is_empty() {
        return PathBuf::from(".").join(API_ADDR_FILE_NAME);
    }
    db_path.join(API_ADDR_FILE_NAME)
}

/// Resolve the discovery-file path using env/default DB path rules.
///
/// # Returns
/// Path to the `.api-addr` discovery file.
pub fn api_addr_file_path_from_env_or_default() -> PathBuf {
    api_addr_file_path_for_db_path(db_path_from_env_or_default().as_str())
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

/// Parse a boolean-like environment value strictly.
///
/// # Arguments
/// - `name`: environment variable name.
/// - `default`: value when the variable is missing.
///
/// # Returns
/// Parsed boolean when recognized, otherwise an error describing the malformed input.
///
/// # Errors
/// Returns an error when the variable is present but not a recognized boolean token.
pub fn parse_bool_env_strict(name: &str, default: bool) -> Result<bool, String> {
    let Ok(value) = env::var(name) else {
        return Ok(default);
    };
    parse_env_flag(&value).ok_or_else(|| {
        format!(
            "Invalid value for {}='{}'; expected 1/0/true/false/yes/no/on/off",
            name, value
        )
    })
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

fn parse_env_number_strict<T>(name: &str, default: T) -> Result<T, String>
where
    T: FromStr + Copy + std::fmt::Display,
    <T as FromStr>::Err: std::fmt::Display,
{
    let Ok(value) = env::var(name) else {
        return Ok(default);
    };
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(format!("Environment variable {} is empty", name));
    }
    trimmed
        .parse::<T>()
        .map_err(|err| format!("Invalid value for {}='{}': {}", name, value, err))
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
    /// This permissive variant is intended for tooling/local workflows where
    /// malformed env values should fall back to defaults. The headless server
    /// entrypoint uses [`Config::from_env_strict`] for fail-fast startup.
    ///
    /// # Returns
    /// A populated [`Config`] with defaults applied when env vars are missing.
    pub fn from_env() -> Self {
        Self {
            db_path: db_path_from_env_or_default(),
            port: parse_env_number("PORT", DEFAULT_PORT),
            max_paste_size: parse_env_number("MAX_PASTE_SIZE", DEFAULT_MAX_PASTE_SIZE),
            auto_save_interval: parse_env_number(
                "AUTO_SAVE_INTERVAL",
                DEFAULT_AUTO_SAVE_INTERVAL_MS,
            ), // 2 seconds
            auto_backup: env_flag_enabled("AUTO_BACKUP"), // Default to false - backups should be explicit
        }
    }

    /// Load configuration from environment variables in strict mode.
    ///
    /// Missing variables still use defaults, but malformed values are rejected.
    ///
    /// # Returns
    /// A populated [`Config`] when all provided env vars are valid.
    ///
    /// # Errors
    /// Returns a descriptive message when any configured value is invalid.
    pub fn from_env_strict() -> Result<Self, String> {
        let db_path = match env::var("DB_PATH") {
            Ok(value) => {
                let expanded = expand_tilde(value);
                if expanded.trim().is_empty() {
                    return Err("Environment variable DB_PATH is empty".to_string());
                }
                expanded
            }
            Err(_) => default_db_path(),
        };

        Ok(Self {
            db_path,
            port: parse_env_number_strict("PORT", DEFAULT_PORT)?,
            max_paste_size: parse_env_number_strict("MAX_PASTE_SIZE", DEFAULT_MAX_PASTE_SIZE)?,
            auto_save_interval: parse_env_number_strict(
                "AUTO_SAVE_INTERVAL",
                DEFAULT_AUTO_SAVE_INTERVAL_MS,
            )?,
            auto_backup: parse_bool_env_strict("AUTO_BACKUP", false)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        api_addr_file_path_for_db_path, env_flag_enabled, parse_bool_env, parse_bool_env_strict,
        parse_env_flag, Config,
    };
    use crate::constants::{
        API_ADDR_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS, DEFAULT_MAX_PASTE_SIZE, DEFAULT_PORT,
    };
    use crate::env::{env_lock, EnvGuard};
    use std::path::PathBuf;

    #[test]
    fn parse_env_flag_matrix_covers_truthy_falsy_and_unknown_values() {
        let cases = [
            ("1", Some(true)),
            ("true", Some(true)),
            ("TRUE", Some(true)),
            (" yes ", Some(true)),
            ("on", Some(true)),
            ("", Some(false)),
            ("0", Some(false)),
            ("false", Some(false)),
            ("FALSE", Some(false)),
            (" no ", Some(false)),
            ("off", Some(false)),
            ("maybe", None),
            ("enabled", None),
        ];

        for (value, expected) in cases {
            assert_eq!(parse_env_flag(value), expected, "value: {}", value);
        }
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
    fn config_from_env_strict_rejects_invalid_numeric_and_bool_values() {
        let _lock = env_lock().lock().expect("env lock");
        let _port = EnvGuard::set("PORT", "not-a-number");
        let err = Config::from_env_strict().expect_err("strict parse should fail");
        assert!(err.contains("PORT"));

        let _port = EnvGuard::remove("PORT");
        let _backup = EnvGuard::set("AUTO_BACKUP", "maybe");
        let err = Config::from_env_strict().expect_err("strict bool parse should fail");
        assert!(err.contains("AUTO_BACKUP"));
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

        assert!(!parse_bool_env("LOCALPASTE_TEST_FLAG", false));
        assert!(parse_bool_env("LOCALPASTE_TEST_FLAG", true));

        for (key, value, expected) in cases {
            let _guard = EnvGuard::set(key, value);
            match expected {
                Some(expected) => assert_eq!(parse_bool_env(key, false), expected),
                None => {
                    assert!(!parse_bool_env(key, false));
                    assert!(parse_bool_env(key, true));
                }
            }
        }
    }

    #[test]
    fn parse_bool_env_strict_rejects_invalid_values() {
        let _lock = env_lock().lock().expect("env lock");
        let _flag = EnvGuard::set("LOCALPASTE_TEST_FLAG", "maybe");
        let err = parse_bool_env_strict("LOCALPASTE_TEST_FLAG", false)
            .expect_err("strict bool parser should reject invalid values");
        assert!(err.contains("LOCALPASTE_TEST_FLAG"));
    }

    #[test]
    fn config_auto_backup_obeys_bool_matrix_values() {
        let _lock = env_lock().lock().expect("env lock");
        let backup_key = "AUTO_BACKUP";
        let values = [
            ("1", true),
            ("0", false),
            ("true", true),
            ("false", false),
            ("", false),
        ];

        for (value, expected) in values {
            let _flag = EnvGuard::set(backup_key, value);
            let config = Config::from_env();
            assert_eq!(config.auto_backup, expected, "value: {value}");
        }
    }

    #[test]
    fn api_addr_discovery_path_is_unique_per_db_path() {
        let parent = std::env::temp_dir().join("localpaste-config-discovery");
        let db_a = parent.join("db-a");
        let db_b = parent.join("db-b");
        let db_a_string = db_a.to_string_lossy().to_string();
        let db_b_string = db_b.to_string_lossy().to_string();

        let path_a = api_addr_file_path_for_db_path(db_a_string.as_str());
        let path_b = api_addr_file_path_for_db_path(db_b_string.as_str());

        assert_eq!(path_a, db_a.join(API_ADDR_FILE_NAME));
        assert_eq!(path_b, db_b.join(API_ADDR_FILE_NAME));
        assert_ne!(path_a, path_b);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn config_default_db_path_uses_localappdata_on_windows() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::remove("DB_PATH");
        let _local_app_data = EnvGuard::set("LOCALAPPDATA", r"C:\Users\tester\AppData\Local");
        let _home = EnvGuard::remove("HOME");
        let _userprofile = EnvGuard::remove("USERPROFILE");
        let _homedrive = EnvGuard::remove("HOMEDRIVE");
        let _homepath = EnvGuard::remove("HOMEPATH");

        let config = Config::from_env();
        assert_eq!(
            PathBuf::from(config.db_path),
            PathBuf::from(r"C:\Users\tester\AppData\Local")
                .join("localpaste")
                .join("db")
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn config_default_db_path_uses_home_cache_on_non_windows() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::remove("DB_PATH");
        let _home = EnvGuard::set("HOME", "/tmp/localpaste-home");
        let _userprofile = EnvGuard::remove("USERPROFILE");
        let _homedrive = EnvGuard::remove("HOMEDRIVE");
        let _homepath = EnvGuard::remove("HOMEPATH");

        let config = Config::from_env();
        assert_eq!(
            PathBuf::from(config.db_path),
            PathBuf::from("/tmp/localpaste-home")
                .join(".cache")
                .join("localpaste")
                .join("db")
        );
    }
}
