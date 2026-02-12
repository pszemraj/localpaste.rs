//! Paste HTTP handlers.

use super::deprecation::{warn_folder_deprecation, with_folder_deprecation_headers};
use crate::{error::HttpError, models::paste::*, naming, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Response},
    Json,
};

/// Create a new paste.
///
/// # Arguments
/// - `state`: Application state.
/// - `req`: Paste creation payload.
///
/// # Returns
/// The created paste as JSON.
///
/// # Errors
/// Returns an error if validation or persistence fails.
pub async fn create_paste(
    State(state): State<AppState>,
    Json(mut req): Json<CreatePasteRequest>,
) -> Result<Response, HttpError> {
    let folder_field_used = req.folder_id.is_some();

    // Check paste size limit
    if req.content.len() > state.config.max_paste_size {
        return Err(AppError::BadRequest(format!(
            "Paste size exceeds maximum of {} bytes",
            state.config.max_paste_size
        ))
        .into());
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
                ))
                .into());
            }
        }
    }

    let name = req.name.unwrap_or_else(naming::generate_name);
    let mut paste = Paste::new(req.content, name);
    let language_is_manual = req.language_is_manual;

    if let Some(ref folder_id) = req.folder_id {
        paste.folder_id = Some(folder_id.clone());
    }

    if let Some(tags) = req.tags {
        paste.tags = tags;
    }

    if let Some(language) = req.language {
        paste.language = Some(language);
        paste.language_is_manual = language_is_manual.unwrap_or(true);
    } else if let Some(is_manual) = language_is_manual {
        paste.language_is_manual = is_manual;
    }

    // Use transaction-like operation for atomic folder count update
    if let Some(ref folder_id) = paste.folder_id {
        crate::db::TransactionOps::create_paste_with_folder(&state.db, &paste, folder_id)?;
    } else {
        state.db.pastes.create(&paste)?;
    }

    if folder_field_used {
        warn_folder_deprecation("POST /api/paste with folder_id");
        Ok(with_folder_deprecation_headers(Json(paste)))
    } else {
        Ok(Json(paste).into_response())
    }
}

/// Fetch a paste by id.
///
/// # Arguments
/// - `state`: Application state.
/// - `id`: Paste identifier from the path.
///
/// # Returns
/// The paste as JSON.
///
/// # Errors
/// Returns an error if the paste does not exist or lookup fails.
pub async fn get_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Paste>, HttpError> {
    state
        .db
        .pastes
        .get(&id)?
        .map(Json)
        .ok_or_else(|| AppError::NotFound.into())
}

/// Update an existing paste.
///
/// # Arguments
/// - `state`: Application state.
/// - `id`: Paste identifier from the path.
/// - `req`: Paste update payload.
///
/// # Returns
/// Updated paste as JSON.
///
/// # Errors
/// Returns an error if validation or persistence fails.
pub async fn update_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<UpdatePasteRequest>,
) -> Result<Response, HttpError> {
    let folder_field_used = req.folder_id.is_some();

    // Check size limit if content is being updated
    if let Some(ref content) = req.content {
        if content.len() > state.config.max_paste_size {
            return Err(AppError::BadRequest(format!(
                "Paste size exceeds maximum of {} bytes",
                state.config.max_paste_size
            ))
            .into());
        }
    }

    let old_paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;

    // Validate new folder exists if specified
    if let Some(ref folder_id) = req.folder_id {
        if !folder_id.is_empty() && state.db.folders.get(folder_id)?.is_none() {
            return Err(AppError::BadRequest(format!(
                "Folder with id '{}' does not exist",
                folder_id
            ))
            .into());
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

        let updated = crate::db::TransactionOps::move_paste_between_folders(
            &state.db,
            &id,
            old_folder_id.as_deref(),
            new_folder_id.as_deref(),
            req,
        )?
        .ok_or(AppError::NotFound)?;
        if folder_field_used {
            warn_folder_deprecation("PUT /api/paste/:id with folder_id");
            Ok(with_folder_deprecation_headers(Json(updated)))
        } else {
            Ok(Json(updated).into_response())
        }
    } else {
        // folder_id not changing, just update the paste
        let updated = state
            .db
            .pastes
            .update(&id, req)?
            .ok_or(AppError::NotFound)?;
        if folder_field_used {
            warn_folder_deprecation("PUT /api/paste/:id with folder_id");
            Ok(with_folder_deprecation_headers(Json(updated)))
        } else {
            Ok(Json(updated).into_response())
        }
    }
}

/// Delete a paste by id.
///
/// # Arguments
/// - `state`: Application state.
/// - `id`: Paste identifier from the path.
///
/// # Returns
/// Success marker as JSON.
///
/// # Errors
/// Returns an error if deletion fails.
pub async fn delete_paste(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let _paste = state.db.pastes.get(&id)?.ok_or(AppError::NotFound)?;
    if state.locks.is_locked(&id) {
        return Err(AppError::Locked("Paste is currently open for editing.".to_string()).into());
    }

    // Use transaction-like operation for atomic folder count update.
    // The helper derives folder ownership from the deleted record to avoid stale-folder races.
    let deleted = crate::db::TransactionOps::delete_paste_with_folder(&state.db, &id)?;

    if deleted {
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(AppError::NotFound.into())
    }
}

/// List pastes with optional filters.
///
/// # Arguments
/// - `state`: Application state.
/// - `query`: List query parameters.
///
/// # Returns
/// Pastes as JSON.
///
/// # Errors
/// Returns an error if listing fails.
pub async fn list_pastes(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Response, HttpError> {
    let folder_filter_used = query
        .folder_id
        .as_deref()
        .map(str::trim)
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    let limit = query.limit.unwrap_or(50).min(100);
    let pastes = state.db.pastes.list(limit, query.folder_id)?;
    if folder_filter_used {
        warn_folder_deprecation("GET /api/pastes?folder_id=...");
        Ok(with_folder_deprecation_headers(Json(pastes)))
    } else {
        Ok(Json(pastes).into_response())
    }
}

/// Search pastes by query.
///
/// # Arguments
/// - `state`: Application state.
/// - `query`: Search query parameters.
///
/// # Returns
/// Matching pastes as JSON.
///
/// # Errors
/// Returns an error if search fails.
pub async fn search_pastes(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Response, HttpError> {
    let folder_filter_used = query
        .folder_id
        .as_deref()
        .map(str::trim)
        .map(|value| !value.is_empty())
        .unwrap_or(false);
    let limit = query.limit.unwrap_or(50).min(100);
    let pastes = state
        .db
        .pastes
        .search(&query.q, limit, query.folder_id, query.language)?;
    if folder_filter_used {
        warn_folder_deprecation("GET /api/search?folder_id=...");
        Ok(with_folder_deprecation_headers(Json(pastes)))
    } else {
        Ok(Json(pastes).into_response())
    }
}
