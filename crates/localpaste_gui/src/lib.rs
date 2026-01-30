//! Native rewrite library entry point.
//!
//! Exposes a `run` helper so the workspace root can launch the native UI
//! without duplicating initialization logic.

mod app;
mod backend;

use app::LocalPasteApp;
use eframe::egui;
use tracing::error;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("localpaste=warn,localpaste_gui=info"))
        .unwrap();

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

/// Start the native rewrite UI with tracing enabled.
///
/// # Returns
/// The result of `eframe::run_native`.
///
/// # Errors
/// Propagates any `eframe` initialization or runtime error (including app
/// creation failures when the database cannot be opened).
pub fn run() -> eframe::Result<()> {
    init_tracing();

    let app = LocalPasteApp::new().map_err(|err| {
        error!("failed to start native app: {}", err);
        eframe::Error::AppCreation(Box::new(err))
    })?;

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("LocalPaste.rs"),
        ..Default::default()
    };

    eframe::run_native("LocalPaste.rs", options, Box::new(|_cc| Ok(Box::new(app))))
}
