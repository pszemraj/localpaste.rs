use crate::{error::AppError, models::folder::*, models::paste::UpdatePasteRequest, AppState};
use axum::{
    extract::{Path, State},
    Json,
};

pub async fn create_folder(
    State(state): State<AppState>,
    Json(req): Json<CreateFolderRequest>,
) -> Result<Json<Folder>, AppError> {
    if let Some(ref parent_id) = req.parent_id {
        if state.db.folders.get(parent_id)?.is_none() {
            return Err(AppError::BadRequest(format!(
                "Parent folder with id '{}' does not exist",
                parent_id
            )));
        }
    }

    let folder = Folder::with_parent(req.name, req.parent_id);
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
    if let Some(ref parent_id) = req.parent_id {
        if parent_id == &id {
            return Err(AppError::BadRequest(
                "Folder cannot be its own parent".to_string(),
            ));
        }
        if !parent_id.is_empty() && state.db.folders.get(parent_id)?.is_none() {
            return Err(AppError::BadRequest(format!(
                "Parent folder with id '{}' does not exist",
                parent_id
            )));
        }
    }

    state
        .db
        .folders
        .update(&id, req.name, req.parent_id)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

pub async fn delete_folder(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let folders = state.db.folders.list()?;
    if !folders.iter().any(|f| f.id == id) {
        return Err(AppError::NotFound);
    }

    // Collect descendants (depth-first) so pastes can be migrated before deletion
    let mut to_visit = vec![id.clone()];
    let mut delete_order = Vec::new();
    while let Some(current) = to_visit.pop() {
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
