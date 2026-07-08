//! Shared harness helpers for GUI/backend headless integration tests.

use crossbeam_channel::Receiver;
use localpaste_core::{Config, Database};
use localpaste_gui::backend::{spawn_backend_with_locks, BackendHandle, CoreEvent};
use localpaste_server::{AppState, EmbeddedServer, PasteLockManager};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Max paste size used by headless GUI/backend integration tests.
pub const TEST_MAX_PASTE_SIZE: usize = 10 * 1024 * 1024;

/// Receives the next backend event with the standard integration-test timeout.
///
/// # Returns
/// The next backend event.
///
/// # Panics
/// Panics if no event arrives before the timeout.
pub fn recv_event(rx: &Receiver<CoreEvent>) -> CoreEvent {
    rx.recv_timeout(Duration::from_secs(2))
        .expect("expected backend event")
}

fn test_config(db_path: &str) -> Config {
    Config {
        db_path: db_path.to_string(),
        port: 0,
        max_paste_size: TEST_MAX_PASTE_SIZE,
        auto_save_interval: 2000,
        auto_backup: false,
        search_case_sensitive: false,
    }
}

/// Owns the temporary database and shared database handle for headless GUI/backend tests.
pub struct TestEnv {
    _dir: TempDir,
    db_path: String,
    /// Database handle used by tests that need direct storage assertions.
    pub db: Database,
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl TestEnv {
    /// Creates an isolated temporary database for a test.
    ///
    /// # Returns
    /// A test environment owning a temporary database directory.
    ///
    /// # Panics
    /// Panics if the temporary directory or database cannot be created.
    pub fn new() -> Self {
        let dir = TempDir::new().expect("temp dir");
        let db_path = dir.path().join("db");
        let db_path_str = db_path.to_string_lossy().to_string();
        let db = Database::new(&db_path_str).expect("db");
        Self {
            _dir: dir,
            db_path: db_path_str,
            db,
        }
    }

    /// Starts an embedded API server against this test database and lock manager.
    ///
    /// # Returns
    /// A running embedded server bound to an ephemeral port.
    ///
    /// # Panics
    /// Panics if the database cannot be shared or the embedded server cannot start.
    pub fn start_server(&self, locks: Arc<PasteLockManager>) -> EmbeddedServer {
        let state = AppState::with_locks(
            test_config(&self.db_path),
            self.db.share().expect("share db"),
            locks,
        );
        EmbeddedServer::start(state, false).expect("server")
    }

    /// Starts a backend worker with a default lock manager.
    ///
    /// # Returns
    /// A backend worker handle connected to this test database.
    pub fn spawn_backend(&self) -> BackendHandle {
        self.spawn_backend_with_locks(Arc::new(PasteLockManager::default()))
    }

    /// Starts a backend worker using the supplied lock manager.
    ///
    /// # Returns
    /// A backend worker handle connected to this test database.
    ///
    /// # Panics
    /// Panics if the database cannot be shared.
    pub fn spawn_backend_with_locks(&self, locks: Arc<PasteLockManager>) -> BackendHandle {
        spawn_backend_with_locks(
            self.db.share().expect("share db"),
            TEST_MAX_PASTE_SIZE,
            locks,
        )
    }
}
