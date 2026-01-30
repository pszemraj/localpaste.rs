//! In-memory paste edit locks shared between GUI and API handlers.

use std::collections::HashSet;
use std::sync::Mutex;

/// Tracks which paste ids are currently open for editing.
#[derive(Default)]
pub struct PasteLockManager {
    inner: Mutex<HashSet<String>>,
}

impl PasteLockManager {
    /// Mark a paste as locked for editing.
    pub fn lock(&self, id: &str) {
        let mut guard = self.inner.lock().expect("paste lock manager poisoned");
        guard.insert(id.to_string());
    }

    /// Remove a paste lock.
    pub fn unlock(&self, id: &str) {
        let mut guard = self.inner.lock().expect("paste lock manager poisoned");
        guard.remove(id);
    }

    /// Check if a paste is currently locked.
    pub fn is_locked(&self, id: &str) -> bool {
        let guard = self.inner.lock().expect("paste lock manager poisoned");
        guard.contains(id)
    }
}
