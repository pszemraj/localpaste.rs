use crate::{error::AppError, models::paste::*, naming, AppState};
use axum::{
    extract::{Path, Query, State},
    Json,
};

pub async fn create_paste(
    State(state): State<AppState>,
    Json(mut req): Json<CreatePasteRequest>,
) -> Result<Json<Paste>, AppError> {
    // Check paste size limit
    if req.content.len() > state.config.max_paste_size {
        return Err(AppError::BadRequest(format!(
            "Paste size exceeds maximum of {} bytes",
            state.config.max_paste_size
        )));
    }

    // Normalize empty string folder_id to None
    if let Some(ref folder_id) = req.folder_id {
        if folder_id.is_empty() {
            req.folder_id = None;
        }
    }

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
    state
        .db
        .pastes
        .get(&id)?
        .map(Json)
        .ok_or(AppError::NotFound)
}

pub async fn update_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePasteRequest>,
) -> Result<Json<Paste>, AppError> {
    // Check size limit if content is being updated
    if let Some(ref content) = req.content {
        if content.len() > state.config.max_paste_size {
            return Err(AppError::BadRequest(format!(
                "Paste size exceeds maximum of {} bytes",
                state.config.max_paste_size
            )));
        }
    }

    let old_paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;

    // Check if folder_id is changing (DB layer will normalize empty string to None)
    let new_folder_id =
        req.folder_id
            .as_ref()
            .and_then(|f| if f.is_empty() { None } else { Some(f.as_str()) });
    let old_folder_id = old_paste.folder_id.as_deref();

    if new_folder_id != old_folder_id {
        if let Some(old_folder) = old_folder_id {
            state.db.folders.update_count(old_folder, -1)?;
        }
        if let Some(new_folder) = new_folder_id {
            state.db.folders.update_count(new_folder, 1)?;
        }
    }

    state
        .db
        .pastes
        .update(&id, req)?
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
    let pastes = state
        .db
        .pastes
        .search(&query.q, limit, query.folder_id, query.language)?;
    Ok(Json(pastes))
}
