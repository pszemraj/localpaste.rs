#![cfg(feature = "gui")]
//! Desktop GUI entrypoint for LocalPaste.

use std::sync::{Arc, Once};

use eframe::egui;
use localpaste::gui::{self, LocalPasteApp};
use tracing::error;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let env_filter = EnvFilter::try_from_default_env()
            .or_else(|_| EnvFilter::try_new("localpaste=warn,localpaste::gui=warn"))
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
            .with_title("LocalPaste Desktop")
            .with_icon(Arc::new(gui::app_icon())),
        ..Default::default()
    };

    let app = match LocalPasteApp::initialise() {
        Ok(app) => app,
        Err(err) => {
            error!("initialise error: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = eframe::run_native(
        "LocalPaste Desktop",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    ) {
        error!("runtime error: {err}");
        std::process::exit(1);
    }
}
