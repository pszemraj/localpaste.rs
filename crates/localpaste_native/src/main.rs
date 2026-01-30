mod app;
mod backend;

use app::LocalPasteApp;
use eframe::egui;
use tracing::error;
use tracing_subscriber::EnvFilter;

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("localpaste=warn,localpaste_native=info"))
        .unwrap();

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

fn main() {
    init_tracing();

    let app = match LocalPasteApp::new() {
        Ok(app) => app,
        Err(err) => {
            error!("failed to start native app: {}", err);
            return;
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 720.0])
            .with_title("LocalPaste Native"),
        ..Default::default()
    };

    if let Err(err) = eframe::run_native(
        "LocalPaste Native",
        options,
        Box::new(|_cc| Ok(Box::new(app))),
    ) {
        error!("native app error: {}", err);
    }
}
