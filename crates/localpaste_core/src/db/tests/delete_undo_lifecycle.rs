//! Deleted-paste undo lifecycle tests.

use super::*;

#[test]
fn expired_restore_consumes_all_staged_rows_without_restoring_paste() {
    let (db, _temp) = setup_test_db();
    let paste_id = seed_versioned_paste(&db, "expired restore");
    let token = "expired-restore-token";

    assert!(
        TransactionOps::delete_paste_with_folder_staged_undo(&db, &paste_id, token, 0,)
            .expect("stage expired delete undo")
    );
    assert_staged_undo_token(&db, token, true);
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("lookup after staged delete")
        .is_none());

    let restored = TransactionOps::restore_deleted_paste_by_token(&db, token)
        .expect("expired restore should be handled without storage failure");
    assert!(restored.is_none());
    assert_staged_undo_token(&db, token, false);
    assert!(db
        .pastes
        .get(&paste_id)
        .expect("lookup after expired restore")
        .is_none());
}
