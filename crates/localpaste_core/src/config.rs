//! Configuration loading from environment variables.

use serde::Deserialize;
use std::env;
use std::path::PathBuf;
use std::str::FromStr;
use tracing::warn;

use crate::constants::{
    API_ADDR_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS, DEFAULT_MAX_PASTE_SIZE,
    DEFAULT_PASTE_VERSION_INTERVAL_SECS, DEFAULT_PORT,
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

fn normalize_db_path_value(raw: String, strict_empty: bool) -> Result<String, String> {
    let expanded = expand_tilde(raw);
    if expanded.trim().is_empty() {
        if strict_empty {
            Err("Environment variable DB_PATH is empty".to_string())
        } else {
            let default = default_db_path();
            warn!(
                "Environment variable DB_PATH is empty; using default {}",
                default
            );
            Ok(default)
        }
    } else {
        Ok(expanded)
    }
}

/// Resolve the configured DB path, falling back to the default path.
///
/// This uses the same env/default rules as [`Config::from_env`].
///
/// # Returns
/// The expanded `DB_PATH` value or the platform default path.
pub fn db_path_from_env_or_default() -> String {
    match env::var("DB_PATH") {
        Ok(value) => normalize_db_path_value(value, false).unwrap_or_else(|_| default_db_path()),
        Err(_) => default_db_path(),
    }
}

/// Resolve the configured DB path in strict mode.
///
/// Missing `DB_PATH` still falls back to the platform default path, but an
/// explicitly empty value is rejected instead of silently defaulting.
///
/// # Returns
/// The expanded `DB_PATH` value or the platform default path.
///
/// # Errors
/// Returns an error when `DB_PATH` is present but empty after trimming.
pub fn db_path_from_env_strict() -> Result<String, String> {
    match env::var("DB_PATH") {
        Ok(value) => normalize_db_path_value(value, true),
        Err(_) => Ok(default_db_path()),
    }
}

/// Resolve a database path from an explicit CLI flag, `DB_PATH`, or the
/// platform default path when allowed.
///
/// # Arguments
/// - `explicit`: optional CLI-provided database path
/// - `allow_default`: whether the platform default path may be used when
///   neither a CLI flag nor `DB_PATH` is provided
///
/// # Returns
/// The normalized database path.
///
/// # Errors
/// Returns an error when the explicit path or `DB_PATH` is blank, or when no
/// explicit path is provided and default use is disallowed.
pub fn resolve_db_path_with_explicit_or_env(
    explicit: Option<String>,
    allow_default: bool,
) -> Result<String, String> {
    if let Some(raw) = explicit {
        let expanded = expand_tilde(raw);
        if expanded.trim().is_empty() {
            return Err("--db-path cannot be empty".to_string());
        }
        return Ok(expanded);
    }

    match env::var("DB_PATH") {
        Ok(value) => normalize_db_path_value(value, true),
        Err(_) if allow_default => Ok(default_db_path()),
        Err(_) => Err(
            "database path is required; pass --db-path, set DB_PATH, or rerun with --allow-default-db"
                .to_string(),
        ),
    }
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

fn parse_nonzero_interval_seconds_strict(name: &str, default: u64) -> Result<u64, String> {
    let value = parse_env_number_strict(name, default)?;
    if value == 0 {
        return Err(format!(
            "Invalid value for {}='0': expected integer >= 1",
            name
        ));
    }
    Ok(value)
}

/// Resolve the minimum interval between persisted paste-version snapshots.
///
/// # Returns
/// Interval in seconds (minimum `1`), sourced from
/// `LOCALPASTE_VERSION_INTERVAL_SECS` when set, with
/// `LOCALPASTE_PASTE_VERSION_INTERVAL_SECS` as a legacy fallback.
///
/// # Errors
/// Returns an error when an explicitly provided interval is malformed or less than `1`.
pub fn paste_version_interval_secs_from_env() -> Result<u64, String> {
    const PRIMARY_KEY: &str = "LOCALPASTE_VERSION_INTERVAL_SECS";
    const LEGACY_KEY: &str = "LOCALPASTE_PASTE_VERSION_INTERVAL_SECS";
    if env::var(PRIMARY_KEY).is_ok() {
        return parse_nonzero_interval_seconds_strict(
            PRIMARY_KEY,
            DEFAULT_PASTE_VERSION_INTERVAL_SECS,
        );
    }
    if env::var(LEGACY_KEY).is_ok() {
        return parse_nonzero_interval_seconds_strict(
            LEGACY_KEY,
            DEFAULT_PASTE_VERSION_INTERVAL_SECS,
        );
    }
    Ok(DEFAULT_PASTE_VERSION_INTERVAL_SECS)
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
        let db_path = db_path_from_env_strict()?;

        // Validate snapshot interval envs during strict startup so malformed values
        // fail fast instead of surfacing later during write operations.
        let _ = paste_version_interval_secs_from_env()?;

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
        api_addr_file_path_for_db_path, db_path_from_env_or_default, db_path_from_env_strict,
        env_flag_enabled, parse_bool_env, parse_bool_env_strict, parse_env_flag,
        paste_version_interval_secs_from_env, resolve_db_path_with_explicit_or_env, Config,
    };
    use crate::constants::{
        API_ADDR_FILE_NAME, DEFAULT_AUTO_SAVE_INTERVAL_MS, DEFAULT_MAX_PASTE_SIZE,
        DEFAULT_PASTE_VERSION_INTERVAL_SECS, DEFAULT_PORT,
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
    fn blank_db_path_defaults_in_permissive_mode_and_fails_in_strict_mode() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::set("DB_PATH", "   ");

        let permissive = db_path_from_env_or_default();
        assert!(
            !permissive.trim().is_empty(),
            "permissive path should not preserve an empty DB_PATH"
        );

        let err = db_path_from_env_strict().expect_err("strict db path should reject blank");
        assert!(err.contains("DB_PATH"));
    }

    #[test]
    fn resolve_db_path_requires_explicit_or_env_when_default_is_disallowed() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::remove("DB_PATH");

        let err = resolve_db_path_with_explicit_or_env(None, false)
            .expect_err("default db path should require explicit opt-in");
        assert!(err.contains("--db-path"));
        assert!(err.contains("--allow-default-db"));
    }

    #[test]
    fn resolve_db_path_prefers_explicit_then_env_then_default() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::set("DB_PATH", "/tmp/from-env");

        let explicit = resolve_db_path_with_explicit_or_env(Some("~/from-flag".to_string()), true)
            .expect("explicit db path");
        assert!(explicit.ends_with("from-flag"));

        let from_env = resolve_db_path_with_explicit_or_env(None, false).expect("env db path");
        assert_eq!(from_env, "/tmp/from-env");

        drop(_db_path);
        let _db_path = EnvGuard::remove("DB_PATH");
        let default_path =
            resolve_db_path_with_explicit_or_env(None, true).expect("default db path");
        assert!(!default_path.trim().is_empty());
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
    fn paste_version_interval_secs_strict_matrix() {
        let _lock = env_lock().lock().expect("env lock");
        let _primary = EnvGuard::remove("LOCALPASTE_VERSION_INTERVAL_SECS");
        let _legacy = EnvGuard::remove("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS");
        assert_eq!(
            paste_version_interval_secs_from_env().expect("default interval"),
            DEFAULT_PASTE_VERSION_INTERVAL_SECS
        );

        let _legacy = EnvGuard::set("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS", "7");
        assert_eq!(
            paste_version_interval_secs_from_env().expect("legacy interval"),
            7
        );
        drop(_legacy);

        let _primary = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "9");
        let _legacy = EnvGuard::set("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS", "7");
        assert_eq!(
            paste_version_interval_secs_from_env().expect("primary interval"),
            9
        );
        drop(_primary);
        drop(_legacy);

        let _primary = EnvGuard::set("LOCALPASTE_VERSION_INTERVAL_SECS", "0");
        let err = paste_version_interval_secs_from_env().expect_err("zero should fail");
        assert!(err.contains("LOCALPASTE_VERSION_INTERVAL_SECS"));
        drop(_primary);

        let _legacy = EnvGuard::set("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS", "not-a-number");
        let err = paste_version_interval_secs_from_env().expect_err("invalid legacy should fail");
        assert!(err.contains("LOCALPASTE_PASTE_VERSION_INTERVAL_SECS"));
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

    #[test]
    fn config_default_db_path_uses_platform_cache_location() {
        let _lock = env_lock().lock().expect("env lock");
        let _db_path = EnvGuard::remove("DB_PATH");

        #[cfg(target_os = "windows")]
        let _local_app_data = EnvGuard::set("LOCALAPPDATA", r"C:\Users\tester\AppData\Local");
        #[cfg(not(target_os = "windows"))]
        let _local_app_data = EnvGuard::remove("LOCALAPPDATA");

        #[cfg(target_os = "windows")]
        let _home = EnvGuard::remove("HOME");
        #[cfg(not(target_os = "windows"))]
        let _home = EnvGuard::set("HOME", "/tmp/localpaste-home");

        let _userprofile = EnvGuard::remove("USERPROFILE");
        let _homedrive = EnvGuard::remove("HOMEDRIVE");
        let _homepath = EnvGuard::remove("HOMEPATH");

        #[cfg(target_os = "windows")]
        let expected = PathBuf::from(r"C:\Users\tester\AppData\Local")
            .join("localpaste")
            .join("db");
        #[cfg(not(target_os = "windows"))]
        let expected = PathBuf::from("/tmp/localpaste-home")
            .join(".cache")
            .join("localpaste")
            .join("db");

        let config = Config::from_env();
        assert_eq!(PathBuf::from(config.db_path), expected);
    }
}
