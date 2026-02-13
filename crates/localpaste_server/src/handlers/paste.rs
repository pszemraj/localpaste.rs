//! Paste HTTP handlers.

use super::deprecation::maybe_with_folder_deprecation_headers;
use super::normalize::{
    normalize_optional_for_create, normalize_optional_for_update,
    validate_assignable_folder_for_request,
};
use crate::{error::HttpError, models::paste::*, naming, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::HeaderValue,
    response::Response,
    Json,
};

const RESPONSE_SHAPE_HEADER: &str = "x-localpaste-response-shape";
const META_RESPONSE_SHAPE: &str = "meta-only";

fn with_meta_only_response_shape(mut response: Response) -> Response {
    response.headers_mut().insert(
        RESPONSE_SHAPE_HEADER,
        HeaderValue::from_static(META_RESPONSE_SHAPE),
    );
    response
}

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
    req.folder_id = normalize_optional_for_create(req.folder_id);

    // Check paste size limit
    if req.content.len() > state.config.max_paste_size {
        return Err(AppError::BadRequest(format!(
            "Paste size exceeds maximum of {} bytes",
            state.config.max_paste_size
        ))
        .into());
    }

    if let Some(ref folder_id) = req.folder_id {
        validate_assignable_folder_for_request(&state.db, folder_id, "Folder")?;
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

    Ok(maybe_with_folder_deprecation_headers(
        Json(paste),
        folder_field_used,
        "POST /api/paste with folder_id",
    ))
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
    Json(mut req): Json<UpdatePasteRequest>,
) -> Result<Response, HttpError> {
    if state.locks.is_locked(&id) {
        return Err(AppError::Locked("Paste is currently open for editing.".to_string()).into());
    }

    let folder_field_used = req.folder_id.is_some();
    req.folder_id = normalize_optional_for_update(req.folder_id);

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

    // Validate new folder exists if specified
    if let Some(ref folder_id) = req.folder_id {
        if !folder_id.is_empty() {
            validate_assignable_folder_for_request(&state.db, folder_id, "Folder")?;
        }
    }

    let updated = if req.folder_id.is_some() {
        // Explicit folder operations (including clear-to-unfiled) use CAS-backed transaction
        // logic to avoid stale-read folder count drift under concurrent updates.
        let new_folder_id =
            req.folder_id
                .clone()
                .and_then(|f| if f.is_empty() { None } else { Some(f) });

        crate::db::TransactionOps::move_paste_between_folders(
            &state.db,
            &id,
            new_folder_id.as_deref(),
            req,
        )?
        .ok_or(AppError::NotFound)?
    } else {
        // folder_id not changing, just update the paste
        state
            .db
            .pastes
            .update(&id, req)?
            .ok_or(AppError::NotFound)?
    };

    Ok(maybe_with_folder_deprecation_headers(
        Json(updated),
        folder_field_used,
        "PUT /api/paste/:id with folder_id",
    ))
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
/// Metadata rows as JSON.
///
/// # Errors
/// Returns an error if listing fails.
pub async fn list_pastes(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Response, HttpError> {
    let normalized_folder_id = normalize_optional_for_create(query.folder_id);
    let folder_filter_used = normalized_folder_id.is_some();
    let limit = query.limit.unwrap_or(50).min(100);
    // This route intentionally returns metadata only to cap payload size.
    let pastes = state.db.pastes.list_meta(limit, normalized_folder_id)?;
    Ok(with_meta_only_response_shape(
        maybe_with_folder_deprecation_headers(
            Json(pastes),
            folder_filter_used,
            "GET /api/pastes?folder_id=...",
        ),
    ))
}

/// List paste metadata with optional filters.
///
/// # Arguments
/// - `state`: Application state.
/// - `query`: List query parameters.
///
/// # Returns
/// Metadata rows as JSON.
///
/// # Errors
/// Returns an error if listing fails.
pub async fn list_pastes_meta(
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Response, HttpError> {
    let normalized_folder_id = normalize_optional_for_create(query.folder_id);
    let folder_filter_used = normalized_folder_id.is_some();
    let limit = query.limit.unwrap_or(50).min(100);
    let metas = state.db.pastes.list_meta(limit, normalized_folder_id)?;
    Ok(maybe_with_folder_deprecation_headers(
        Json(metas),
        folder_filter_used,
        "GET /api/pastes/meta?folder_id=...",
    ))
}

/// Search pastes by query.
///
/// # Arguments
/// - `state`: Application state.
/// - `query`: Search query parameters.
///
/// # Returns
/// Matching metadata rows as JSON.
///
/// # Errors
/// Returns an error if search fails.
pub async fn search_pastes(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Response, HttpError> {
    let normalized_language = normalize_optional_for_create(query.language);
    let normalized_folder_id = normalize_optional_for_create(query.folder_id);
    let folder_filter_used = normalized_folder_id.is_some();
    let limit = query.limit.unwrap_or(50).min(100);
    // Preserve content-match semantics from canonical search while returning
    // metadata rows to avoid large full-content responses.
    let pastes =
        state
            .db
            .pastes
            .search(&query.q, limit, normalized_folder_id, normalized_language)?;
    let metas: Vec<PasteMeta> = pastes.iter().map(PasteMeta::from).collect();
    Ok(with_meta_only_response_shape(
        maybe_with_folder_deprecation_headers(
            Json(metas),
            folder_filter_used,
            "GET /api/search?folder_id=...",
        ),
    ))
}

/// Search paste metadata by query.
///
/// Metadata search matches name/tags/language and does not scan content.
///
/// # Arguments
/// - `state`: Application state.
/// - `query`: Search query parameters.
///
/// # Returns
/// Matching metadata rows as JSON.
///
/// # Errors
/// Returns an error if search fails.
pub async fn search_pastes_meta(
    State(state): State<AppState>,
    Query(query): Query<SearchQuery>,
) -> Result<Response, HttpError> {
    let normalized_language = normalize_optional_for_create(query.language);
    let normalized_folder_id = normalize_optional_for_create(query.folder_id);
    let folder_filter_used = normalized_folder_id.is_some();
    let limit = query.limit.unwrap_or(50).min(100);
    let metas =
        state
            .db
            .pastes
            .search_meta(&query.q, limit, normalized_folder_id, normalized_language)?;
    Ok(maybe_with_folder_deprecation_headers(
        Json(metas),
        folder_filter_used,
        "GET /api/search/meta?folder_id=...",
    ))
}
