//! Native rewrite library entry point.
//!
//! Exposes a `run` helper so the workspace root can launch the native UI
//! without duplicating initialization logic.

mod app;
/// Backend worker + protocol types used by the GUI and headless tests.
pub mod backend;
mod lock_owner;

use app::LocalPasteApp;
use eframe::egui;
use localpaste_core::config::env_flag_enabled;
use localpaste_core::env::remove_env_var;
use tracing_subscriber::EnvFilter;

const DESKTOP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icons/desktop_icon.png"
));

fn suppress_vulkan_loader_debug() {
    if env_flag_enabled("LOCALPASTE_KEEP_VK_DEBUG") {
        return;
    }
    remove_env_var("VK_LOADER_DEBUG");
}

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

fn load_desktop_icon() -> Option<egui::IconData> {
    match eframe::icon_data::from_png_bytes(DESKTOP_ICON_PNG) {
        Ok(icon) => Some(icon),
        Err(err) => {
            tracing::warn!("failed to decode desktop icon PNG: {}", err);
            None
        }
    }
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
    suppress_vulkan_loader_debug();
    init_tracing();

    let app = LocalPasteApp::new().map_err(|err| eframe::Error::AppCreation(Box::new(err)))?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(app::DEFAULT_WINDOW_SIZE)
        .with_min_inner_size(app::MIN_WINDOW_SIZE)
        .with_title("LocalPaste.rs");
    if let Some(icon) = load_desktop_icon() {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native("LocalPaste.rs", options, Box::new(|_cc| Ok(Box::new(app))))
}

#[cfg(test)]
mod tests {
    use super::load_desktop_icon;

    #[test]
    fn desktop_icon_asset_decodes() {
        let icon = load_desktop_icon().expect("desktop icon should decode");
        assert!(icon.width > 0);
        assert!(icon.height > 0);
        assert_eq!(icon.rgba.len() as u32, icon.width * icon.height * 4);
    }
}
