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

pub async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    if state.db.folders.delete(&id)? {
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(AppError::NotFound)
    }
}
