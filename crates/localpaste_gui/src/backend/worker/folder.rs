//! Folder command handlers for the GUI backend worker.

use super::{send_error, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent};
use localpaste_core::{
    folder_ops::{
        delete_folder_tree_and_migrate_guarded, ensure_folder_assignable, introduces_cycle,
    },
    models::folder::Folder,
    AppError,
};
use tracing::error;

pub(super) fn handle_list_folders(state: &mut WorkerState) {
    match state.db.folders.list() {
        Ok(items) => {
            let _ = state.evt_tx.send(CoreEvent::FoldersLoaded { items });
        }
        Err(err) => {
            error!("backend list folders failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("List folders failed: {}", err),
            );
        }
    }
}

pub(super) fn handle_create_folder(
    state: &mut WorkerState,
    name: String,
    parent_id: Option<String>,
) {
    let normalized_parent = parent_id
        .map(|pid| pid.trim().to_string())
        .filter(|pid| !pid.is_empty());
    if let Some(parent_id) = normalized_parent.as_deref() {
        if let Err(err) = ensure_folder_assignable(&state.db, parent_id) {
            let message = match err {
                AppError::NotFound => {
                    format!(
                        "Create folder failed: parent '{}' does not exist",
                        parent_id
                    )
                }
                other => format!("Create folder failed: {}", other),
            };
            send_error(&state.evt_tx, CoreErrorSource::Other, message);
            return;
        }
    }

    let folder = Folder::with_parent(name, normalized_parent);
    match state.db.folders.create(&folder) {
        Ok(()) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::FolderSaved { folder });
        }
        Err(err) => {
            error!("backend create folder failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Create folder failed: {}", err),
            );
        }
    }
}

pub(super) fn handle_update_folder(
    state: &mut WorkerState,
    id: String,
    name: String,
    parent_id: Option<String>,
) {
    // Preserve API semantics:
    // - `None` => leave parent unchanged
    // - `Some(\"\")` => clear parent (top-level)
    // - `Some(\"id\")` => set explicit parent
    let parent_update = parent_id.map(|pid| pid.trim().to_string());
    let normalized_parent = parent_update.as_ref().and_then(|pid| match pid.trim() {
        "" => None,
        trimmed => Some(trimmed),
    });
    if normalized_parent == Some(id.as_str()) {
        send_error(
            &state.evt_tx,
            CoreErrorSource::Other,
            "Update folder failed: folder cannot be its own parent".to_string(),
        );
        return;
    }

    if let Some(parent_id) = normalized_parent {
        let folders = match state.db.folders.list() {
            Ok(folders) => folders,
            Err(err) => {
                send_error(
                    &state.evt_tx,
                    CoreErrorSource::Other,
                    format!("Update folder failed: {}", err),
                );
                return;
            }
        };

        if folders.iter().all(|f| f.id != parent_id) {
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!(
                    "Update folder failed: parent '{}' does not exist",
                    parent_id
                ),
            );
            return;
        }
        if let Err(err) = ensure_folder_assignable(&state.db, parent_id) {
            let message = match err {
                AppError::NotFound => {
                    format!(
                        "Update folder failed: parent '{}' does not exist",
                        parent_id
                    )
                }
                other => format!("Update folder failed: {}", other),
            };
            send_error(&state.evt_tx, CoreErrorSource::Other, message);
            return;
        }

        if introduces_cycle(&folders, &id, parent_id) {
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                "Update folder failed: would create cycle".to_string(),
            );
            return;
        }
    }

    match state.db.folders.update(&id, name, parent_update) {
        Ok(Some(folder)) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::FolderSaved { folder });
        }
        Ok(None) => {
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                "Update folder failed: folder not found".to_string(),
            );
        }
        Err(err) => {
            error!("backend update folder failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Update folder failed: {}", err),
            );
        }
    }
}

pub(super) fn handle_delete_folder(state: &mut WorkerState, id: String) {
    let delete_result =
        delete_folder_tree_and_migrate_guarded(&state.db, &id, |affected_paste_ids| {
            state
                .locks
                .begin_batch_mutation(affected_paste_ids.iter())
                .map_err(localpaste_server::locks::map_folder_delete_lock_error)
        });

    match delete_result {
        Ok(_) => {
            state.query_cache.invalidate();
            let _ = state.evt_tx.send(CoreEvent::FolderDeleted { id });
        }
        Err(err) => {
            error!("backend delete folder failed: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Delete folder failed: {}", err),
            );
        }
    }
}
