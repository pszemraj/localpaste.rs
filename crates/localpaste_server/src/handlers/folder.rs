//! Folder HTTP handlers.

use crate::{
    error::HttpError, models::folder::*, models::paste::UpdatePasteRequest, AppError, AppState,
};
use axum::{
    extract::{Path, State},
    Json,
};
use std::collections::{HashMap, HashSet};

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
    let folders = state.db.folders.list()?;
    if !folders.iter().any(|f| f.id == id) {
        return Err(AppError::NotFound.into());
    }

    // Collect descendants (depth-first) so pastes can be migrated before deletion
    let mut to_visit = vec![id.clone()];
    let mut delete_order = Vec::new();
    let mut visited = HashSet::new();
    while let Some(current) = to_visit.pop() {
        if !visited.insert(current.clone()) {
            continue;
        }
        delete_order.push(current.clone());
        for child in folders
            .iter()
            .filter(|f| f.parent_id.as_deref() == Some(current.as_str()))
        {
            to_visit.push(child.id.clone());
        }
    }

    // Ensure children are deleted before parent
    delete_order.reverse();

    for folder_id in &delete_order {
        // Migrate all pastes in this folder to unfiled
        loop {
            let pastes = state.db.pastes.list(100, Some(folder_id.clone()))?;
            if pastes.is_empty() {
                break;
            }

            for paste in pastes {
                let update = UpdatePasteRequest {
                    content: None,
                    name: None,
                    language: None,
                    language_is_manual: None,
                    folder_id: Some(String::new()), // Normalized to None
                    tags: None,
                };
                state.db.pastes.update(&paste.id, update)?;
            }
        }

        state.db.folders.delete(folder_id)?;
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

fn introduces_cycle(folders: &[Folder], folder_id: &str, new_parent_id: &str) -> bool {
    let parent_map: HashMap<&str, Option<&str>> = folders
        .iter()
        .map(|f| (f.id.as_str(), f.parent_id.as_deref()))
        .collect();
    let mut current = Some(new_parent_id);
    let mut visited = HashSet::new();

    while let Some(curr) = current {
        if !visited.insert(curr) || curr == folder_id {
            return true;
        }
        current = parent_map.get(curr).copied().flatten();
    }

    false
}
