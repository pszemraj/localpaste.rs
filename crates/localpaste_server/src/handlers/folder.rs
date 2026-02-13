//! Folder HTTP handlers.

use super::deprecation::{warn_folder_deprecation, with_folder_deprecation_headers};
use super::normalize::{normalize_optional_for_create, normalize_optional_for_update};
use crate::{error::HttpError, models::folder::*, AppError, AppState};
use axum::{
    extract::{Path, State},
    response::Response,
    Json,
};
use localpaste_core::folder_ops::{
    delete_folder_tree_and_migrate, folder_delete_order, introduces_cycle,
};
use std::collections::HashSet;

fn first_locked_paste_in_folder_delete_set(
    state: &AppState,
    root_folder_id: &str,
) -> Result<Option<String>, AppError> {
    let folders = state.db.folders.list()?;
    if !folders.iter().any(|folder| folder.id == root_folder_id) {
        return Err(AppError::NotFound);
    }

    let delete_set: HashSet<String> = folder_delete_order(&folders, root_folder_id)
        .into_iter()
        .collect();
    for locked_id in state.locks.locked_ids() {
        if let Some(paste) = state.db.pastes.get(locked_id.as_str())? {
            if let Some(folder_id) = paste.folder_id.as_ref() {
                if delete_set.contains(folder_id) {
                    return Ok(Some(locked_id));
                }
            }
        }
    }
    Ok(None)
}

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
    Json(mut req): Json<CreateFolderRequest>,
) -> Result<Response, HttpError> {
    warn_folder_deprecation("POST /api/folder");
    req.parent_id = normalize_optional_for_create(req.parent_id);

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
    Json(mut req): Json<UpdateFolderRequest>,
) -> Result<Response, HttpError> {
    warn_folder_deprecation("PUT /api/folder/:id");
    req.parent_id = normalize_optional_for_update(req.parent_id);

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

    let folder = state
        .db
        .folders
        .update(&id, req.name, req.parent_id)?
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

    if let Some(locked_id) = first_locked_paste_in_folder_delete_set(&state, &id)? {
        return Err(AppError::Locked(format!(
            "Folder delete would migrate locked paste '{}'; close it first.",
            locked_id
        ))
        .into());
    }

    let _ = delete_folder_tree_and_migrate(&state.db, &id)?;

    Ok(with_folder_deprecation_headers(Json(
        serde_json::json!({ "success": true }),
    )))
}
