//! Metadata saves racing a content save whose ack resolves the paste language.

use super::*;

const PYTHON_SOURCE: &str =
    "import os\n\ndef main():\n    print(os.getcwd())\n\nif __name__ == \"__main__\":\n    main()\n";

/// A harness editing an auto-language paste stored behind a real backend worker.
///
/// The harness keeps its command channel, so each test forwards commands and
/// applies replies in the order the race it covers needs.
struct LanguageSyncFixture {
    harness: TestHarness,
    backend: BackendHandle,
    db_path: String,
    _dir: TempDir,
}

impl LanguageSyncFixture {
    fn new() -> Self {
        let mut harness = make_app();
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db").to_string_lossy().into_owned();
        let db = Database::new(&db_path).expect("db");
        let mut paste = Paste::new_with_language(String::new(), "Untitled".into(), None, false);
        paste.id = "fresh".into();
        db.pastes.create(&paste).expect("create paste");
        let backend = crate::backend::spawn_backend_with_locks_and_owner(
            db,
            10 * 1024 * 1024,
            harness.app.locks.clone(),
            harness.app.lock_owner_id.clone(),
        );
        assert!(harness.app.acquire_paste_lock("alpha"));
        harness.app.all_pastes = vec![test_summary("fresh", "Untitled", None, 0)];
        harness.app.pastes = harness.app.all_pastes.clone().into();
        harness.app.select_loaded_paste(paste);
        Self {
            harness,
            backend,
            db_path,
            _dir: dir,
        }
    }

    /// Sends the next app command to the worker and returns the worker's reply.
    fn forward(&mut self) -> CoreEvent {
        self.backend
            .cmd_tx
            .send(recv_cmd(&self.harness.cmd_rx))
            .expect("forward command");
        self.backend
            .evt_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("backend reply")
    }

    /// Types Python into the editor and dispatches its content save.
    fn save_python_content(&mut self) {
        let app = &mut self.harness.app;
        set_active_content(app, PYTHON_SOURCE);
        app.mark_dirty();
        app.save_now();
        assert!(app.save_in_flight);
    }

    /// Types a title, which dirties metadata as the title field does.
    fn type_title(&mut self, title: &str) {
        self.harness.app.edit_name = title.to_string();
        self.harness.app.metadata_dirty = true;
    }

    /// Stops the worker and reads the paste back from a reopened store.
    fn persisted(self) -> Paste {
        let Self {
            harness,
            mut backend,
            db_path,
            _dir,
        } = self;
        backend
            .shutdown_and_join(true, Duration::from_secs(5))
            .expect("backend shutdown");
        drop(backend);
        drop(harness);
        let reopened = Database::new(&db_path).expect("reopen db");
        reopened
            .pastes
            .get("fresh")
            .expect("read paste")
            .expect("paste exists")
    }
}

fn assert_python_ack(event: &CoreEvent) {
    match event {
        CoreEvent::PasteSaved { paste } => {
            assert_eq!(paste.language.as_deref(), Some("python"));
            assert!(paste.language_is_manual, "detection locks the language");
        }
        other => panic!("expected PasteSaved, got {other:?}"),
    }
}

fn sent_language_fields(cmd: &CoreCmd) -> (Option<String>, Option<bool>) {
    match cmd {
        CoreCmd::UpdatePasteMeta {
            language,
            language_is_manual,
            ..
        } => (language.clone(), *language_is_manual),
        other => panic!("expected UpdatePasteMeta, got {other:?}"),
    }
}

#[test]
fn title_commit_after_a_content_ack_keeps_the_detected_language() {
    let mut fixture = LanguageSyncFixture::new();
    fixture.save_python_content();
    let content_ack = fixture.forward();
    assert_python_ack(&content_ack);

    // The title is being typed when the content ack arrives.
    fixture.type_title("Script");
    fixture.harness.app.apply_event(content_ack);
    let app = &fixture.harness.app;
    assert_eq!(app.edit_language.as_deref(), Some("python"));
    assert!(app.edit_language_is_manual);
    assert_eq!(app.edit_name, "Script", "the typed title stays");
    assert!(app.metadata_dirty);

    fixture.harness.app.save_metadata_now();
    let meta_ack = fixture.forward();
    fixture.harness.app.apply_event(meta_ack);
    let app = &fixture.harness.app;
    assert!(!app.metadata_dirty);
    assert_eq!(app.edit_language.as_deref(), Some("python"));

    let stored = fixture.persisted();
    assert_eq!(stored.name, "Script");
    assert_eq!(stored.language.as_deref(), Some("python"));
    assert!(stored.language_is_manual);
}

#[test]
fn title_commit_before_a_content_ack_keeps_the_detected_language() {
    let mut fixture = LanguageSyncFixture::new();
    fixture.save_python_content();
    let content_cmd = recv_cmd(&fixture.harness.cmd_rx);

    fixture.type_title("Script");
    fixture.harness.app.save_metadata_now();
    let meta_cmd = recv_cmd(&fixture.harness.cmd_rx);
    assert_eq!(
        sent_language_fields(&meta_cmd),
        (None, None),
        "an unchanged language is not sent"
    );

    // The worker answers in command order: content first, then metadata.
    for cmd in [content_cmd, meta_cmd] {
        fixture.backend.cmd_tx.send(cmd).expect("forward command");
    }
    let receive = || {
        fixture
            .backend
            .evt_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("backend reply")
    };
    let content_ack = receive();
    let meta_ack = receive();
    assert_python_ack(&content_ack);
    fixture.harness.app.apply_event(content_ack);
    fixture.harness.app.apply_event(meta_ack);
    let app = &fixture.harness.app;
    assert!(
        !app.metadata_dirty,
        "the saved draft leaves nothing to apply"
    );
    assert_eq!(app.edit_name, "Script");
    assert_eq!(app.edit_language.as_deref(), Some("python"));

    let stored = fixture.persisted();
    assert_eq!(stored.name, "Script");
    assert_eq!(stored.language.as_deref(), Some("python"));
    assert!(stored.language_is_manual);
}

#[test]
fn a_language_chosen_before_the_content_ack_still_saves() {
    let mut fixture = LanguageSyncFixture::new();
    fixture.save_python_content();
    let content_ack = fixture.forward();

    let app = &mut fixture.harness.app;
    app.edit_language = Some("rust".to_string());
    app.edit_language_is_manual = true;
    app.metadata_dirty = true;
    app.apply_event(content_ack);
    assert_eq!(
        app.edit_language.as_deref(),
        Some("rust"),
        "a staged choice is not replaced"
    );

    app.save_metadata_now();
    let meta_ack = fixture.forward();
    fixture.harness.app.apply_event(meta_ack);
    assert!(!fixture.harness.app.metadata_dirty);

    let stored = fixture.persisted();
    assert_eq!(stored.language.as_deref(), Some("rust"));
    assert!(stored.language_is_manual);
}

#[test]
fn switching_a_detected_language_back_to_auto_still_clears_it() {
    let mut fixture = LanguageSyncFixture::new();
    fixture.save_python_content();
    let content_ack = fixture.forward();
    fixture.harness.app.apply_event(content_ack);
    assert_eq!(fixture.harness.app.edit_language.as_deref(), Some("python"));

    let app = &mut fixture.harness.app;
    app.edit_language = None;
    app.edit_language_is_manual = false;
    app.metadata_dirty = true;
    app.save_metadata_now();
    let meta_ack = fixture.forward();
    fixture.harness.app.apply_event(meta_ack);

    let stored = fixture.persisted();
    assert_eq!(stored.language, None);
    assert!(!stored.language_is_manual);
}
