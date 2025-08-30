use crate::{
    error::AppError,
    models::paste::*,
    naming,
    AppState,
};
use axum::{
    extract::{Path, Query, State},
    Json,
};

pub async fn create_paste(
    State(state): State<AppState>,
    Json(req): Json<CreatePasteRequest>,
) -> Result<Json<Paste>, AppError> {
    let name = req.name.unwrap_or_else(naming::generate_name);
    let mut paste = Paste::new(req.content, name);
    
    if let Some(folder_id) = req.folder_id {
        paste.folder_id = Some(folder_id.clone());
        state.db.folders.update_count(&folder_id, 1)?;
    }
    
    if let Some(tags) = req.tags {
        paste.tags = tags;
    }
    
    if let Some(language) = req.language {
        paste.language = Some(language);
    }
    
    state.db.pastes.create(&paste)?;
    Ok(Json(paste))
}

pub async fn get_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Paste>, AppError> {
    state.db.pastes.get(&id)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

pub async fn update_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePasteRequest>,
) -> Result<Json<Paste>, AppError> {
    let old_paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;
    
    if req.folder_id != old_paste.folder_id {
        if let Some(ref old_folder) = old_paste.folder_id {
            state.db.folders.update_count(old_folder, -1)?;
        }
        if let Some(ref new_folder) = req.folder_id {
            state.db.folders.update_count(new_folder, 1)?;
        }
    }
    
    state.db.pastes.update(&id, req)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

pub async fn delete_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;
    
    if let Some(ref folder_id) = paste.folder_id {
        state.db.folders.update_count(folder_id, -1)?;
    }
    
    if state.db.pastes.delete(&id)? {
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(AppError::NotFound)
    }
}

pub async fn list_pastes(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Json<Vec<Paste>>, AppError> {
    let limit = query.limit.unwrap_or(50).min(100);
    let pastes = state.db.pastes.list(limit, query.folder_id)?;
    Ok(Json(pastes))
}

pub async fn search_pastes(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Vec<Paste>>, AppError> {
    let limit = query.limit.unwrap_or(50).min(100);
    let pastes = state.db.pastes.search(&query.q, limit)?;
    Ok(Json(pastes))
}