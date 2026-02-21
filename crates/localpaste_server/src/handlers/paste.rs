//! Paste HTTP handlers.

use super::deprecation::maybe_with_folder_deprecation_headers;
use super::normalize::{normalize_optional_for_create, normalize_optional_for_update};
use crate::{error::HttpError, models::paste::*, naming, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::HeaderValue,
    response::Response,
    Json,
};
use localpaste_core::folder_ops::map_missing_folder_for_optional_request;

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

#[derive(Clone, Copy)]
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
    Json(req): Json<CreatePasteRequest>,
) -> Result<Response, HttpError> {
    let folder_field_used = req.folder_id.is_some();
    let CreatePasteRequest {
        content,
        language,
        language_is_manual,
        folder_id,
        tags,
        name,
    } = req;
    let normalized_folder_id = normalize_optional_for_create(folder_id);

    // Check paste size limit
    if content.len() > state.config.max_paste_size {
        return Err(AppError::BadRequest(format!(
            "Paste size exceeds maximum of {} bytes",
            state.config.max_paste_size
        ))
        .into());
    }

    let name = name.unwrap_or_else(naming::generate_name);
    let mut paste = if let Some(language) = language {
        Paste::new_with_language(
            content,
            name,
            Some(language),
            language_is_manual.unwrap_or(true),
        )
    } else {
        let mut inferred = Paste::new(content, name);
        if let Some(is_manual) = language_is_manual {
            inferred.language_is_manual = is_manual;
            if !is_manual {
                inferred.language = None;
            }
        }
        inferred
    };

    if let Some(ref folder_id) = normalized_folder_id {
        paste.folder_id = Some(folder_id.clone());
    }

    if let Some(tags) = tags {
        paste.tags = tags;
    }

    // Use transaction-like operation for atomic folder count update
    if let Some(ref folder_id) = paste.folder_id {
        crate::db::TransactionOps::create_paste_with_folder(&state.db, &paste, folder_id).map_err(
            |err| map_missing_folder_for_optional_request(err, Some(folder_id.as_str()), "Folder"),
        )?;
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

    let updated = if req.folder_id.is_some() {
        let (folder_guard, _mutation_guard) = crate::locks::acquire_folder_scoped_mutation_guards(
            state.db.as_ref(),
            state.locks.as_ref(),
            &id,
            "Paste is currently open for editing.",
            None,
        )?;
        let new_folder_id =
            req.folder_id
                .clone()
                .and_then(|f| if f.is_empty() { None } else { Some(f) });

        crate::db::TransactionOps::move_paste_between_folders_locked(
            &state.db,
            &folder_guard,
            &id,
            new_folder_id.as_deref(),
            req,
        )
        .map_err(|err| {
            map_missing_folder_for_optional_request(err, new_folder_id.as_deref(), "Folder")
        })?
        .ok_or(AppError::NotFound)?
    } else {
        let _mutation_guard = crate::locks::acquire_paste_mutation_guard(
            state.locks.as_ref(),
            &id,
            "Paste is currently open for editing.",
            None,
        )?;
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
    let (folder_guard, _mutation_guard) = crate::locks::acquire_folder_scoped_mutation_guards(
        state.db.as_ref(),
        state.locks.as_ref(),
        &id,
        "Paste is currently open for editing.",
        None,
    )?;
    let deleted =
        crate::db::TransactionOps::delete_paste_with_folder_locked(&state.db, &folder_guard, &id)?;

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

#[cfg(test)]
mod tests {
    use crate::{db::TransactionOps, AppState, Config, Database};
    use localpaste_core::models::{folder::Folder, paste::Paste};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};
    use tempfile::TempDir;

    fn setup_state_with_foldered_paste() -> (TempDir, AppState, String) {
        let temp_dir = TempDir::new().expect("temp dir");
        let db_path = temp_dir.path().join("db");
        let db = Database::new(db_path.to_str().expect("db path")).expect("open db");

        let folder = Folder::new("folder".to_string());
        let folder_id = folder.id.clone();
        db.folders.create(&folder).expect("create folder");

        let mut paste = Paste::new("content".to_string(), "name".to_string());
        paste.folder_id = Some(folder_id.clone());
        let paste_id = paste.id.clone();
        TransactionOps::create_paste_with_folder(&db, &paste, &folder_id).expect("create paste");

        let state = AppState::new(
            Config {
                db_path: db_path.to_string_lossy().to_string(),
                port: 3055,
                max_paste_size: 1024 * 1024,
                auto_save_interval: 500,
                auto_backup: false,
            },
            db,
        );
        (temp_dir, state, paste_id)
    }

    #[test]
    fn folder_scoped_mutation_waits_for_folder_lock_before_marking_mutating() {
        let (_temp_dir, state, paste_id) = setup_state_with_foldered_paste();
        let held_folder_guard =
            TransactionOps::acquire_folder_txn_guard(state.db.as_ref()).expect("hold folder lock");

        let worker_state = state.clone();
        let worker_paste_id = paste_id.clone();
        let (started_tx, started_rx) = mpsc::channel();
        let worker = thread::spawn(move || {
            started_tx.send(()).expect("started signal");
            let guards = crate::locks::acquire_folder_scoped_mutation_guards(
                worker_state.db.as_ref(),
                worker_state.locks.as_ref(),
                worker_paste_id.as_str(),
                "Paste is currently open for editing.",
                None,
            );
            guards.map(|_| ()).map_err(|err| format!("{:?}", err))
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker should start");
        thread::sleep(Duration::from_millis(50));

        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline {
            let guard = state
                .locks
                .begin_batch_mutation([paste_id.as_str()])
                .expect(
                    "folder delete should reserve affected ids while mutation waits on folder lock",
                );
            drop(guard);
            thread::sleep(Duration::from_millis(5));
        }

        drop(held_folder_guard);
        let worker_result = worker.join().expect("join worker");
        assert!(
            worker_result.is_ok(),
            "folder-scoped mutation guard acquisition should eventually succeed: {:?}",
            worker_result
        );
    }
}
