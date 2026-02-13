//! Shared filesystem copy utilities for database backup paths.

use crate::error::AppError;
use std::fs;
use std::path::Path;

/// Recursively copy a directory while preserving existing backup error semantics.
pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), AppError> {
    fs::create_dir_all(dst).map_err(|err| {
        AppError::DatabaseError(format!("Failed to create backup directory: {}", err))
    })?;

    for entry in fs::read_dir(src)
        .map_err(|err| AppError::DatabaseError(format!("Failed to read directory: {}", err)))?
    {
        let entry = entry.map_err(|err| {
            AppError::DatabaseError(format!("Failed to read directory entry: {}", err))
        })?;

        let path = entry.path();
        let file_name = entry.file_name();
        let dst_path = dst.join(&file_name);

        if path.is_dir() {
            copy_dir_recursive(&path, &dst_path)?;
        } else {
            fs::copy(&path, &dst_path).map_err(|err| {
                AppError::DatabaseError(format!("Failed to copy file {:?}: {}", path, err))
            })?;
        }
    }

    Ok(())
}
