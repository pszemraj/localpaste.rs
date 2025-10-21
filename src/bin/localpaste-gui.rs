#![cfg(feature = "gui")]

use std::sync::Arc;

use eframe::egui;
use localpaste::gui::{self, LocalPasteApp};

fn main() {
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
            eprintln!("[localpaste-gui] initialise error: {err}");
            std::process::exit(1);
        }
    };

    if let Err(err) = eframe::run_native(
        "LocalPaste Desktop",
        options,
        Box::new(move |_cc| Ok(Box::new(app))),
    ) {
        eprintln!("[localpaste-gui] runtime error: {err}");
        std::process::exit(1);
    }
}
