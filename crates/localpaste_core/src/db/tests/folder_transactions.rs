//! Folder transaction and rollback tests.

use super::*;

fn assert_create_with_folder_rejects_deleted_destination(failpoint: TransactionFailpoint) {
    let _lock = transaction_failpoint_test_lock()
        .lock()
        .expect("transaction failpoint lock");
    let _guard = FailpointGuard;
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("target-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).unwrap();

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();

    set_transaction_failpoint(Some(failpoint));
    let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
    set_transaction_failpoint(None);

    assert!(
        matches!(result, Err(AppError::NotFound)),
        "deleted destination must reject create"
    );
    assert!(
        db.pastes.get(&paste_id).unwrap().is_none(),
        "failed create must not leave an orphan canonical row"
    );
    assert!(
        db.folders.get(&folder_id).unwrap().is_none(),
        "injected race removes destination folder"
    );
}

fn run_move_between_folders_failpoint(
    failpoint: TransactionFailpoint,
) -> (
    Database,
    String,
    String,
    String,
    Result<Option<Paste>, AppError>,
) {
    let _lock = transaction_failpoint_test_lock()
        .lock()
        .expect("transaction failpoint lock");
    let _guard = FailpointGuard;
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).unwrap();

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };

    set_transaction_failpoint(Some(failpoint));
    let result = TransactionOps::move_paste_between_folders(
        &db,
        &paste_id,
        Some(new_folder_id.as_str()),
        update,
    );
    set_transaction_failpoint(None);

    (db, paste_id, old_folder_id, new_folder_id, result)
}

#[derive(Clone, Copy)]
enum MoveFailpointExpectation {
    InjectedError,
    DestinationDeleted,
}

#[test]
fn test_move_between_folders_rolls_back_counts_when_paste_missing() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).unwrap();

    db.folders.update_count(&old_folder_id, 1).unwrap();
    db.folders.update_count(&new_folder_id, 1).unwrap();

    let update = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };

    let result = TransactionOps::move_paste_between_folders(
        &db,
        "missing-paste-id",
        Some(new_folder_id.as_str()),
        update,
    )
    .unwrap();

    assert!(result.is_none(), "missing paste should return None");

    let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
    let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
    assert_eq!(
        old_after.paste_count, 1,
        "old folder count should rollback when paste update returns None"
    );
    assert_eq!(
        new_after.paste_count, 1,
        "new folder count should rollback when paste update returns None"
    );
}

#[test]
fn test_move_between_folders_failpoint_matrix_preserves_assignment_and_counts() {
    let cases = [
        (
            TransactionFailpoint::MoveAfterDestinationReserveOnce,
            MoveFailpointExpectation::InjectedError,
        ),
        (
            TransactionFailpoint::MoveDeleteDestinationAfterReserveOnce,
            MoveFailpointExpectation::DestinationDeleted,
        ),
    ];

    for (failpoint, expectation) in cases {
        let (db, paste_id, old_folder_id, new_folder_id, result) =
            run_move_between_folders_failpoint(failpoint);

        match expectation {
            MoveFailpointExpectation::InjectedError => {
                assert!(
                    matches!(result, Err(AppError::StorageMessage(message)) if message.contains("Injected transaction failpoint")),
                    "failpoint should force a deterministic move error"
                );
            }
            MoveFailpointExpectation::DestinationDeleted => {
                assert!(
                    matches!(result, Err(AppError::NotFound)),
                    "destination deletion during move must reject the move"
                );
            }
        }

        let paste_after = db
            .pastes
            .get(&paste_id)
            .expect("paste lookup")
            .expect("paste exists");
        assert_eq!(
            paste_after.folder_id.as_deref(),
            Some(old_folder_id.as_str()),
            "failed move must preserve canonical folder assignment"
        );

        let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
        assert_eq!(
            old_after.paste_count, 1,
            "old folder count should remain unchanged after failed move"
        );
        match expectation {
            MoveFailpointExpectation::InjectedError => {
                let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
                assert_eq!(
                    new_after.paste_count, 0,
                    "destination reservation must rollback on move error"
                );
            }
            MoveFailpointExpectation::DestinationDeleted => {
                assert!(
                    db.folders.get(&new_folder_id).unwrap().is_none(),
                    "injected race removes destination folder"
                );
            }
        }
    }
}

#[test]
fn test_move_and_folder_delete_are_linearized_after_destination_reserve() {
    let _lock = transaction_failpoint_test_lock()
        .lock()
        .expect("transaction failpoint lock");
    let _guard = FailpointGuard;
    let (db, _temp) = setup_test_db();

    let source = Folder::new("source-folder".to_string());
    let source_id = source.id.clone();
    db.folders.create(&source).unwrap();

    let destination = Folder::new("destination-folder".to_string());
    let destination_id = destination.id.clone();
    db.folders.create(&destination).unwrap();

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(source_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &source_id).unwrap();

    let reached = Arc::new(Barrier::new(2));
    let resume = Arc::new(Barrier::new(2));
    set_move_pause_hooks(Some(MovePauseHooks {
        reached: reached.clone(),
        resume: resume.clone(),
    }));

    let mover_db = db.share().expect("share db");
    let mover_paste_id = paste_id.clone();
    let mover_destination = destination_id.clone();
    let mover = thread::spawn(move || {
        set_transaction_failpoint(Some(
            TransactionFailpoint::MovePauseAfterDestinationReserveOnce,
        ));
        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(mover_destination.clone()),
            tags: None,
        };
        let result = TransactionOps::move_paste_between_folders(
            &mover_db,
            &mover_paste_id,
            Some(mover_destination.as_str()),
            update,
        );
        set_transaction_failpoint(None);
        result
    });

    // Ensure the move paused after destination reservation and before CAS.
    reached.wait();

    let deleter_db = db.share().expect("share db");
    let deleter_destination = destination_id.clone();
    let deleter = thread::spawn(move || {
        crate::folder_ops::delete_folder_tree_and_migrate(&deleter_db, &deleter_destination)
    });

    // Allow the move to resume and release the transaction lock.
    resume.wait();

    let move_result = mover.join().expect("mover join");
    set_move_pause_hooks(None);
    assert!(
        move_result.is_ok(),
        "move should succeed under lock serialization: {:?}",
        move_result
    );

    let delete_result = deleter.join().expect("deleter join");
    assert!(
        delete_result.is_ok(),
        "delete should succeed after move completes: {:?}",
        delete_result
    );

    let final_paste = db
        .pastes
        .get(&paste_id)
        .expect("paste lookup")
        .expect("paste should remain");
    assert_eq!(
        final_paste.folder_id, None,
        "folder delete should migrate moved paste to unfiled after serialization"
    );
    assert!(
        db.folders.get(&destination_id).unwrap().is_none(),
        "destination folder should be deleted"
    );
}

#[test]
fn test_create_with_folder_injected_error_rolls_back_reservation_and_leaves_no_paste() {
    let _lock = transaction_failpoint_test_lock()
        .lock()
        .expect("transaction failpoint lock");
    let _guard = FailpointGuard;
    let (db, _temp) = setup_test_db();

    let folder = Folder::new("target-folder".to_string());
    let folder_id = folder.id.clone();
    db.folders.create(&folder).unwrap();

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(folder_id.clone());
    let paste_id = paste.id.clone();

    set_transaction_failpoint(Some(
        TransactionFailpoint::CreateAfterDestinationReserveOnce,
    ));
    let result = TransactionOps::create_paste_with_folder(&db, &paste, &folder_id);
    set_transaction_failpoint(None);

    assert!(
        matches!(result, Err(AppError::StorageMessage(message)) if message.contains("CreateAfterDestinationReserveOnce")),
        "injected create failpoint should surface error"
    );
    assert!(
        db.pastes.get(&paste_id).unwrap().is_none(),
        "create error must not leave canonical paste row"
    );
    let folder_after = db.folders.get(&folder_id).unwrap().unwrap();
    assert_eq!(
        folder_after.paste_count, 0,
        "destination reservation must rollback on create error"
    );
}

#[test]
fn test_create_with_folder_rejects_destination_deleted_after_reservation_without_orphan() {
    assert_create_with_folder_rejects_deleted_destination(
        TransactionFailpoint::CreateDeleteDestinationAfterReserveOnce,
    );
}

#[test]
fn test_create_with_folder_destination_deleted_after_create_rolls_back_canonical_insert() {
    assert_create_with_folder_rejects_deleted_destination(
        TransactionFailpoint::CreateDeleteDestinationAfterCanonicalCreateOnce,
    );
}

#[test]
fn test_create_with_folder_duplicate_id_rolls_back_destination_reservation() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).unwrap();

    let mut existing = Paste::new("original".to_string(), "name".to_string());
    existing.folder_id = Some(old_folder_id.clone());
    let paste_id = existing.id.clone();
    TransactionOps::create_paste_with_folder(&db, &existing, &old_folder_id).unwrap();

    let mut duplicate = Paste::new("conflicting".to_string(), "name".to_string());
    duplicate.id = paste_id.clone();
    duplicate.folder_id = Some(new_folder_id.clone());

    let result = TransactionOps::create_paste_with_folder(&db, &duplicate, &new_folder_id);
    assert!(
        matches!(result, Err(AppError::StorageMessage(ref message)) if message.contains("already exists")),
        "duplicate id create should fail without count drift: {:?}",
        result
    );

    let stored = db
        .pastes
        .get(&paste_id)
        .expect("lookup")
        .expect("existing paste");
    assert_eq!(
        stored.folder_id.as_deref(),
        Some(old_folder_id.as_str()),
        "duplicate create must not reassign canonical folder"
    );
    assert_eq!(stored.content, "original");

    let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
    let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
    assert_eq!(old_after.paste_count, 1);
    assert_eq!(
        new_after.paste_count, 0,
        "destination reservation must rollback on duplicate create"
    );
}

#[test]
fn test_delete_uses_folder_from_deleted_record_not_stale_context() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).unwrap();

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

    let stale_folder_id = paste.folder_id.clone().unwrap();
    assert_eq!(stale_folder_id, old_folder_id);

    let move_req = UpdatePasteRequest {
        content: None,
        name: None,
        language: None,
        language_is_manual: None,
        folder_id: Some(new_folder_id.clone()),
        tags: None,
    };
    TransactionOps::move_paste_between_folders(
        &db,
        &paste_id,
        Some(new_folder_id.as_str()),
        move_req,
    )
    .unwrap()
    .expect("paste should exist when moving");

    let deleted = TransactionOps::delete_paste_with_folder(&db, &paste_id).unwrap();
    assert!(deleted, "delete should remove existing paste");

    let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
    let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
    assert_eq!(
        old_after.paste_count, 0,
        "stale folder should not be decremented twice"
    );
    assert_eq!(
        new_after.paste_count, 0,
        "current folder should be decremented on delete"
    );
}
