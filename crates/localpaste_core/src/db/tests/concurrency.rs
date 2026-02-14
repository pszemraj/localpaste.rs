//! Concurrency and serialization tests for database operations.

use super::*;

#[test]
fn test_concurrent_moves_keep_folder_counts_consistent() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let folder_a = Folder::new("folder-a".to_string());
    let folder_a_id = folder_a.id.clone();
    db.folders.create(&folder_a).unwrap();

    let folder_b = Folder::new("folder-b".to_string());
    let folder_b_id = folder_b.id.clone();
    db.folders.create(&folder_b).unwrap();

    let mut paste = Paste::new("concurrent".to_string(), "move".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

    let worker_a = db.share().unwrap();
    let worker_b = db.share().unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let paste_for_a = paste_id.clone();
    let folder_for_a = folder_a_id.clone();
    let barrier_a = barrier.clone();
    let handle_a = thread::spawn(move || {
        barrier_a.wait();
        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(folder_for_a.clone()),
            tags: None,
        };
        TransactionOps::move_paste_between_folders(
            &worker_a,
            &paste_for_a,
            Some(folder_for_a.as_str()),
            update,
        )
        .expect("move to folder-a should not fail");
    });

    let paste_for_b = paste_id.clone();
    let folder_for_b = folder_b_id.clone();
    let barrier_b = barrier;
    let handle_b = thread::spawn(move || {
        barrier_b.wait();
        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(folder_for_b.clone()),
            tags: None,
        };
        TransactionOps::move_paste_between_folders(
            &worker_b,
            &paste_for_b,
            Some(folder_for_b.as_str()),
            update,
        )
        .expect("move to folder-b should not fail");
    });

    handle_a.join().expect("worker-a join");
    handle_b.join().expect("worker-b join");

    let moved = db
        .pastes
        .get(&paste_id)
        .unwrap()
        .expect("paste should exist");
    let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
    let a_after = db.folders.get(&folder_a_id).unwrap().unwrap();
    let b_after = db.folders.get(&folder_b_id).unwrap().unwrap();

    assert_eq!(
        old_after.paste_count, 0,
        "old folder should be empty after concurrent moves"
    );
    assert_eq!(
        a_after.paste_count + b_after.paste_count,
        1,
        "exactly one destination folder should own the paste"
    );
    match moved.folder_id.as_deref() {
        Some(fid) if fid == folder_a_id.as_str() => {
            assert_eq!(a_after.paste_count, 1);
            assert_eq!(b_after.paste_count, 0);
        }
        Some(fid) if fid == folder_b_id.as_str() => {
            assert_eq!(a_after.paste_count, 0);
            assert_eq!(b_after.paste_count, 1);
        }
        other => panic!("paste moved to unexpected folder: {:?}", other),
    }
}

#[test]
fn test_concurrent_move_and_delete_keep_folder_counts_consistent() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).unwrap();

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).unwrap();

    let mut paste = Paste::new("concurrent".to_string(), "move-delete".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).unwrap();

    let mover_db = db.share().unwrap();
    let deleter_db = db.share().unwrap();
    let barrier = Arc::new(Barrier::new(2));

    let mover_barrier = barrier.clone();
    let mover_paste_id = paste_id.clone();
    let mover_folder_id = new_folder_id.clone();
    let mover = thread::spawn(move || {
        mover_barrier.wait();
        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(mover_folder_id.clone()),
            tags: None,
        };
        TransactionOps::move_paste_between_folders(
            &mover_db,
            &mover_paste_id,
            Some(mover_folder_id.as_str()),
            update,
        )
    });

    let deleter_barrier = barrier;
    let deleter_paste_id = paste_id.clone();
    let deleter = thread::spawn(move || {
        deleter_barrier.wait();
        TransactionOps::delete_paste_with_folder(&deleter_db, &deleter_paste_id)
    });

    let move_result = mover.join().expect("mover join");
    let delete_result = deleter.join().expect("deleter join");
    assert!(
        move_result.is_ok(),
        "move should not error: {:?}",
        move_result
    );
    assert!(
        delete_result.is_ok(),
        "delete should not error: {:?}",
        delete_result
    );

    let old_after = db.folders.get(&old_folder_id).unwrap().unwrap();
    let new_after = db.folders.get(&new_folder_id).unwrap().unwrap();
    let maybe_paste = db.pastes.get(&paste_id).unwrap();

    match maybe_paste {
        Some(paste) => {
            assert_eq!(
                paste.folder_id.as_deref(),
                Some(new_folder_id.as_str()),
                "remaining paste should only exist in destination folder"
            );
            assert_eq!(old_after.paste_count, 0);
            assert_eq!(new_after.paste_count, 1);
        }
        None => {
            assert_eq!(old_after.paste_count, 0);
            assert_eq!(new_after.paste_count, 0);
        }
    }
}

#[test]
fn test_reconcile_folder_invariants_and_move_are_linearized() {
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

    // Move is paused while holding the folder transaction lock.
    reached.wait();

    let reconcile_db = db.share().expect("share db");
    let reconciler =
        thread::spawn(move || crate::folder_ops::reconcile_folder_invariants(&reconcile_db));

    // Allow mover to finish and release the lock so reconcile can proceed.
    resume.wait();

    let move_result = mover.join().expect("mover join");
    set_move_pause_hooks(None);
    assert!(
        matches!(move_result, Ok(Some(_))),
        "move should commit successfully: {:?}",
        move_result
    );

    let reconcile_result = reconciler.join().expect("reconciler join");
    assert!(
        reconcile_result.is_ok(),
        "reconcile should succeed after lock handoff: {:?}",
        reconcile_result
    );

    let current = db
        .pastes
        .get(&paste_id)
        .expect("paste lookup")
        .expect("paste exists");
    assert_eq!(
        current.folder_id.as_deref(),
        Some(destination_id.as_str()),
        "final folder assignment should remain valid after serialized reconcile"
    );
    crate::test_support::assert_folder_counts_match_canonical(&db);
}
