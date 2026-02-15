//! Paste CRUD command handlers for the GUI backend worker.

use super::{send_error, validate_paste_size, validate_paste_size_bytes, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent};
use localpaste_core::{
    db::TransactionOps,
    models::paste::{self, UpdatePasteRequest},
    naming, AppError,
};
use localpaste_server::LockOwnerId;
use ropey::Rope;
use tracing::error;

fn begin_owner_mutation_guard<'a>(
    locks: &'a localpaste_server::PasteLockManager,
    owner_id: &LockOwnerId,
    id: &str,
) -> Result<localpaste_server::PasteMutationGuard<'a>, AppError> {
    locks
        .begin_mutation_ignoring_owner(id, owner_id)
        .map_err(|err| {
            localpaste_server::locks::map_paste_mutation_lock_error(
                err,
                "Paste is currently open for editing.",
            )
        })
}

fn map_gui_folder_not_found(err: AppError, folder_id: Option<&str>) -> AppError {
    match (err, folder_id) {
        (AppError::NotFound, Some(folder_id)) => {
            AppError::BadRequest(format!("folder '{}' does not exist", folder_id))
        }
        (err, _) => err,
    }
}

pub(super) fn handle_get_paste(state: &mut WorkerState, id: String) {
    match state.db.pastes.get(&id) {
        Ok(Some(paste)) => {
            let _ = state.evt_tx.send(CoreEvent::PasteLoaded { paste });
        }
        Ok(None) => {
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
        }
        Err(err) => {
            error!("backend get failed: {}", err);
            let _ = state.evt_tx.send(CoreEvent::PasteLoadFailed {
                id,
                message: format!("Get failed: {}", err),
            });
        }
    }
}

pub(super) fn handle_create_paste(state: &mut WorkerState, content: String) {
    if let Err(message) = validate_paste_size(content.as_str(), state.max_paste_size) {
        send_error(&state.evt_tx, CoreErrorSource::Other, message);
        return;
    }
    let inferred = paste::detect_language(&content);
    let name = naming::generate_name_for_content(&content, inferred.as_deref());
    let paste = paste::Paste::new(content, name);
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

fn apply_content_update(state: &mut WorkerState, id: String, content: String, log_label: &str) {
    if let Err(message) = validate_paste_size(content.as_str(), state.max_paste_size) {
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
    let _mutation_guard =
        match begin_owner_mutation_guard(state.locks.as_ref(), &state.lock_owner_id, id.as_str()) {
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
    match state.db.pastes.update(&id, update) {
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

pub(super) fn handle_update_paste(state: &mut WorkerState, id: String, content: String) {
    apply_content_update(state, id, content, "backend update failed");
}

pub(super) fn handle_update_paste_virtual(state: &mut WorkerState, id: String, content: Rope) {
    // Guard on rope byte length before materializing a String to avoid
    // allocation spikes when autosave tries to persist an oversized buffer.
    if let Err(message) = validate_paste_size_bytes(content.len_bytes(), state.max_paste_size) {
        send_error(&state.evt_tx, CoreErrorSource::SaveContent, message);
        return;
    }
    apply_content_update(
        state,
        id,
        content.to_string(),
        "backend virtual update failed",
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) fn handle_update_paste_meta(
    state: &mut WorkerState,
    id: String,
    name: Option<String>,
    language: Option<String>,
    language_is_manual: Option<bool>,
    folder_id: Option<String>,
    tags: Option<Vec<String>>,
) {
    // Intentionally gate metadata operations on paste existence before validating
    // destination folders so missing-paste responses are not masked by folder errors.
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
        let folder_guard = match TransactionOps::acquire_folder_txn_guard(&state.db) {
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
        let _mutation_guard = match begin_owner_mutation_guard(
            state.locks.as_ref(),
            &state.lock_owner_id,
            id.as_str(),
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
        .map_err(|err| map_gui_folder_not_found(err, new_folder_id.as_deref()))
    } else {
        let _mutation_guard = match begin_owner_mutation_guard(
            state.locks.as_ref(),
            &state.lock_owner_id,
            id.as_str(),
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

pub(super) fn handle_delete_paste(state: &mut WorkerState, id: String) {
    let folder_guard = match TransactionOps::acquire_folder_txn_guard(&state.db) {
        Ok(guard) => guard,
        Err(err) => {
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Delete failed: {}", err),
            );
            return;
        }
    };

    let _mutation_guard = match state
        .locks
        .begin_mutation_ignoring_owner(id.as_str(), &state.lock_owner_id)
        .map_err(|err| {
            localpaste_server::locks::map_paste_mutation_lock_error(
                err,
                "Paste is currently open for editing.",
            )
        }) {
        Ok(guard) => guard,
        Err(err) => {
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Delete failed: {}", err),
            );
            return;
        }
    };

    let deleted = TransactionOps::delete_paste_with_folder_locked(&state.db, &folder_guard, &id);
    match deleted {
        Ok(true) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::PasteDeleted { id });
        }
        Ok(false) => {
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
