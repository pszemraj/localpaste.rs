use super::{send_error, validate_paste_size, WorkerState};
use crate::backend::{CoreErrorSource, CoreEvent};
use localpaste_core::{
    db::TransactionOps,
    folder_ops::ensure_folder_assignable,
    models::paste::{self, UpdatePasteRequest},
    naming, AppError,
};
use ropey::Rope;
use tracing::error;

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

    if let Some(folder_id) = normalized_folder_id.as_ref().filter(|fid| !fid.is_empty()) {
        if let Err(err) = ensure_folder_assignable(&state.db, folder_id) {
            let message = match err {
                AppError::NotFound => format!(
                    "Metadata update failed: folder '{}' does not exist",
                    folder_id
                ),
                other => format!("Metadata update failed: {}", other),
            };
            send_error(&state.evt_tx, CoreErrorSource::SaveMetadata, message);
            return;
        }
    }

    let update = UpdatePasteRequest {
        content: None,
        name,
        language,
        language_is_manual,
        folder_id: normalized_folder_id.clone(),
        tags,
    };

    let result = if normalized_folder_id.is_some() {
        let new_folder_id =
            normalized_folder_id
                .clone()
                .and_then(|f| if f.is_empty() { None } else { Some(f) });
        TransactionOps::move_paste_between_folders(&state.db, &id, new_folder_id.as_deref(), update)
    } else {
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
    let _existing = match state.db.pastes.get(&id) {
        Ok(Some(paste)) => paste,
        Ok(None) => {
            let _ = state.evt_tx.send(CoreEvent::PasteMissing { id });
            return;
        }
        Err(err) => {
            error!("backend delete failed during lookup: {}", err);
            send_error(
                &state.evt_tx,
                CoreErrorSource::Other,
                format!("Delete failed: {}", err),
            );
            return;
        }
    };

    let deleted = TransactionOps::delete_paste_with_folder(&state.db, &id);
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
