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

fn normalized_limit(limit: Option<usize>) -> usize {
    limit.unwrap_or(50).min(100)
}

fn normalize_folder_filter_for_query(folder_id: Option<String>) -> (Option<String>, bool) {
    let normalized = normalize_optional_for_create(folder_id);
    let used = normalized.is_some();
    (normalized, used)
}

fn normalize_search_filters_for_query(
    query: &SearchQuery,
) -> (usize, Option<String>, Option<String>, bool) {
    let limit = normalized_limit(query.limit);
    let normalized_language = normalize_optional_for_create(query.language.clone());
    let (normalized_folder_id, folder_filter_used) =
        normalize_folder_filter_for_query(query.folder_id.clone());
    (
        limit,
        normalized_folder_id,
        normalized_language,
        folder_filter_used,
    )
}

fn with_folder_metadata_response(response: Response, include_meta_shape_header: bool) -> Response {
    if include_meta_shape_header {
        with_meta_only_response_shape(response)
    } else {
        response
    }
}

fn map_paste_mutation_error(err: crate::PasteLockError) -> AppError {
    match err {
        crate::PasteLockError::Held { .. } | crate::PasteLockError::Mutating { .. } => {
            AppError::Locked("Paste is currently open for editing.".to_string())
        }
        crate::PasteLockError::Poisoned => {
            AppError::StorageMessage("Paste lock manager is unavailable.".to_string())
        }
        crate::PasteLockError::NotHeld { .. } => {
            AppError::StorageMessage(format!("Unexpected paste lock state: {}", err))
        }
    }
}

fn list_meta_response(
    state: &AppState,
    query: ListQuery,
    route_hint: &'static str,
    include_meta_shape_header: bool,
) -> Result<Response, HttpError> {
    let limit = normalized_limit(query.limit);
    let (normalized_folder_id, folder_filter_used) =
        normalize_folder_filter_for_query(query.folder_id);
    let items = state.db.pastes.list_meta(limit, normalized_folder_id)?;
    let response =
        maybe_with_folder_deprecation_headers(Json(items), folder_filter_used, route_hint);
    Ok(with_folder_metadata_response(
        response,
        include_meta_shape_header,
    ))
}

enum SearchMode {
    Canonical,
    MetaOnly,
}

fn search_meta_response(
    state: &AppState,
    query: SearchQuery,
    mode: SearchMode,
    route_hint: &'static str,
    include_meta_shape_header: bool,
) -> Result<Response, HttpError> {
    let (limit, normalized_folder_id, normalized_language, folder_filter_used) =
        normalize_search_filters_for_query(&query);
    let items = match mode {
        SearchMode::Canonical => {
            // Preserve content-match semantics from canonical search while returning
            // metadata rows to avoid large full-content responses.
            state
                .db
                .pastes
                .search(&query.q, limit, normalized_folder_id, normalized_language)?
        }
        SearchMode::MetaOnly => state.db.pastes.search_meta(
            &query.q,
            limit,
            normalized_folder_id,
            normalized_language,
        )?,
    };
    let response =
        maybe_with_folder_deprecation_headers(Json(items), folder_filter_used, route_hint);
    Ok(with_folder_metadata_response(
        response,
        include_meta_shape_header,
    ))
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

    let _mutation_guard = state
        .locks
        .begin_mutation(&id)
        .map_err(map_paste_mutation_error)?;
    let updated = if req.folder_id.is_some() {
        // Explicit folder operations (including clear-to-unfiled) use CAS-backed
        // transaction logic to avoid stale-read folder count drift under concurrency.
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
    let _mutation_guard = state
        .locks
        .begin_mutation(&id)
        .map_err(map_paste_mutation_error)?;
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
    // This route intentionally returns metadata only to cap payload size.
    list_meta_response(&state, query, "GET /api/pastes?folder_id=...", true)
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
    list_meta_response(&state, query, "GET /api/pastes/meta?folder_id=...", false)
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
    search_meta_response(
        &state,
        query,
        SearchMode::Canonical,
        "GET /api/search?folder_id=...",
        true,
    )
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
    search_meta_response(
        &state,
        query,
        SearchMode::MetaOnly,
        "GET /api/search/meta?folder_id=...",
        false,
    )
}
