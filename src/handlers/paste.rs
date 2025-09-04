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
        } else {
            // Validate folder exists
            if state.db.folders.get(folder_id)?.is_none() {
                return Err(AppError::BadRequest(format!(
                    "Folder with id '{}' does not exist",
                    folder_id
                )));
            }
        }
    }

    let name = req.name.unwrap_or_else(naming::generate_name);
    let mut paste = Paste::new(req.content, name);

    if let Some(ref folder_id) = req.folder_id {
        paste.folder_id = Some(folder_id.clone());
    }

    if let Some(tags) = req.tags {
        paste.tags = tags;
    }

    if let Some(language) = req.language {
        paste.language = Some(language);
    }

    // Use transaction-like operation for atomic folder count update
    if let Some(ref folder_id) = paste.folder_id {
        crate::db::TransactionOps::create_paste_with_folder(&state.db, &paste, folder_id)?;
    } else {
        state.db.pastes.create(&paste)?;
    }

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

    // Validate new folder exists if specified
    if let Some(ref folder_id) = req.folder_id {
        if !folder_id.is_empty() && state.db.folders.get(folder_id)?.is_none() {
            return Err(AppError::BadRequest(format!(
                "Folder with id '{}' does not exist",
                folder_id
            )));
        }
    }

    // Check if folder_id is actually changing
    let folder_changing = req.folder_id.is_some() && {
        let new_folder =
            req.folder_id
                .as_ref()
                .and_then(|f| if f.is_empty() { None } else { Some(f.as_str()) });
        let old_folder = old_paste.folder_id.as_deref();
        new_folder != old_folder
    };

    if folder_changing {
        // folder_id is changing, use transaction for atomic count updates
        let new_folder_id =
            req.folder_id
                .clone()
                .and_then(|f| if f.is_empty() { None } else { Some(f) });
        let old_folder_id = old_paste.folder_id.clone();

        crate::db::TransactionOps::move_paste_between_folders(
            &state.db,
            &id,
            old_folder_id.as_deref(),
            new_folder_id.as_deref(),
            req,
        )?
        .map(Json)
        .ok_or(AppError::NotFound)
    } else {
        // folder_id not changing, just update the paste
        state
            .db
            .pastes
            .update(&id, req)?
            .map(Json)
            .ok_or(AppError::NotFound)
    }
}

pub async fn delete_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, AppError> {
    let paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;

    // Use transaction-like operation for atomic folder count update
    let deleted = if let Some(ref folder_id) = paste.folder_id {
        crate::db::TransactionOps::delete_paste_with_folder(&state.db, &id, folder_id)?
    } else {
        state.db.pastes.delete(&id)?
    };

    if deleted {
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
