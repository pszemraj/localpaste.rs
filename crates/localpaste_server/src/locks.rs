//! In-memory paste edit locks shared between GUI and API handlers.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Mutex, MutexGuard};

/// Stable owner id used to scope edit locks to a specific client/session.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LockOwnerId(String);

impl LockOwnerId {
    /// Construct an owner id from a caller-provided identifier.
    ///
    /// # Returns
    /// A new [`LockOwnerId`] wrapping the provided identifier.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Return this owner id as a string slice.
    ///
    /// # Returns
    /// The underlying owner id as `&str`.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for LockOwnerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Lock-manager runtime errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PasteLockError {
    /// A paste is currently held by one or more owners.
    Held { paste_id: String },
    /// A paste is currently under an active mutation guard.
    Mutating { paste_id: String },
    /// Release attempted by an owner that does not hold the paste.
    NotHeld {
        paste_id: String,
        owner_id: LockOwnerId,
    },
    /// Internal mutex state is poisoned.
    Poisoned,
}

impl fmt::Display for PasteLockError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Held { paste_id } => write!(f, "paste '{paste_id}' is currently locked"),
            Self::Mutating { paste_id } => {
                write!(f, "paste '{paste_id}' is currently being mutated")
            }
            Self::NotHeld { paste_id, owner_id } => write!(
                f,
                "owner '{owner_id}' does not hold lock for paste '{paste_id}'"
            ),
            Self::Poisoned => write!(f, "paste lock manager state is poisoned"),
        }
    }
}

impl std::error::Error for PasteLockError {}

#[derive(Default)]
struct LockState {
    holders_by_paste: HashMap<String, HashSet<LockOwnerId>>,
    mutating_pastes: HashSet<String>,
}

/// Tracks lock holders and in-flight mutation guards for paste ids.
#[derive(Default)]
pub struct PasteLockManager {
    inner: Mutex<LockState>,
}

/// Guard marking one or more paste ids as under mutation.
///
/// While this guard is alive, lock acquisition on the guarded ids is rejected.
pub struct PasteMutationGuard<'a> {
    manager: &'a PasteLockManager,
    paste_ids: Vec<String>,
}

impl Drop for PasteMutationGuard<'_> {
    fn drop(&mut self) {
        match self.manager.inner.lock() {
            Ok(mut state) => {
                for paste_id in &self.paste_ids {
                    state.mutating_pastes.remove(paste_id);
                }
            }
            Err(_) => {
                tracing::error!(
                    "Failed to clear mutation guard for {:?}: lock manager poisoned",
                    self.paste_ids
                );
            }
        }
    }
}

impl PasteLockManager {
    fn state(&self) -> Result<MutexGuard<'_, LockState>, PasteLockError> {
        self.inner.lock().map_err(|_| PasteLockError::Poisoned)
    }

    /// Acquire an edit lock for `paste_id` on behalf of `owner_id`.
    ///
    /// Acquisition is idempotent for the same owner and paste.
    ///
    /// # Arguments
    /// - `paste_id`: Target paste id to lock.
    /// - `owner_id`: Caller/session owner id.
    ///
    /// # Returns
    /// `Ok(())` when the lock is acquired (or already held by `owner_id`).
    ///
    /// # Errors
    /// Returns [`PasteLockError::Mutating`] when `paste_id` is currently under
    /// mutation, or [`PasteLockError::Poisoned`] when lock state is poisoned.
    pub fn acquire(&self, paste_id: &str, owner_id: &LockOwnerId) -> Result<(), PasteLockError> {
        let mut state = self.state()?;
        if state.mutating_pastes.contains(paste_id) {
            return Err(PasteLockError::Mutating {
                paste_id: paste_id.to_string(),
            });
        }
        state
            .holders_by_paste
            .entry(paste_id.to_string())
            .or_default()
            .insert(owner_id.clone());
        Ok(())
    }

    /// Release an edit lock for `paste_id` held by `owner_id`.
    ///
    /// # Arguments
    /// - `paste_id`: Target paste id to unlock.
    /// - `owner_id`: Caller/session owner id.
    ///
    /// # Returns
    /// `Ok(())` when the owner lock is released.
    ///
    /// # Errors
    /// Returns [`PasteLockError::NotHeld`] when `owner_id` does not currently
    /// hold `paste_id`, or [`PasteLockError::Poisoned`] when lock state is
    /// poisoned.
    pub fn release(&self, paste_id: &str, owner_id: &LockOwnerId) -> Result<(), PasteLockError> {
        let mut state = self.state()?;
        let Some(holders) = state.holders_by_paste.get_mut(paste_id) else {
            return Err(PasteLockError::NotHeld {
                paste_id: paste_id.to_string(),
                owner_id: owner_id.clone(),
            });
        };
        if !holders.remove(owner_id) {
            return Err(PasteLockError::NotHeld {
                paste_id: paste_id.to_string(),
                owner_id: owner_id.clone(),
            });
        }
        if holders.is_empty() {
            state.holders_by_paste.remove(paste_id);
        }
        Ok(())
    }

    /// Check whether a paste is currently held by one or more owners.
    ///
    /// # Returns
    /// `Ok(true)` when at least one owner currently holds `paste_id`.
    ///
    /// # Errors
    /// Returns [`PasteLockError::Poisoned`] when lock state is poisoned.
    pub fn is_locked(&self, paste_id: &str) -> Result<bool, PasteLockError> {
        let state = self.state()?;
        Ok(state
            .holders_by_paste
            .get(paste_id)
            .map(|holders| !holders.is_empty())
            .unwrap_or(false))
    }

    /// Begin a mutation guard for one paste id.
    ///
    /// # Returns
    /// A guard that blocks new lock acquisition on `paste_id` until dropped.
    ///
    /// # Errors
    /// Returns an error when `paste_id` is held, already mutating, or lock
    /// state is poisoned.
    pub fn begin_mutation(&self, paste_id: &str) -> Result<PasteMutationGuard<'_>, PasteLockError> {
        self.begin_batch_mutation([paste_id])
    }

    /// Begin a mutation guard for multiple paste ids.
    ///
    /// Fails if any target id is currently held or already mutating.
    ///
    /// # Returns
    /// A guard that blocks new lock acquisition on all provided paste ids until
    /// dropped.
    ///
    /// # Errors
    /// Returns an error when any target id is held, already mutating, or lock
    /// state is poisoned.
    pub fn begin_batch_mutation<'a, I>(
        &'a self,
        paste_ids: I,
    ) -> Result<PasteMutationGuard<'a>, PasteLockError>
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut deduped_ids = Vec::new();
        let mut seen = HashSet::new();
        for id in paste_ids {
            let id = id.as_ref();
            if seen.insert(id.to_string()) {
                deduped_ids.push(id.to_string());
            }
        }

        let mut state = self.state()?;
        for paste_id in &deduped_ids {
            if state.mutating_pastes.contains(paste_id) {
                return Err(PasteLockError::Mutating {
                    paste_id: paste_id.clone(),
                });
            }
            if state
                .holders_by_paste
                .get(paste_id)
                .map(|holders| !holders.is_empty())
                .unwrap_or(false)
            {
                return Err(PasteLockError::Held {
                    paste_id: paste_id.clone(),
                });
            }
        }
        for paste_id in &deduped_ids {
            state.mutating_pastes.insert(paste_id.clone());
        }
        Ok(PasteMutationGuard {
            manager: self,
            paste_ids: deduped_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{LockOwnerId, PasteLockError, PasteLockManager};
    use std::sync::Arc;
    use std::thread;

    fn owner(id: &str) -> LockOwnerId {
        LockOwnerId::new(id.to_string())
    }

    #[test]
    fn two_owner_lock_lifecycle_requires_each_owner_to_release() {
        let locks = PasteLockManager::default();
        let owner_a = owner("owner-a");
        let owner_b = owner("owner-b");

        locks.acquire("alpha", &owner_a).expect("owner-a acquires");
        locks.acquire("alpha", &owner_b).expect("owner-b acquires");
        assert!(locks.is_locked("alpha").expect("is_locked"));

        locks.release("alpha", &owner_a).expect("owner-a releases");
        assert!(
            locks.is_locked("alpha").expect("is_locked"),
            "lock should remain while owner-b still holds it"
        );

        locks.release("alpha", &owner_b).expect("owner-b releases");
        assert!(
            !locks.is_locked("alpha").expect("is_locked"),
            "lock should clear after all owners release"
        );
    }

    #[test]
    fn acquire_is_idempotent_for_same_owner() {
        let locks = PasteLockManager::default();
        let owner_a = owner("owner-a");
        locks.acquire("alpha", &owner_a).expect("first acquire");
        locks
            .acquire("alpha", &owner_a)
            .expect("idempotent acquire should succeed");
        assert!(locks.is_locked("alpha").expect("is_locked"));
        locks.release("alpha", &owner_a).expect("release");
        assert!(
            !locks.is_locked("alpha").expect("is_locked"),
            "single release should clear idempotent duplicate acquire"
        );
    }

    #[test]
    fn release_with_non_holder_owner_returns_typed_error() {
        let locks = PasteLockManager::default();
        let owner_a = owner("owner-a");
        let owner_b = owner("owner-b");
        locks.acquire("alpha", &owner_a).expect("owner-a acquires");

        let err = locks
            .release("alpha", &owner_b)
            .expect_err("owner-b should not be able to release owner-a lock");
        assert!(matches!(err, PasteLockError::NotHeld { .. }));
        assert!(locks.is_locked("alpha").expect("is_locked"));
    }

    #[test]
    fn begin_mutation_blocks_target_acquire_but_not_other_ids() {
        let locks = PasteLockManager::default();
        let owner_a = owner("owner-a");
        let _guard = locks.begin_mutation("alpha").expect("begin mutation");

        let blocked = locks
            .acquire("alpha", &owner_a)
            .expect_err("guarded id should reject acquire");
        assert!(matches!(blocked, PasteLockError::Mutating { .. }));
        locks
            .acquire("beta", &owner_a)
            .expect("other ids should remain acquirable");
        assert!(locks.is_locked("beta").expect("is_locked"));
    }

    #[test]
    fn begin_batch_mutation_blocks_only_affected_ids() {
        let locks = PasteLockManager::default();
        let owner_a = owner("owner-a");
        let _guard = locks
            .begin_batch_mutation(["alpha", "beta"])
            .expect("begin batch mutation");

        let blocked_alpha = locks.acquire("alpha", &owner_a).expect_err("alpha blocked");
        assert!(matches!(blocked_alpha, PasteLockError::Mutating { .. }));
        let blocked_beta = locks.acquire("beta", &owner_a).expect_err("beta blocked");
        assert!(matches!(blocked_beta, PasteLockError::Mutating { .. }));
        locks
            .acquire("gamma", &owner_a)
            .expect("gamma should not be blocked");
        assert!(locks.is_locked("gamma").expect("is_locked"));
    }

    #[test]
    fn methods_return_poisoned_error_instead_of_panicking() {
        let locks = Arc::new(PasteLockManager::default());
        let poison_target = Arc::clone(&locks);
        let _ = thread::spawn(move || {
            let _guard = poison_target.inner.lock().expect("inner lock");
            panic!("poison lock manager");
        })
        .join();

        let owner_a = owner("owner-a");
        assert!(matches!(
            locks.acquire("alpha", &owner_a),
            Err(PasteLockError::Poisoned)
        ));
        assert!(matches!(
            locks.release("alpha", &owner_a),
            Err(PasteLockError::Poisoned)
        ));
        assert!(matches!(
            locks.is_locked("alpha"),
            Err(PasteLockError::Poisoned)
        ));
        assert!(matches!(
            locks.begin_mutation("alpha"),
            Err(PasteLockError::Poisoned)
        ));
    }
}
