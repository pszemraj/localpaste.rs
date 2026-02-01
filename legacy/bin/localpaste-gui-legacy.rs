#![cfg(feature = "gui-legacy")]
//! Legacy desktop GUI entrypoint for LocalPaste.

use std::sync::{Arc, Once};

use eframe::egui;
use tracing::error;
use tracing_subscriber::EnvFilter;

#[path = "../gui/mod.rs"]
mod legacy_gui;

fn init_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let env_filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("localpaste_gui=warn"))
            .unwrap();

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .compact()
            .init();
    });
}

fn main() {
    init_tracing();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1200.0, 800.0])
            .with_title("LocalPaste Desktop (Legacy)")
            .with_icon(Arc::new(legacy_gui::app_icon())),
        ..Default::default()
    };

    let app = match legacy_gui::LocalPasteApp::initialise() {
        Ok(app) => app,
        Err(err) => {
            error!("initialise error: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = eframe::run_native(
        "LocalPaste Desktop (Legacy)",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    ) {
        error!("runtime error: {err}");
        std::process::exit(1);
    }
}
