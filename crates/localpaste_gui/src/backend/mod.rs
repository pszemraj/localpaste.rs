//! Backend worker wiring for the native rewrite.
//!
//! This module exposes the command/event protocol plus the worker spawn helper
//! used by the egui UI thread.

pub(crate) mod collections;
mod protocol;
mod worker;

pub(crate) use protocol::DELETE_UNDO_LIMIT;
pub use collections::SidebarCollection;
pub use protocol::{CoreCmd, CoreErrorSource, CoreEvent, PasteSummary};
pub use worker::{
    spawn_backend, spawn_backend_with_locks, spawn_backend_with_locks_and_owner, BackendHandle,
};

#[cfg(test)]
mod tests;
