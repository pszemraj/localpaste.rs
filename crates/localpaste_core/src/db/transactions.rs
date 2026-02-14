//! Cross-tree transaction helpers for folder-affecting mutations.

use super::Database;
use crate::error::AppError;
use crate::folder_ops::ensure_folder_assignable;
#[cfg(test)]
use std::cell::RefCell;
#[cfg(test)]
use std::sync::Arc;
#[cfg(test)]
use std::sync::{Barrier, Mutex, OnceLock};

/// Transaction-like operations for atomic updates across trees.
///
/// Sled transactions are limited to a single tree, so we use careful ordering
/// and rollback logic to maintain consistency across trees.
pub struct TransactionOps;

#[cfg(test)]
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransactionFailpoint {
    CreateAfterDestinationReserveOnce,
    CreateDeleteDestinationAfterReserveOnce,
    CreateDeleteDestinationAfterCanonicalCreateOnce,
    MoveAfterDestinationReserveOnce,
    MoveDeleteDestinationAfterReserveOnce,
    MovePauseAfterDestinationReserveOnce,
}

#[cfg(test)]
thread_local! {
    static TRANSACTION_FAILPOINT: RefCell<Option<TransactionFailpoint>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_transaction_failpoint(failpoint: Option<TransactionFailpoint>) {
    TRANSACTION_FAILPOINT.with(|slot| {
        *slot.borrow_mut() = failpoint;
    });
}

#[cfg(test)]
fn take_transaction_failpoint() -> Option<TransactionFailpoint> {
    TRANSACTION_FAILPOINT.with(|slot| slot.borrow_mut().take())
}

#[cfg(test)]
fn restore_transaction_failpoint(failpoint: TransactionFailpoint) {
    TRANSACTION_FAILPOINT.with(|slot| {
        *slot.borrow_mut() = Some(failpoint);
    });
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct MovePauseHooks {
    pub(crate) reached: Arc<Barrier>,
    pub(crate) resume: Arc<Barrier>,
}

#[cfg(test)]
fn move_pause_hooks_slot() -> &'static Mutex<Option<MovePauseHooks>> {
    static SLOT: OnceLock<Mutex<Option<MovePauseHooks>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

#[cfg(test)]
pub(crate) fn set_move_pause_hooks(hooks: Option<MovePauseHooks>) {
    let mut slot = move_pause_hooks_slot()
        .lock()
        .expect("move pause hook lock poisoned");
    *slot = hooks;
}

#[cfg(test)]
fn take_move_pause_hooks() -> Option<MovePauseHooks> {
    move_pause_hooks_slot()
        .lock()
        .expect("move pause hook lock poisoned")
        .take()
}

#[cfg(test)]
fn apply_move_failpoint_after_destination_reserve(
    db: &Database,
    folder_changing: bool,
    new_folder_id: Option<&str>,
) -> Result<(), AppError> {
    let Some(failpoint) = take_transaction_failpoint() else {
        return Ok(());
    };

    match failpoint {
        TransactionFailpoint::CreateAfterDestinationReserveOnce
        | TransactionFailpoint::CreateDeleteDestinationAfterReserveOnce
        | TransactionFailpoint::CreateDeleteDestinationAfterCanonicalCreateOnce => {
            restore_transaction_failpoint(failpoint);
            Ok(())
        }
        TransactionFailpoint::MoveAfterDestinationReserveOnce => Err(AppError::StorageMessage(
            format!("Injected transaction failpoint: {:?}", failpoint),
        )),
        TransactionFailpoint::MoveDeleteDestinationAfterReserveOnce => {
            if folder_changing {
                if let Some(new_id) = new_folder_id {
                    db.folders.delete(new_id)?;
                }
            }
            Ok(())
        }
        TransactionFailpoint::MovePauseAfterDestinationReserveOnce => {
            if let Some(hooks) = take_move_pause_hooks() {
                hooks.reached.wait();
                hooks.resume.wait();
            }
            Ok(())
        }
    }
}

#[cfg(test)]
fn apply_create_failpoint_after_destination_reserve(
    db: &Database,
    folder_id: &str,
) -> Result<(), AppError> {
    let Some(failpoint) = take_transaction_failpoint() else {
        return Ok(());
    };

    match failpoint {
        TransactionFailpoint::CreateAfterDestinationReserveOnce => Err(AppError::StorageMessage(
            format!("Injected transaction failpoint: {:?}", failpoint),
        )),
        TransactionFailpoint::CreateDeleteDestinationAfterReserveOnce => {
            db.folders.delete(folder_id)?;
            Ok(())
        }
        TransactionFailpoint::CreateDeleteDestinationAfterCanonicalCreateOnce
        | TransactionFailpoint::MoveAfterDestinationReserveOnce
        | TransactionFailpoint::MoveDeleteDestinationAfterReserveOnce
        | TransactionFailpoint::MovePauseAfterDestinationReserveOnce => {
            restore_transaction_failpoint(failpoint);
            Ok(())
        }
    }
}

#[cfg(test)]
fn apply_create_failpoint_after_canonical_create(
    db: &Database,
    folder_id: &str,
) -> Result<(), AppError> {
    let Some(failpoint) = take_transaction_failpoint() else {
        return Ok(());
    };

    match failpoint {
        TransactionFailpoint::CreateDeleteDestinationAfterCanonicalCreateOnce => {
            db.folders.delete(folder_id)?;
            Ok(())
        }
        TransactionFailpoint::CreateAfterDestinationReserveOnce
        | TransactionFailpoint::CreateDeleteDestinationAfterReserveOnce
        | TransactionFailpoint::MoveAfterDestinationReserveOnce
        | TransactionFailpoint::MoveDeleteDestinationAfterReserveOnce
        | TransactionFailpoint::MovePauseAfterDestinationReserveOnce => {
            restore_transaction_failpoint(failpoint);
            Ok(())
        }
    }
}

fn rollback_destination_reservation(
    db: &Database,
    destination_folder_id: Option<&str>,
    context: &str,
    ignore_not_found: bool,
) {
    let Some(folder_id) = destination_folder_id else {
        return;
    };
    if let Err(err) = db.folders.update_count(folder_id, -1) {
        if ignore_not_found && matches!(err, AppError::NotFound) {
            return;
        }
        tracing::error!(
            "Failed to rollback destination folder count after {}: {}",
            context,
            err
        );
    }
}

impl TransactionOps {
    /// Acquire the global folder-transaction lock.
    ///
    /// This lock serializes folder-affecting flows (folder delete trees, folder-targeted
    /// paste create/move/delete operations, and folder-parent mutations) that cannot be
    /// represented as a single sled cross-tree transaction.
    ///
    /// # Returns
    /// A lock guard that must be held for the full critical section.
    ///
    /// # Errors
    /// Returns [`AppError::StorageMessage`] when the lock is poisoned.
    pub fn acquire_folder_txn_lock(
        db: &Database,
    ) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        db.folder_txn_lock
            .lock()
            .map_err(|_| AppError::StorageMessage("Folder transaction lock poisoned".to_string()))
    }

    /// Atomically create a paste and update folder count
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste`: Paste to insert.
    /// - `folder_id`: Folder that will contain the paste.
    ///
    /// # Returns
    /// `Ok(())` on success.
    ///
    /// # Errors
    /// Propagates storage errors from paste or folder updates.
    pub fn create_paste_with_folder(
        db: &Database,
        paste: &crate::models::paste::Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        let _guard = Self::acquire_folder_txn_lock(db)?;
        Self::create_paste_with_folder_locked(db, paste, folder_id)
    }

    pub(crate) fn create_paste_with_folder_locked(
        db: &Database,
        paste: &crate::models::paste::Paste,
        folder_id: &str,
    ) -> Result<(), AppError> {
        ensure_folder_assignable(db, folder_id)?;
        db.folders.update_count(folder_id, 1)?;

        #[cfg(test)]
        if let Err(err) = apply_create_failpoint_after_destination_reserve(db, folder_id) {
            rollback_destination_reservation(
                db,
                Some(folder_id),
                "injected create failpoint",
                true,
            );
            return Err(err);
        }

        if let Err(err) = ensure_folder_assignable(db, folder_id) {
            rollback_destination_reservation(
                db,
                Some(folder_id),
                "destination became unassignable before create",
                true,
            );
            return Err(err);
        }

        if let Err(e) = db.pastes.create(paste) {
            rollback_destination_reservation(db, Some(folder_id), "canonical create failure", true);
            return Err(e);
        }

        #[cfg(test)]
        if let Err(err) = apply_create_failpoint_after_canonical_create(db, folder_id) {
            if let Err(delete_err) = db.pastes.delete(&paste.id) {
                tracing::error!(
                    "Failed to delete just-created paste after injected post-create failpoint: {}",
                    delete_err
                );
            }
            rollback_destination_reservation(
                db,
                Some(folder_id),
                "injected post-create failpoint",
                true,
            );
            return Err(err);
        }

        if let Err(err) = ensure_folder_assignable(db, folder_id) {
            if let Err(delete_err) = db.pastes.delete(&paste.id) {
                tracing::error!(
                    "Failed to delete just-created paste after destination folder disappeared: {}",
                    delete_err
                );
            }
            rollback_destination_reservation(
                db,
                Some(folder_id),
                "post-create destination check",
                true,
            );
            // Canonical create may have committed before this point. Returning Err is intentional:
            // callers must retry because the destination became invalid and compensating actions
            // were already attempted above.
            return Err(err);
        }

        Ok(())
    }

    /// Atomically delete a paste and update folder count
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste_id`: Paste identifier to delete.
    ///
    /// # Returns
    /// `Ok(true)` if a paste was deleted, `Ok(false)` if not found.
    ///
    /// # Errors
    /// Propagates storage errors from the paste tree.
    pub fn delete_paste_with_folder(db: &Database, paste_id: &str) -> Result<bool, AppError> {
        let _guard = Self::acquire_folder_txn_lock(db)?;
        Self::delete_paste_with_folder_locked(db, paste_id)
    }

    pub(crate) fn delete_paste_with_folder_locked(
        db: &Database,
        paste_id: &str,
    ) -> Result<bool, AppError> {
        let deleted = db.pastes.delete_and_return(paste_id)?;

        if let Some(paste) = deleted {
            if let Some(folder_id) = paste.folder_id.as_deref() {
                // Update folder count - if this fails, log but continue
                // (paste is already deleted, better to have incorrect count than fail)
                if let Err(e) = db.folders.update_count(folder_id, -1) {
                    tracing::error!("Failed to update folder count after paste deletion: {}", e);
                }
            }

            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Atomically move a paste between folders
    ///
    /// # Arguments
    /// - `db`: Database handle.
    /// - `paste_id`: Paste identifier to update.
    /// - `new_folder_id`: Destination folder id, if any.
    /// - `update_req`: Update payload to apply to the paste.
    ///
    /// # Returns
    /// Updated paste if it existed.
    ///
    /// # Errors
    /// Propagates storage errors from paste or folder updates.
    pub fn move_paste_between_folders(
        db: &Database,
        paste_id: &str,
        new_folder_id: Option<&str>,
        update_req: crate::models::paste::UpdatePasteRequest,
    ) -> Result<Option<crate::models::paste::Paste>, AppError> {
        let _guard = Self::acquire_folder_txn_lock(db)?;
        Self::move_paste_between_folders_locked(db, paste_id, new_folder_id, update_req)
    }

    pub(crate) fn move_paste_between_folders_locked(
        db: &Database,
        paste_id: &str,
        new_folder_id: Option<&str>,
        update_req: crate::models::paste::UpdatePasteRequest,
    ) -> Result<Option<crate::models::paste::Paste>, AppError> {
        const MAX_MOVE_RETRIES: usize = 8;

        for _ in 0..MAX_MOVE_RETRIES {
            let current = match db.pastes.get(paste_id)? {
                Some(paste) => paste,
                None => return Ok(None),
            };

            let old_folder_id = current.folder_id.as_deref();
            let folder_changing = old_folder_id != new_folder_id;

            // Reserve the destination count first so we can fail fast if the folder is gone.
            if folder_changing {
                if let Some(new_id) = new_folder_id {
                    ensure_folder_assignable(db, new_id)?;
                    db.folders.update_count(new_id, 1)?;
                }
            }
            #[cfg(test)]
            if let Err(err) =
                apply_move_failpoint_after_destination_reserve(db, folder_changing, new_folder_id)
            {
                rollback_destination_reservation(
                    db,
                    if folder_changing { new_folder_id } else { None },
                    "injected move failpoint",
                    false,
                );
                return Err(err);
            }

            // The destination can disappear between reservation and the paste CAS.
            // Revalidate here so we don't commit a folder_id that no longer exists.
            if folder_changing {
                if let Some(new_id) = new_folder_id {
                    if let Err(err) = ensure_folder_assignable(db, new_id) {
                        rollback_destination_reservation(
                            db,
                            Some(new_id),
                            "destination disappeared before move CAS",
                            true,
                        );
                        return Err(err);
                    }
                }
            }

            let update_result =
                db.pastes
                    .update_if_folder_matches(paste_id, old_folder_id, update_req.clone());
            match update_result {
                Ok(Some(updated)) => {
                    if folder_changing {
                        if let Some(new_id) = new_folder_id {
                            if let Err(err) = ensure_folder_assignable(db, new_id) {
                                let revert_folder_value = old_folder_id
                                    .map(ToString::to_string)
                                    .unwrap_or_else(String::new);
                                let revert_req = crate::models::paste::UpdatePasteRequest {
                                    content: None,
                                    name: None,
                                    language: None,
                                    language_is_manual: None,
                                    folder_id: Some(revert_folder_value),
                                    tags: None,
                                };
                                match db.pastes.update_if_folder_matches(
                                    paste_id,
                                    Some(new_id),
                                    revert_req,
                                ) {
                                    Ok(_) => {}
                                    Err(revert_err) => {
                                        tracing::error!(
                                            "Failed to revert paste folder after destination became unassignable: {}",
                                            revert_err
                                        );
                                    }
                                }
                                rollback_destination_reservation(
                                    db,
                                    Some(new_id),
                                    "post-commit destination check",
                                    true,
                                );
                                // Canonical CAS may have committed before this point. Returning Err
                                // is intentional so callers observe that destination validation
                                // failed and compensating revert/rollback was attempted.
                                return Err(err);
                            }
                        }
                    }

                    if folder_changing {
                        if let Some(old_id) = old_folder_id {
                            if let Err(err) = db.folders.update_count(old_id, -1) {
                                tracing::error!(
                                    "Failed to decrement old folder count after move: {}",
                                    err
                                );
                            }
                        }
                    }
                    return Ok(Some(updated));
                }
                Ok(None) => {
                    // Compare-and-swap mismatch or deletion. Roll back destination reservation.
                    rollback_destination_reservation(
                        db,
                        if folder_changing { new_folder_id } else { None },
                        "move conflict",
                        false,
                    );

                    if db.pastes.get(paste_id)?.is_none() {
                        return Ok(None);
                    }
                }
                Err(err) => {
                    // PasteDb update methods treat metadata-index failures as best-effort and
                    // return success once the canonical row is committed. An error here means
                    // canonical compare-and-swap did not commit, so rolling back reservation is safe.
                    rollback_destination_reservation(
                        db,
                        if folder_changing { new_folder_id } else { None },
                        "move error",
                        false,
                    );
                    return Err(err);
                }
            }
        }

        Err(AppError::StorageMessage(
            "Paste update conflicted repeatedly; please retry.".to_string(),
        ))
    }
}
