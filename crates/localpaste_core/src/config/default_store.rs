//! Identifies the platform default paste store.

use super::{default_db_path, expand_tilde};
use std::path::PathBuf;

/// Reports whether `db_path` names the platform default store.
///
/// Both paths are compared after `~` expansion, through symlinks when they
/// exist and as absolute paths otherwise, so spellings such as
/// `~/.cache/localpaste/db/`, a relative path, or a symlink to the default
/// directory still count as the default store.
///
/// # Arguments
/// - `db_path`: Database directory path, for example a resolved `DB_PATH`.
///
/// # Returns
/// `true` when `db_path` resolves to the same location as the default path.
pub fn is_default_db_path(db_path: &str) -> bool {
    resolve_location(db_path) == resolve_location(&default_db_path())
}

/// Resolves `db_path` to the location that store comparisons use.
///
/// # Returns
/// The canonical path when `db_path` exists, otherwise its absolute form.
fn resolve_location(db_path: &str) -> PathBuf {
    let path = PathBuf::from(expand_tilde(db_path.to_string()));
    std::fs::canonicalize(&path)
        .or_else(|_| std::path::absolute(&path))
        .unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::{default_db_path, is_default_db_path};
    use crate::config::db_path_from_env_or_default;
    use crate::env::{env_lock, EnvGuard};
    use std::path::{Path, PathBuf};

    /// Runs `run` with the default store pointed into a fresh temp home.
    fn with_temp_default_store(run: impl FnOnce(&Path, PathBuf)) {
        let _lock = env_lock().lock().expect("env lock");
        let home = tempfile::tempdir().expect("temp home");
        let home_str = home.path().to_str().expect("home path");
        let _home = EnvGuard::set("HOME", home_str);
        let _local_app_data = EnvGuard::set("LOCALAPPDATA", home_str);
        let _db_path = EnvGuard::remove("DB_PATH");
        let default = PathBuf::from(default_db_path());
        assert!(default.starts_with(home.path()), "{}", default.display());
        run(home.path(), default);
    }

    #[test]
    fn default_store_matches_unset_db_path_and_trailing_separator() {
        with_temp_default_store(|home, default| {
            let default_str = default.to_str().expect("default path");
            assert!(is_default_db_path(&db_path_from_env_or_default()));
            assert!(is_default_db_path(default_str));
            let trailing = format!("{default_str}{}", std::path::MAIN_SEPARATOR);
            assert!(is_default_db_path(&trailing));

            let other = home.join("other-db");
            assert!(!is_default_db_path(other.to_str().expect("other path")));
            assert!(!is_default_db_path("db"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn default_store_matches_tilde_relative_and_symlinked_spellings() {
        with_temp_default_store(|home, default| {
            assert!(is_default_db_path("~/.cache/localpaste/db"));
            assert!(is_default_db_path("~/.cache/localpaste/db/"));

            std::fs::create_dir_all(&default).expect("create default store dir");
            let alias = home.join("alias-db");
            std::os::unix::fs::symlink(&default, &alias).expect("symlink");
            assert!(is_default_db_path(alias.to_str().expect("alias path")));

            // Walk up from the test's working directory to `/`, then down to the store.
            let cwd = std::env::current_dir().expect("cwd");
            let mut relative = PathBuf::new();
            for _ in cwd.components().skip(1) {
                relative.push("..");
            }
            relative.push(default.strip_prefix("/").expect("absolute default"));
            assert!(relative.is_relative());
            assert!(is_default_db_path(
                relative.to_str().expect("relative path")
            ));

            let sibling = home.join(".cache").join("localpaste").join("db-copy");
            std::fs::create_dir_all(&sibling).expect("create sibling dir");
            assert!(!is_default_db_path(sibling.to_str().expect("sibling path")));
        });
    }
}
