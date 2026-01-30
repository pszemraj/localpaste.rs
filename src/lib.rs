//! Root crate facade for LocalPaste server and legacy GUI.

#[cfg(feature = "gui-legacy")]
#[path = "../legacy/gui/mod.rs"]
/// Legacy egui desktop UI (feature-gated).
pub mod gui;

pub use localpaste_server::{
    config, create_app, db, error, handlers, locks, models, naming, serve_router, AppError,
    AppState, Config, Database, PasteLockManager,
};
