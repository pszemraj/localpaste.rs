//! Folder command handlers for the GUI backend worker.

use super::{send_error, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent};
use localpaste_core::folder_ops::{
    create_folder_validated, delete_folder_tree_and_migrate_guarded, update_folder_validated,
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
    match create_folder_validated(&state.db, name, parent_id) {
        Ok(folder) => {
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
    match update_folder_validated(&state.db, &id, name, parent_id) {
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
