//! Paste CRUD command handlers for the GUI backend worker.

use super::{send_error, WorkerState, DELETE_UNDO_VERSION_PAYLOAD_LIMIT_BYTES};
use crate::backend::{CoreErrorSource, CoreEvent, VERSION_WORKFLOW_LIST_LIMIT};
use localpaste_core::{
    db::TransactionOps,
    diff::{unified_diff_lines, DiffResponse},
    folder_ops::map_missing_folder_for_optional_request,
    models::paste::{self, UpdatePasteRequest},
    naming,
    validation::paste_content_size_error,
};
use ropey::Rope;
use tracing::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PasteLoadRoute {
    Selection,
    DiffTarget,
}

/// Fetches a paste by id and emits load/missing/error events.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `id`: Paste id to load.
pub(super) fn handle_get_paste(state: &mut WorkerState, id: String) {
    handle_get_paste_for_route(state, id, PasteLoadRoute::Selection);
}

/// Fetches a detached diff target paste by id and emits diff-specific events.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `id`: Paste id to load for detached comparison.
pub(super) fn handle_get_diff_target_paste(state: &mut WorkerState, id: String) {
    handle_get_paste_for_route(state, id, PasteLoadRoute::DiffTarget);
}

fn handle_get_paste_for_route(state: &mut WorkerState, id: String, route: PasteLoadRoute) {
    match state.db.pastes.get(&id) {
        Ok(Some(paste)) => {
            let event = match route {
                PasteLoadRoute::Selection => CoreEvent::PasteLoaded { paste },
                PasteLoadRoute::DiffTarget => CoreEvent::DiffTargetLoaded { paste },
            };
            let _ = state.evt_tx.send(event);
        }
        Ok(None) => {
            let event = match route {
                PasteLoadRoute::Selection => CoreEvent::PasteMissing { id },
                PasteLoadRoute::DiffTarget => CoreEvent::DiffTargetMissing { id },
            };
            let _ = state.evt_tx.send(event);
        }
        Err(err) => {
            let (log_label, event) = match route {
                PasteLoadRoute::Selection => (
                    "backend get failed",
                    CoreEvent::PasteLoadFailed {
                        id,
                        message: format!("Get failed: {}", err),
                    },
                ),
                PasteLoadRoute::DiffTarget => (
                    "backend diff target get failed",
                    CoreEvent::DiffTargetLoadFailed {
                        id,
                        message: format!("Diff load failed: {}", err),
                    },
                ),
            };
            error!("{}: {}", log_label, err);
            let _ = state.evt_tx.send(event);
        }
    }
}

/// Creates a new paste from raw content and emits `PasteCreated` on success.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `content`: Paste body content.
pub(super) fn handle_create_paste(state: &mut WorkerState, content: String) {
    if let Some(message) = paste_content_size_error(content.len(), state.max_paste_size) {
        send_error(&state.evt_tx, CoreErrorSource::Other, message);
        return;
    }
    let inferred = paste::detect_language(&content);
    let inferred_is_locked = inferred.is_some();
    let name = naming::generate_name();
    let paste = paste::Paste::new_with_language(content, name, inferred, inferred_is_locked);
    match state.db.pastes.create(&paste) {
        Ok(()) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteCreated { paste });
        }
        Err(err) => {
            error!("backend create failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Create failed: {}", err),
            );
        }
    }
}

fn apply_content_update(
    state: &mut WorkerState,
    id: String,
    content: String,
    protected_version_id_ms: Option<u64>,
    log_label: &str,
) {
    if let Some(message) = paste_content_size_error(content.len(), state.max_paste_size) {
        send_error(&state.evt_tx, CoreErrorSource::SaveContent, message);
        return;
    }
    let update = UpdatePasteRequest {
        content: Some(content),
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: None,
        tags: None,
    };
    let _mutation_guard = match localpaste_server::locks::acquire_paste_mutation_guard(
        state.locks.as_ref(),
        id.as_str(),
        "Paste is currently open for editing.",
        Some(&state.lock_owner_id),
    ) {
        Ok(guard) => guard,
        Err(err) => {
            send_error(
                &state.evt_tx,
                CoreErrorSource::SaveContent,
                format!("Update failed: {}", err),
            );
            return;
        }
    };
    let update_result = if let Some(protected_version_id_ms) = protected_version_id_ms {
        state
            .db
            .pastes
            .update_preserving_version(&id, update, protected_version_id_ms)
    } else {
        state.db.pastes.update(&id, update)
    };
    match update_result {
        Ok(Some(paste)) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteSaved { paste });
        }
        Ok(None) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
        }
        Err(err) => {
            error!("{}: {}", log_label, err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::SaveContent,
                format!("Update failed: {}", err),
            );
        }
    }
}

/// Saves updated paste content from the virtual-editor rope buffer.
///
/// # Arguments
/// - `state`: Worker state containing db, locks, and event channel handles.
/// - `id`: Target paste id.
/// - `content`: Replacement content stored as a rope buffer.
pub(super) fn handle_update_paste_virtual(
    state: &mut WorkerState,
    id: String,
    content: Rope,
    protected_version_id_ms: Option<u64>,
) {
    if let Some(message) = paste_content_size_error(content.len_bytes(), state.max_paste_size) {
        send_error(&state.evt_tx, CoreErrorSource::SaveContent, message);
        return;
    }
    apply_content_update(
        state,
        id,
        content.to_string(),
        protected_version_id_ms,
        "backend virtual update failed",
    );
}

#[allow(clippy::too_many_arguments)]
/// Updates mutable paste metadata fields and emits meta save/missing/error events.
///
/// # Arguments
/// - `state`: Worker state containing db, locks, and event channel handles.
/// - `id`: Target paste id.
/// - `name`: Optional replacement name.
/// - `language`: Optional replacement language.
/// - `language_is_manual`: Optional manual-language intent override.
/// - `folder_id`: Optional replacement folder id (`Some("")` clears folder).
/// - `tags`: Optional replacement tag set.
pub(super) fn handle_update_paste_meta(
    state: &mut WorkerState,
    id: String,
    name: Option<String>,
    language: Option<String>,
    language_is_manual: Option<bool>,
    folder_id: Option<String>,
    tags: Option<Vec<String>>,
) {
    let _existing = match state.db.pastes.get(&id) {
        Ok(Some(paste)) => paste,
        Ok(None) => {
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
            return;
        }
        Err(err) => {
            error!("backend metadata load failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::SaveMetadata,
                format!("Metadata update failed: {}", err),
            );
            return;
        }
    };

    let normalized_folder_id = folder_id.map(|fid| {
        let trimmed = fid.trim().to_string();
        if trimmed.is_empty() {
            String::new()
        } else {
            trimmed
        }
    });

    let update = UpdatePasteRequest {
        content: None,
        name,
        language,
        language_is_manual,
        folder_id: normalized_folder_id.clone(),
        tags,
    };

    let result = if normalized_folder_id.is_some() {
        let (folder_guard, _mutation_guard) =
            match localpaste_server::locks::acquire_folder_scoped_mutation_guards(
                &state.db,
                state.locks.as_ref(),
                id.as_str(),
                "Paste is currently open for editing.",
                Some(&state.lock_owner_id),
            ) {
                Ok(guards) => guards,
                Err(err) => {
                    send_error(
                        &state.evt_tx,
                        CoreErrorSource::SaveMetadata,
                        format!("Metadata update failed: {}", err),
                    );
                    return;
                }
            };
        let new_folder_id =
            normalized_folder_id
                .clone()
                .and_then(|f| if f.is_empty() { None } else { Some(f) });
        TransactionOps::move_paste_between_folders_locked(
            &state.db,
            &folder_guard,
            &id,
            new_folder_id.as_deref(),
            update,
        )
        .map_err(|err| {
            map_missing_folder_for_optional_request(err, new_folder_id.as_deref(), "Folder")
        })
    } else {
        let _mutation_guard = match localpaste_server::locks::acquire_paste_mutation_guard(
            state.locks.as_ref(),
            id.as_str(),
            "Paste is currently open for editing.",
            Some(&state.lock_owner_id),
        ) {
            Ok(guard) => guard,
            Err(err) => {
                send_error(
                    &state.evt_tx,
                    CoreErrorSource::SaveMetadata,
                    format!("Metadata update failed: {}", err),
                );
                return;
            }
        };
        state.db.pastes.update(&id, update)
    };

    match result {
        Ok(Some(paste)) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteMetaSaved { paste });
        }
        Ok(None) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
        }
        Err(err) => {
            error!("backend metadata update failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::SaveMetadata,
                format!("Metadata update failed: {}", err),
            );
        }
    }
}

/// Deletes a paste under folder-transaction and lock guards.
///
/// # Arguments
/// - `state`: Worker state containing db, locks, and event channel handles.
/// - `id`: Paste id to delete.
pub(super) fn handle_delete_paste(state: &mut WorkerState, id: String) {
    let deleted = {
        let (folder_guard, _mutation_guard) =
            match localpaste_server::locks::acquire_folder_scoped_mutation_guards(
                &state.db,
                state.locks.as_ref(),
                id.as_str(),
                "Paste is currently open for editing.",
                Some(&state.lock_owner_id),
            ) {
                Ok(guards) => guards,
                Err(err) => {
                    send_error(
                        &state.evt_tx,
                        CoreErrorSource::Other,
                        format!("Delete failed: {}", err),
                    );
                    return;
                }
            };

        TransactionOps::delete_paste_with_folder_undo_limited_locked(
            &state.db,
            &folder_guard,
            &id,
            Some(DELETE_UNDO_VERSION_PAYLOAD_LIMIT_BYTES),
        )
    };
    match deleted {
        Ok(Some(result)) => {
            state.query_cache.invalidate();
            let undo_token = result
                .undo_bundle
                .map(|bundle| state.register_deleted_paste_undo(bundle));
            let _ = state
                .evt_tx
                .send(CoreEvent::PasteDeleted { id, undo_token });
        }
        Ok(None) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
        }
        Err(err) => {
            error!("backend delete failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Delete failed: {}", err),
            );
        }
    }
}

/// Restores a recently deleted paste from the backend undo buffer.
///
/// # Arguments
/// - `state`: Worker state containing db, undo buffer, and event channel handles.
/// - `undo_token`: Token emitted by a prior delete event.
pub(super) fn handle_restore_deleted_paste(state: &mut WorkerState, undo_token: String) {
    let Some(bundle) = state.pending_deleted_paste_bundle(undo_token.as_str()) else {
        let _ = state.evt_tx.send(CoreEvent::PasteRestoreFailed {
            undo_token,
            message: "Undo delete expired.".to_string(),
            retryable: false,
        });
        return;
    };

    match TransactionOps::restore_deleted_paste(&state.db, bundle) {
        Ok(paste) => {
            state.query_cache.invalidate();
            state.discard_deleted_paste_undo(undo_token.as_str());
            let _ = state
                .evt_tx
                .send(CoreEvent::PasteRestored { paste, undo_token });
        }
        Err(err) => {
            error!("backend restore deleted paste failed: {}", err);
            let _ = state.evt_tx.send(CoreEvent::PasteRestoreFailed {
                undo_token,
                message: format!("Undo delete failed: {}", err),
                retryable: true,
            });
        }
    }
}

/// Lists historical versions for a paste and emits version events.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `id`: Paste id to inspect.
/// - `limit`: Maximum version rows to return.
pub(super) fn handle_list_paste_versions(state: &mut WorkerState, id: String, limit: usize) {
    match state.db.pastes.list_versions(id.as_str(), Some(limit)) {
        Ok(Some(items)) => {
            let _ = state
                .evt_tx
                .send(CoreEvent::PasteVersionsLoaded { id, items });
        }
        Ok(None) => {
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
        }
        Err(err) => {
            error!("backend list versions failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("List versions failed: {}", err),
            );
        }
    }
}

/// Loads one historical version snapshot and emits version load/missing events.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `id`: Paste id to inspect.
/// - `version_id_ms`: Historical version id to load.
pub(super) fn handle_get_paste_version(state: &mut WorkerState, id: String, version_id_ms: u64) {
    match state.db.pastes.get_version(id.as_str(), version_id_ms) {
        Ok(Some(snapshot)) => {
            let _ = state
                .evt_tx
                .send(CoreEvent::PasteVersionLoaded { snapshot });
        }
        Ok(None) => match state.db.pastes.get(id.as_str()) {
            Ok(Some(_)) => {
                let message = format!("Version {} not found for paste {}.", version_id_ms, id);
                let _ = state.evt_tx.send(CoreEvent::PasteVersionLoadFailed {
                    paste_id: id,
                    version_id_ms,
                    message,
                });
            }
            Ok(None) => {
                let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
            }
            Err(err) => {
                let _ = state.evt_tx.send(CoreEvent::PasteVersionLoadFailed {
                    paste_id: id,
                    version_id_ms,
                    message: format!("Get version failed: {}", err),
                });
            }
        },
        Err(err) => {
            error!("backend get version failed: {}", err);
            let _ = state.evt_tx.send(CoreEvent::PasteVersionLoadFailed {
                paste_id: id,
                version_id_ms,
                message: format!("Get version failed: {}", err),
            });
        }
    }
}

/// Resets current paste content to a historical version.
///
/// # Arguments
/// - `state`: Worker state containing db, locks, and event channel handles.
/// - `id`: Target paste id.
/// - `version_id_ms`: Historical version id used as reset target.
pub(super) fn handle_reset_paste_hard_to_version(
    state: &mut WorkerState,
    id: String,
    version_id_ms: u64,
    preserve_current_head: bool,
) {
    let reset_result = {
        let _mutation_guard = match localpaste_server::locks::acquire_paste_mutation_guard(
            state.locks.as_ref(),
            id.as_str(),
            "Paste is currently open for editing.",
            Some(&state.lock_owner_id),
        ) {
            Ok(guard) => guard,
            Err(err) => {
                send_error(
                    &state.evt_tx,
                    CoreErrorSource::SaveContent,
                    format!("Reset hard failed: {}", err),
                );
                return;
            }
        };
        if preserve_current_head {
            state
                .db
                .pastes
                .reset_hard_to_version_preserving_current_head(
                    id.as_str(),
                    version_id_ms,
                    state.max_paste_size,
                )
        } else {
            state
                .db
                .pastes
                .reset_hard_to_version(id.as_str(), version_id_ms, state.max_paste_size)
        }
    };

    match reset_result {
        Ok(Some(paste)) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteResetToVersion { paste });
            // Reset refresh should preserve the same history window depth the GUI
            // requested for detached version workflows.
            handle_list_paste_versions(state, id, VERSION_WORKFLOW_LIST_LIMIT);
        }
        Ok(None) => match state.db.pastes.get(id.as_str()) {
            Ok(Some(_)) => send_error(
                &state.evt_tx,
                CoreErrorSource::SaveContent,
                format!("Reset hard failed: version {} not found.", version_id_ms),
            ),
            Ok(None) => {
                let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
            }
            Err(err) => send_error(
                &state.evt_tx,
                CoreErrorSource::SaveContent,
                format!("Reset hard failed: {}", err),
            ),
        },
        Err(err) => {
            error!("backend reset hard failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::SaveContent,
                format!("Reset hard failed: {}", err),
            );
        }
    }
}

/// Duplicates a historical version into a new paste.
///
/// # Arguments
/// - `state`: Worker state containing db and event channel handles.
/// - `id`: Source paste id.
/// - `version_id_ms`: Source historical version id.
/// - `name`: Optional explicit name for the duplicate.
pub(super) fn handle_duplicate_paste_version(
    state: &mut WorkerState,
    id: String,
    version_id_ms: u64,
    name: Option<String>,
) {
    match state.db.pastes.duplicate_from_version(
        id.as_str(),
        version_id_ms,
        state.max_paste_size,
        name,
    ) {
        Ok(Some(paste)) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteCreated { paste });
        }
        Ok(None) => match state.db.pastes.get(id.as_str()) {
            Ok(Some(_)) => send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!(
                    "Duplicate version failed: version {} not found.",
                    version_id_ms
                ),
            ),
            Ok(None) => {
                let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
            }
            Err(err) => send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Duplicate version failed: {}", err),
            ),
        },
        Err(err) => {
            error!("backend duplicate version failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Duplicate version failed: {}", err),
            );
        }
    }
}

/// Computes a diff preview from explicit left/right text snapshots off the UI thread.
///
/// # Arguments
/// - `state`: Worker state containing event channel handles.
/// - `request_id`: Caller-owned request token used to drop stale preview results.
/// - `left_text`: Frozen current-editor snapshot for the diff left side.
/// - `right_text`: Frozen comparison target snapshot for the diff right side.
pub(super) fn handle_compute_diff_preview(
    state: &mut WorkerState,
    request_id: u64,
    left_text: String,
    right_text: String,
) {
    let equal = left_text == right_text;
    let diff = DiffResponse {
        equal,
        unified: if equal {
            Vec::new()
        } else {
            unified_diff_lines(left_text.as_str(), right_text.as_str())
        },
    };
    let _ = state
        .evt_tx
        .send(CoreEvent::DiffPreviewComputed { request_id, diff });
}
