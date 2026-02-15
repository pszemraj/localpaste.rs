//! Concurrency and serialization tests for database operations.

use super::*;

#[test]
fn concurrent_moves_keep_folder_counts_consistent() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let folder_a = Folder::new("folder-a".to_string());
    let folder_a_id = folder_a.id.clone();
    db.folders.create(&folder_a).expect("create a");

    let folder_b = Folder::new("folder-b".to_string());
    let folder_b_id = folder_b.id.clone();
    db.folders.create(&folder_b).expect("create b");

    let mut paste = Paste::new("concurrent".to_string(), "move".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create paste");

    let worker_a = db.share().expect("share a");
    let worker_b = db.share().expect("share b");
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
        .expect("move a");
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
        .expect("move b");
    });

    handle_a.join().expect("join a");
    handle_b.join().expect("join b");

    let moved = db
        .pastes
        .get(&paste_id)
        .expect("lookup")
        .expect("paste exists");
    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let a_after = db.folders.get(&folder_a_id).expect("a").expect("row");
    let b_after = db.folders.get(&folder_b_id).expect("b").expect("row");

    assert_eq!(old_after.paste_count, 0);
    assert_eq!(a_after.paste_count + b_after.paste_count, 1);
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
fn concurrent_move_and_delete_keep_folder_counts_consistent() {
    let (db, _temp) = setup_test_db();

    let old_folder = Folder::new("old-folder".to_string());
    let old_folder_id = old_folder.id.clone();
    db.folders.create(&old_folder).expect("create old");

    let new_folder = Folder::new("new-folder".to_string());
    let new_folder_id = new_folder.id.clone();
    db.folders.create(&new_folder).expect("create new");

    let mut paste = Paste::new("concurrent".to_string(), "move-delete".to_string());
    paste.folder_id = Some(old_folder_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &old_folder_id).expect("create paste");

    let mover_db = db.share().expect("share mover");
    let deleter_db = db.share().expect("share deleter");
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
    assert!(move_result.is_ok(), "move error: {:?}", move_result);
    assert!(delete_result.is_ok(), "delete error: {:?}", delete_result);

    let old_after = db.folders.get(&old_folder_id).expect("old").expect("row");
    let new_after = db.folders.get(&new_folder_id).expect("new").expect("row");
    let maybe_paste = db.pastes.get(&paste_id).expect("lookup");

    match maybe_paste {
        Some(paste) => {
            assert_eq!(paste.folder_id.as_deref(), Some(new_folder_id.as_str()));
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
fn concurrent_reconcile_and_move_preserve_invariants() {
    let (db, _temp) = setup_test_db();

    let source = Folder::new("source-folder".to_string());
    let source_id = source.id.clone();
    db.folders.create(&source).expect("create source");

    let destination = Folder::new("destination-folder".to_string());
    let destination_id = destination.id.clone();
    db.folders.create(&destination).expect("create destination");

    let mut paste = Paste::new("content".to_string(), "name".to_string());
    paste.folder_id = Some(source_id.clone());
    let paste_id = paste.id.clone();
    TransactionOps::create_paste_with_folder(&db, &paste, &source_id).expect("create");

    let move_db = db.share().expect("share move");
    let reconcile_db = db.share().expect("share reconcile");
    let barrier = Arc::new(Barrier::new(2));

    let move_barrier = barrier.clone();
    let move_destination = destination_id.clone();
    let move_paste_id = paste_id.clone();
    let mover = thread::spawn(move || {
        move_barrier.wait();
        let update = UpdatePasteRequest {
            content: None,
            name: None,
            language: None,
            language_is_manual: None,
            folder_id: Some(move_destination.clone()),
            tags: None,
        };
        TransactionOps::move_paste_between_folders(
            &move_db,
            &move_paste_id,
            Some(move_destination.as_str()),
            update,
        )
    });

    let reconcile_barrier = barrier;
    let reconciler = thread::spawn(move || {
        reconcile_barrier.wait();
        crate::folder_ops::reconcile_folder_invariants(&reconcile_db)
    });

    let move_result = mover.join().expect("move join");
    let reconcile_result = reconciler.join().expect("reconcile join");
    assert!(move_result.is_ok(), "move result: {:?}", move_result);
    assert!(
        reconcile_result.is_ok(),
        "reconcile result: {:?}",
        reconcile_result
    );

    let current = db
        .pastes
        .get(&paste_id)
        .expect("lookup")
        .expect("paste exists");
    if let Some(folder_id) = current.folder_id.as_deref() {
        assert!(
            db.folders.get(folder_id).expect("folder lookup").is_some(),
            "paste must reference an existing folder"
        );
    }
    crate::test_support::assert_folder_counts_match_canonical(&db);
}
