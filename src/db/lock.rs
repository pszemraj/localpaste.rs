use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use crate::error::AppError;

/// Lock file manager for handling database locks gracefully
pub struct LockManager {
    lock_path: PathBuf,
}

impl LockManager {
    pub fn new(db_path: &str) -> Self {
        let lock_path = PathBuf::from(format!("{}.lock", db_path));
        Self { lock_path }
    }

    /// Check if a lock file exists and if it's stale
    pub fn check_lock(&self) -> LockStatus {
        if !self.lock_path.exists() {
            return LockStatus::Unlocked;
        }

        // Check if lock file contains a PID and if that process is still running
        if let Ok(contents) = fs::read_to_string(&self.lock_path) {
            if let Ok(pid) = contents.trim().parse::<u32>() {
                if is_process_running(pid) {
                    return LockStatus::LockedByProcess(pid);
                }
            }
        }

        // Check lock file age
        if let Ok(metadata) = fs::metadata(&self.lock_path) {
            if let Ok(modified) = metadata.modified() {
                if let Ok(elapsed) = SystemTime::now().duration_since(modified) {
                    // Consider lock stale after 60 seconds
                    if elapsed.as_secs() > 60 {
                        return LockStatus::StaleLock;
                    }
                }
            }
        }

        LockStatus::LockedUnknown
    }

    /// Try to clean up a stale lock
    pub fn cleanup_stale_lock(&self) -> Result<(), AppError> {
        match self.check_lock() {
            LockStatus::StaleLock => {
                tracing::warn!("Removing stale lock file: {:?}", self.lock_path);
                fs::remove_file(&self.lock_path).map_err(|e| {
                    AppError::DatabaseError(format!("Failed to remove stale lock: {}", e))
                })?;
                Ok(())
            }
            LockStatus::Unlocked => Ok(()),
            LockStatus::LockedByProcess(pid) => {
                Err(AppError::DatabaseError(format!(
                    "Database is locked by running process (PID: {})", pid
                )))
            }
            LockStatus::LockedUnknown => {
                Err(AppError::DatabaseError(
                    "Database is locked by unknown process".to_string()
                ))
            }
        }
    }

    /// Force unlock (use with caution!)
    pub fn force_unlock(&self) -> Result<(), AppError> {
        if self.lock_path.exists() {
            tracing::warn!("Force removing lock file: {:?}", self.lock_path);
            fs::remove_file(&self.lock_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to force remove lock: {}", e))
            })?;
        }
        Ok(())
    }

    /// Create a backup of the database before potentially destructive operations
    pub fn backup_database(db_path: &str) -> Result<String, AppError> {
        let db_path = Path::new(db_path);
        if !db_path.exists() {
            return Ok(String::new());
        }

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        
        let backup_path = db_path.with_extension(format!("backup.{}", timestamp));
        
        // Copy the entire database directory
        if db_path.is_dir() {
            copy_dir_recursive(db_path, &backup_path)?;
        } else {
            fs::copy(db_path, &backup_path).map_err(|e| {
                AppError::DatabaseError(format!("Failed to backup database: {}", e))
            })?;
        }

        tracing::info!("Created database backup at: {:?}", backup_path);
        Ok(backup_path.to_string_lossy().to_string())
    }
}

pub enum LockStatus {
    Unlocked,
    LockedByProcess(u32),
    StaleLock,
    LockedUnknown,
}

/// Check if a process with given PID is running
#[cfg(unix)]
fn is_process_running(pid: u32) -> bool {
    use std::process::Command;
    
    Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_process_running(_pid: u32) -> bool {
    // On non-Unix systems, we can't easily check if a process is running
    // Be conservative and assume it is
    true
}

/// Recursively copy a directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dst).map_err(|e| {
        AppError::DatabaseError(format!("Failed to create backup directory: {}", e))
    })?;

    for entry in fs::read_dir(src).map_err(|e| {
        AppError::DatabaseError(format!("Failed to read directory: {}", e))
    })? {
        let entry = entry.map_err(|e| {
            AppError::DatabaseError(format!("Failed to read directory entry: {}", e))
        })?;
        
        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            fs::copy(&path, &dst_path).map_err(|e| {
                AppError::DatabaseError(format!(
                    "Failed to copy file {:?}: {}", path, e
                ))
            })?;
        }
    }

    Ok(())
}