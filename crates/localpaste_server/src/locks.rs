//! In-memory paste edit locks shared between GUI and API handlers.

use std::collections::HashMap;
use std::sync::Mutex;

/// Error returned when an operation requires an unlocked paste id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockCheckError {
    Locked,
}

/// Tracks which paste ids are currently open for editing.
#[derive(Default)]
pub struct PasteLockManager {
    // Ref-count by paste id so independent lock holders do not clear each other.
    inner: Mutex<HashMap<String, usize>>,
}

impl PasteLockManager {
    /// Mark a paste as locked for editing.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn lock(&self, id: &str) {
        let mut guard = self.inner.lock().expect("paste lock manager poisoned");
        *guard.entry(id.to_string()).or_insert(0) += 1;
    }

    /// Remove a paste lock.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn unlock(&self, id: &str) {
        let mut guard = self.inner.lock().expect("paste lock manager poisoned");
        let Some(count) = guard.get_mut(id) else {
            return;
        };
        if *count > 1 {
            *count -= 1;
        } else {
            guard.remove(id);
        }
    }

    /// Check if a paste is currently locked.
    ///
    /// # Returns
    /// `true` if the paste id is locked for editing.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn is_locked(&self, id: &str) -> bool {
        let guard = self.inner.lock().expect("paste lock manager poisoned");
        guard.contains_key(id)
    }

    /// Execute `operation` only when `id` is currently unlocked.
    ///
    /// The lock-manager mutex is held for the duration of `operation`, making
    /// the unlocked-check and operation atomic with respect to lock/unlock calls.
    ///
    /// # Arguments
    /// - `id`: Paste identifier that must be unlocked for the operation to run.
    /// - `operation`: Closure executed while the lock-manager mutex is held.
    ///
    /// # Returns
    /// - `Ok(T)` when `operation` ran.
    /// - `Err(LockCheckError::Locked)` when `id` was already locked.
    ///
    /// # Errors
    /// Returns [`LockCheckError::Locked`] when `id` is already locked.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn with_unlocked<T, F>(&self, id: &str, operation: F) -> Result<T, LockCheckError>
    where
        F: FnOnce() -> T,
    {
        let guard = self.inner.lock().expect("paste lock manager poisoned");
        if guard.contains_key(id) {
            return Err(LockCheckError::Locked);
        }
        let result = operation();
        drop(guard);
        Ok(result)
    }

    /// Execute `operation` while blocking lock/unlock transitions.
    ///
    /// This helper snapshots locked ids and runs `operation` while the lock-table
    /// mutex remains held. Use it for lock-sensitive operations that must stay
    /// atomic with respect to lock/unlock calls.
    ///
    /// # Arguments
    /// - `operation`: Closure receiving the current locked id snapshot.
    ///
    /// # Returns
    /// The closure return value.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn with_locked_ids<T, F>(&self, operation: F) -> T
    where
        F: FnOnce(Vec<String>) -> T,
    {
        let guard = self.inner.lock().expect("paste lock manager poisoned");
        let ids = guard.keys().cloned().collect();
        let result = operation(ids);
        drop(guard);
        result
    }

    /// Snapshot all currently locked paste ids.
    ///
    /// # Returns
    /// A cloned list of locked paste identifiers.
    ///
    /// # Panics
    /// Panics if the internal lock mutex is poisoned.
    pub fn locked_ids(&self) -> Vec<String> {
        let guard = self.inner.lock().expect("paste lock manager poisoned");
        guard.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::PasteLockManager;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Barrier,
    };
    use std::thread;
    use std::time::Duration;

    #[test]
    fn with_unlocked_runs_operation_when_id_is_unlocked() {
        let locks = PasteLockManager::default();
        let ran = locks.with_unlocked("alpha", || 42).expect("should run");
        assert_eq!(ran, 42);
    }

    #[test]
    fn with_unlocked_rejects_operation_when_id_is_locked() {
        let locks = PasteLockManager::default();
        locks.lock("alpha");
        let result = locks.with_unlocked("alpha", || "never-run");
        assert!(result.is_err(), "locked id should reject operation");
    }

    #[test]
    fn with_unlocked_holds_mutex_for_operation_duration() {
        let locks = Arc::new(PasteLockManager::default());
        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));

        let worker_locks = Arc::clone(&locks);
        let worker_entered = Arc::clone(&entered);
        let worker_release = Arc::clone(&release);
        let worker = thread::spawn(move || {
            worker_locks
                .with_unlocked("alpha", || {
                    worker_entered.wait();
                    worker_release.wait();
                })
                .expect("operation should run while unlocked");
        });

        entered.wait();

        let locker_locks = Arc::clone(&locks);
        let lock_completed = Arc::new(AtomicBool::new(false));
        let lock_completed_worker = Arc::clone(&lock_completed);
        let locker = thread::spawn(move || {
            locker_locks.lock("alpha");
            lock_completed_worker.store(true, Ordering::SeqCst);
        });

        thread::sleep(Duration::from_millis(50));
        assert!(
            !lock_completed.load(Ordering::SeqCst),
            "lock() should block while with_unlocked closure is executing"
        );

        release.wait();
        worker.join().expect("worker join");
        locker.join().expect("locker join");

        assert!(lock_completed.load(Ordering::SeqCst));
        assert!(locks.is_locked("alpha"));
    }

    #[test]
    fn lock_is_reference_counted_per_paste_id() {
        let locks = PasteLockManager::default();
        locks.lock("alpha");
        locks.lock("alpha");

        locks.unlock("alpha");
        assert!(
            locks.is_locked("alpha"),
            "one unlock should not clear another active holder"
        );

        locks.unlock("alpha");
        assert!(
            !locks.is_locked("alpha"),
            "lock should clear after all holders release"
        );
    }
}
