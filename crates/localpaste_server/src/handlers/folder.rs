//! Folder HTTP handlers.

use super::deprecation::{warn_folder_deprecation, with_folder_deprecation_headers};
use crate::{error::HttpError, models::folder::*, AppError, AppState};
use axum::{
    extract::{Path, State},
    response::Response,
    Json,
};
use localpaste_core::folder_ops::{
    create_folder_validated, delete_folder_tree_and_migrate_guarded, update_folder_validated,
};

/// Create a new folder.
///
/// # Arguments
/// - `state`: Application state.
/// - `req`: Folder creation payload.
///
/// # Returns
/// The created folder as JSON.
///
/// # Errors
/// Returns an error if validation or persistence fails.
pub async fn create_folder(
    State(state): State<AppState>,
    Json(req): Json<CreateFolderRequest>,
) -> Result<Response, HttpError> {
    warn_folder_deprecation("POST /api/folder");
    let folder = create_folder_validated(&state.db, req.name, req.parent_id)?;
    Ok(with_folder_deprecation_headers(Json(folder)))
}

/// List all folders.
///
/// # Returns
/// All folders as JSON.
///
/// # Errors
/// Returns an error if listing fails.
pub async fn list_folders(State(state): State<AppState>) -> Result<Response, HttpError> {
    warn_folder_deprecation("GET /api/folders");

    let folders = state.db.folders.list()?;
    Ok(with_folder_deprecation_headers(Json(folders)))
}

/// Update a folder's name or parent.
///
/// # Arguments
/// - `state`: Application state.
/// - `id`: Folder identifier from the path.
/// - `req`: Folder update payload.
///
/// # Returns
/// Updated folder as JSON.
///
/// # Errors
/// Returns an error if validation or persistence fails.
///
/// # Panics
/// Does not intentionally panic; any panic indicates a logic bug.
pub async fn update_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateFolderRequest>,
) -> Result<Response, HttpError> {
    warn_folder_deprecation("PUT /api/folder/:id");
    let folder = update_folder_validated(&state.db, &id, req.name, req.parent_id)?
        .ok_or(AppError::NotFound)?;
    Ok(with_folder_deprecation_headers(Json(folder)))
}

/// Delete a folder and migrate its pastes to unfiled.
///
/// # Arguments
/// - `state`: Application state.
/// - `id`: Folder identifier from the path.
///
/// # Returns
/// Success marker as JSON.
///
/// # Errors
/// Returns an error if deletion or migration fails.
pub async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, HttpError> {
    warn_folder_deprecation("DELETE /api/folder/:id");

    let _ = delete_folder_tree_and_migrate_guarded(&state.db, &id, |affected_paste_ids| {
        state
            .locks
            .begin_batch_mutation(affected_paste_ids.iter())
            .map_err(crate::locks::map_folder_delete_lock_error)
    })?;

    Ok(with_folder_deprecation_headers(Json(
        serde_json::json!({ "success": true }),
    )))
}
