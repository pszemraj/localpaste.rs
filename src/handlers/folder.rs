use crate::{error::AppError, models::folder::*, AppState};
use axum::{
    extract::{Path, State},
    Json,
};

pub async fn create_folder(
    State(state): State<AppState>,
    Json(req): Json<CreateFolderRequest>,
) -> Result<Json<Folder>, AppError> {
    let folder = Folder::new(req.name);
    state.db.folders.create(&folder)?;
    Ok(Json(folder))
}

pub async fn list_folders(State(state): State<AppState>) -> Result<Json<Vec<Folder>>, AppError> {
    let folders = state.db.folders.list()?;
    Ok(Json(folders))
}

pub async fn update_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdateFolderRequest>,
) -> Result<Json<Folder>, AppError> {
    state
        .db
        .folders
        .update(&id, req.name)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

pub async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    // First, move all pastes in this folder to unfiled
    let pastes = state.db.pastes.list(100, Some(id.clone()))?;
    for paste in pastes {
        let update = crate::models::paste::UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            folder_id: Some("".to_string()), // Empty string to make unfiled
            tags: None,
        };
        state.db.pastes.update(&paste.id, update)?;
    }

    if state.db.folders.delete(&id)? {
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(AppError::NotFound)
    }
}
