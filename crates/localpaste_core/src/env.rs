//! Process-global environment mutation helpers.

use std::sync::{Mutex, OnceLock};

fn global_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Return the global lock used to serialize environment mutations in tests.
///
/// # Returns
/// A process-wide mutex for synchronizing environment mutation.
pub fn env_lock() -> &'static Mutex<()> {
    global_env_lock()
}

/// Set an environment variable through a compatibility wrapper.
///
/// Rust toolchains differ on whether env mutation APIs are `unsafe`.
///
/// # Arguments
/// - `key`: Environment variable name.
/// - `value`: Value to assign.
#[allow(unused_unsafe)]
pub fn set_env_var(key: &str, value: &str) {
    // SAFETY: Callers must serialize mutation when test threads may run in parallel.
    unsafe {
        std::env::set_var(key, value);
    }
}

/// Remove an environment variable through a compatibility wrapper.
///
/// Rust toolchains differ on whether env mutation APIs are `unsafe`.
#[allow(unused_unsafe)]
pub fn remove_env_var(key: &str) {
    // SAFETY: Callers must serialize mutation when test threads may run in parallel.
    unsafe {
        std::env::remove_var(key);
    }
}

/// Restores an environment variable value on drop.
pub struct EnvGuard {
    key: String,
    previous: Option<String>,
}

impl EnvGuard {
    /// Set `key=value` and restore the previous value when dropped.
    ///
    /// # Arguments
    /// - `key`: Environment variable name.
    /// - `value`: Value to assign for the lifetime of this guard.
    ///
    /// # Returns
    /// Guard that restores the previous value on drop.
    pub fn set(key: &str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        set_env_var(key, value);
        Self {
            key: key.to_string(),
            previous,
        }
    }

    /// Remove `key` and restore the previous value when dropped.
    ///
    /// # Returns
    /// Guard that restores the previous value on drop.
    pub fn remove(key: &str) -> Self {
        let previous = std::env::var(key).ok();
        remove_env_var(key);
        Self {
            key: key.to_string(),
            previous,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.as_deref() {
            set_env_var(self.key.as_str(), previous);
        } else {
            remove_env_var(self.key.as_str());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{env_lock, EnvGuard};

    #[test]
    fn env_guard_restores_previous_value() {
        let _lock = env_lock().lock().expect("env lock");
        let key = "LOCALPASTE_TEST_ENV_GUARD_RESTORE";
        let _baseline = EnvGuard::set(key, "before");
        {
            let _override = EnvGuard::set(key, "after");
            assert_eq!(std::env::var(key).ok().as_deref(), Some("after"));
        }
        assert_eq!(std::env::var(key).ok().as_deref(), Some("before"));
    }

    #[test]
    fn env_guard_remove_restores_missing_value() {
        let _lock = env_lock().lock().expect("env lock");
        let key = "LOCALPASTE_TEST_ENV_GUARD_REMOVE";
        {
            let _removed = EnvGuard::remove(key);
            assert!(std::env::var(key).is_err());
        }
        assert!(std::env::var(key).is_err());
    }
}
