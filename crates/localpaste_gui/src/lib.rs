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
use std::path::{Path, PathBuf};
use tracing_subscriber::EnvFilter;

const DESKTOP_ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icons/desktop_icon.png"
));
#[cfg(target_os = "linux")]
const LINUX_APP_ID: &str = "io.github.pszemraj.localpaste";
#[cfg(any(target_os = "linux", test))]
const LINUX_MANAGED_MARKER: &str = "X-LocalPaste-Managed=true";

fn suppress_vulkan_loader_debug() {
    if env_flag_enabled("LOCALPASTE_KEEP_VK_DEBUG") {
        return;
    }
    remove_env_var("VK_LOADER_DEBUG");
}

fn resolve_log_file_path() -> Option<PathBuf> {
    let raw = std::env::var("LOCALPASTE_LOG_FILE").ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

fn open_log_file(path: &Path) -> std::io::Result<std::fs::File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("localpaste=warn,localpaste_gui=info"))
        .unwrap();

    if let Some(path) = resolve_log_file_path() {
        match open_log_file(path.as_path()) {
            Ok(log_file) => {
                let make_writer = move || -> Box<dyn std::io::Write + Send> {
                    match log_file.try_clone() {
                        Ok(file) => Box::new(file),
                        Err(_) => Box::new(std::io::stderr()),
                    }
                };
                tracing_subscriber::fmt()
                    .with_env_filter(env_filter)
                    .with_target(false)
                    .compact()
                    .with_writer(make_writer)
                    .init();
                return;
            }
            Err(err) => {
                eprintln!(
                    "failed to open LOCALPASTE_LOG_FILE ({}): {}; using stderr logging",
                    path.display(),
                    err
                );
            }
        }
    }

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

#[cfg(target_os = "linux")]
fn linux_data_home() -> Option<PathBuf> {
    match std::env::var_os("XDG_DATA_HOME") {
        Some(path) if !path.is_empty() => Some(PathBuf::from(path)),
        _ => std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")),
    }
}

#[cfg(target_os = "linux")]
fn write_if_different(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    match std::fs::read(path) {
        Ok(existing) if existing == contents => Ok(()),
        Ok(_) | Err(_) => std::fs::write(path, contents),
    }
}

#[cfg(target_os = "linux")]
fn desktop_exec_value(exe_path: &Path) -> String {
    let escaped = exe_path
        .as_os_str()
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    format!("\"{}\"", escaped)
}

#[cfg(any(target_os = "linux", test))]
fn desktop_entry_is_managed(contents: &str) -> bool {
    contents
        .lines()
        .any(|line| line.trim() == LINUX_MANAGED_MARKER)
}

#[cfg(any(target_os = "linux", test))]
fn has_target_build_segment(path: &Path) -> bool {
    let components: Vec<_> = path.components().collect();
    components.windows(2).any(|window| {
        window[0].as_os_str() == "target"
            && (window[1].as_os_str() == "debug" || window[1].as_os_str() == "release")
    })
}

#[cfg(any(target_os = "linux", test))]
fn linux_exe_path_looks_stable_with_home(exe_path: &Path, home: Option<&Path>) -> bool {
    if has_target_build_segment(exe_path) {
        return false;
    }

    if ["/usr/bin", "/usr/local/bin", "/opt", "/nix/store"]
        .iter()
        .any(|prefix| exe_path.starts_with(prefix))
    {
        return true;
    }

    home.is_some_and(|home| {
        exe_path.starts_with(home.join(".cargo/bin"))
            || exe_path.starts_with(home.join(".local/bin"))
    })
}

#[cfg(target_os = "linux")]
fn linux_exe_path_looks_stable(exe_path: &Path) -> bool {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    linux_exe_path_looks_stable_with_home(exe_path, home.as_deref())
}

#[cfg(any(target_os = "linux", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxDesktopEntryDecision {
    WriteManagedEntry,
    SkipTransientExecutable,
    SkipSystemDesktopOverride,
    SkipUnmanagedUserEntry,
}

#[cfg(any(target_os = "linux", test))]
fn decide_linux_desktop_entry_write(
    exe_path_is_stable: bool,
    user_desktop_entry_contents: Option<&str>,
    system_desktop_entry_exists: bool,
) -> LinuxDesktopEntryDecision {
    if !exe_path_is_stable {
        return LinuxDesktopEntryDecision::SkipTransientExecutable;
    }

    if user_desktop_entry_contents.is_none() && system_desktop_entry_exists {
        return LinuxDesktopEntryDecision::SkipSystemDesktopOverride;
    }

    if let Some(contents) = user_desktop_entry_contents {
        if !desktop_entry_is_managed(contents) {
            return LinuxDesktopEntryDecision::SkipUnmanagedUserEntry;
        }
    }

    LinuxDesktopEntryDecision::WriteManagedEntry
}

#[cfg(target_os = "linux")]
fn linux_system_desktop_entry_exists() -> bool {
    ["/usr/share/applications", "/usr/local/share/applications"]
        .iter()
        .any(|root| {
            Path::new(root)
                .join(format!("{LINUX_APP_ID}.desktop"))
                .is_file()
        })
}

#[cfg(target_os = "linux")]
fn ensure_linux_desktop_integration() -> std::io::Result<()> {
    // AppImage/packaged launches already carry desktop integration.
    if std::env::var_os("APPIMAGE").is_some() {
        return Ok(());
    }

    let data_home = linux_data_home().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "failed to resolve Linux data home",
        )
    })?;
    let exe_path = std::env::current_exe()?;

    let icon_dir = data_home.join("icons/hicolor/256x256/apps");
    std::fs::create_dir_all(&icon_dir)?;
    let icon_path = icon_dir.join(format!("{LINUX_APP_ID}.png"));
    write_if_different(icon_path.as_path(), DESKTOP_ICON_PNG)?;

    let app_dir = data_home.join("applications");
    std::fs::create_dir_all(&app_dir)?;
    let desktop_path = app_dir.join(format!("{LINUX_APP_ID}.desktop"));
    let user_desktop_entry_contents = if desktop_path.exists() {
        Some(std::fs::read_to_string(desktop_path.as_path())?)
    } else {
        None
    };

    match decide_linux_desktop_entry_write(
        linux_exe_path_looks_stable(exe_path.as_path()),
        user_desktop_entry_contents.as_deref(),
        linux_system_desktop_entry_exists(),
    ) {
        LinuxDesktopEntryDecision::WriteManagedEntry => {}
        LinuxDesktopEntryDecision::SkipTransientExecutable => {
            tracing::debug!(
                "skipping desktop entry write for transient executable path: {}",
                exe_path.display()
            );
            return Ok(());
        }
        LinuxDesktopEntryDecision::SkipSystemDesktopOverride => {
            tracing::debug!(
                "system desktop entry already exists; avoiding user-level override at {}",
                desktop_path.display()
            );
            return Ok(());
        }
        LinuxDesktopEntryDecision::SkipUnmanagedUserEntry => {
            tracing::debug!(
                "existing desktop entry is unmanaged; leaving it untouched: {}",
                desktop_path.display()
            );
            return Ok(());
        }
    }

    let desktop_entry = format!(
        "[Desktop Entry]\n\
Type=Application\n\
Name=LocalPaste\n\
Comment=A fast, localhost-only pastebin with a modern editor, built in Rust.\n\
Exec={}\n\
Icon={}\n\
Terminal=false\n\
Categories=Utility;Development;TextEditor;\n\
StartupNotify=true\n\
{}\n",
        desktop_exec_value(exe_path.as_path()),
        LINUX_APP_ID,
        LINUX_MANAGED_MARKER
    );
    write_if_different(desktop_path.as_path(), desktop_entry.as_bytes())
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

    #[cfg(target_os = "linux")]
    if let Err(err) = ensure_linux_desktop_integration() {
        tracing::warn!("failed to set up Linux desktop integration: {}", err);
    }

    let app = LocalPasteApp::new().map_err(|err| eframe::Error::AppCreation(Box::new(err)))?;

    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size(app::DEFAULT_WINDOW_SIZE)
        .with_min_inner_size(app::MIN_WINDOW_SIZE)
        .with_title("LocalPaste.rs");
    #[cfg(target_os = "linux")]
    {
        viewport = viewport.with_app_id(LINUX_APP_ID);
    }
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
    use super::{
        decide_linux_desktop_entry_write, desktop_entry_is_managed,
        linux_exe_path_looks_stable_with_home, LinuxDesktopEntryDecision,
    };
    use super::{load_desktop_icon, open_log_file, resolve_log_file_path};
    use localpaste_core::env::{env_lock, EnvGuard};
    use std::io::Write;
    use std::path::Path;
    use std::path::PathBuf;

    #[test]
    fn desktop_icon_asset_decodes() {
        let icon = load_desktop_icon().expect("desktop icon should decode");
        assert!(icon.width > 0);
        assert!(icon.height > 0);
        assert_eq!(icon.rgba.len() as u32, icon.width * icon.height * 4);
    }

    #[test]
    fn resolve_log_file_path_env_matrix() {
        let _lock = env_lock().lock().expect("env lock");
        let _restore = EnvGuard::remove("LOCALPASTE_LOG_FILE");

        assert!(resolve_log_file_path().is_none());

        {
            let _blank = EnvGuard::set("LOCALPASTE_LOG_FILE", "   ");
            assert!(resolve_log_file_path().is_none());
        }

        {
            let _set = EnvGuard::set("LOCALPASTE_LOG_FILE", "logs/gui.log");
            assert_eq!(resolve_log_file_path(), Some(PathBuf::from("logs/gui.log")));
        }
    }

    #[test]
    fn open_log_file_creates_parent_and_appends() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("nested").join("gui.log");

        {
            let mut file = open_log_file(path.as_path()).expect("open first");
            writeln!(file, "first line").expect("write first");
        }
        {
            let mut file = open_log_file(path.as_path()).expect("open second");
            writeln!(file, "second line").expect("write second");
        }

        let body = std::fs::read_to_string(path.as_path()).expect("read");
        assert!(body.contains("first line"));
        assert!(body.contains("second line"));
    }

    #[test]
    fn desktop_entry_marker_detection_matrix() {
        assert!(desktop_entry_is_managed(
            "[Desktop Entry]\nName=LocalPaste\nX-LocalPaste-Managed=true\n"
        ));
        assert!(!desktop_entry_is_managed(
            "[Desktop Entry]\nName=LocalPaste\nStartupNotify=true\n"
        ));
    }

    #[test]
    fn linux_stable_exe_path_heuristic_matrix() {
        let home = Path::new("/tmp/localpaste-home");

        assert!(!linux_exe_path_looks_stable_with_home(
            Path::new("/work/localpaste/target/debug/localpaste-gui"),
            Some(home)
        ));
        assert!(!linux_exe_path_looks_stable_with_home(
            Path::new("/work/localpaste/target/release/localpaste-gui"),
            Some(home)
        ));
        assert!(linux_exe_path_looks_stable_with_home(
            Path::new("/tmp/localpaste-home/.cargo/bin/localpaste-gui"),
            Some(home)
        ));
        assert!(linux_exe_path_looks_stable_with_home(
            Path::new("/tmp/localpaste-home/.local/bin/localpaste-gui"),
            Some(home)
        ));
        assert!(linux_exe_path_looks_stable_with_home(
            Path::new("/usr/bin/localpaste-gui"),
            Some(home)
        ));
    }

    #[test]
    fn linux_desktop_entry_write_decision_matrix() {
        assert_eq!(
            decide_linux_desktop_entry_write(false, None, false),
            LinuxDesktopEntryDecision::SkipTransientExecutable
        );

        assert_eq!(
            decide_linux_desktop_entry_write(true, None, true),
            LinuxDesktopEntryDecision::SkipSystemDesktopOverride
        );

        assert_eq!(
            decide_linux_desktop_entry_write(
                true,
                Some("[Desktop Entry]\nName=LocalPaste\nStartupNotify=true\n"),
                false,
            ),
            LinuxDesktopEntryDecision::SkipUnmanagedUserEntry
        );

        assert_eq!(
            decide_linux_desktop_entry_write(
                true,
                Some("[Desktop Entry]\nX-LocalPaste-Managed=true\n"),
                true,
            ),
            LinuxDesktopEntryDecision::WriteManagedEntry
        );

        assert_eq!(
            decide_linux_desktop_entry_write(true, None, false),
            LinuxDesktopEntryDecision::WriteManagedEntry
        );
    }
}
