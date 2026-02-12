//! Folder HTTP handlers.

use crate::{error::HttpError, models::folder::*, AppError, AppState};
use axum::{
    extract::{Path, State},
    Json,
};
use localpaste_core::folder_ops::{delete_folder_tree_and_migrate, introduces_cycle};

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
) -> Result<Json<Folder>, HttpError> {
    if let Some(ref parent_id) = req.parent_id {
        if state.db.folders.get(parent_id)?.is_none() {
            return Err(AppError::BadRequest(format!(
                "Parent folder with id '{}' does not exist",
                parent_id
            ))
            .into());
        }
    }

    let folder = Folder::with_parent(req.name, req.parent_id);
    state.db.folders.create(&folder)?;
    Ok(Json(folder))
}

/// List all folders.
///
/// # Returns
/// All folders as JSON.
///
/// # Errors
/// Returns an error if listing fails.
pub async fn list_folders(State(state): State<AppState>) -> Result<Json<Vec<Folder>>, HttpError> {
    let folders = state.db.folders.list()?;
    Ok(Json(folders))
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
) -> Result<Json<Folder>, HttpError> {
    let folders = if req
        .parent_id
        .as_ref()
        .map(|parent_id| !parent_id.is_empty())
        .unwrap_or(false)
    {
        Some(state.db.folders.list()?)
    } else {
        None
    };

    if let Some(ref parent_id) = req.parent_id {
        if parent_id == &id {
            return Err(AppError::BadRequest("Folder cannot be its own parent".to_string()).into());
        }
        if !parent_id.is_empty() {
            let folders = folders.as_ref().unwrap();
            if folders.iter().all(|f| f.id != *parent_id) {
                return Err(AppError::BadRequest(format!(
                    "Parent folder with id '{}' does not exist",
                    parent_id
                ))
                .into());
            }

            if introduces_cycle(folders, &id, parent_id) {
                return Err(AppError::BadRequest(
                    "Updating folder would create a cycle".to_string(),
                )
                .into());
            }
        }
    }

    state
        .db
        .folders
        .update(&id, req.name, req.parent_id)?
        .map(Json)
        .ok_or_else(|| AppError::NotFound.into())
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
) -> Result<Json<serde_json::Value>, HttpError> {
    let _ = delete_folder_tree_and_migrate(&state.db, &id)?;

    Ok(Json(serde_json::json!({ "success": true })))
}
