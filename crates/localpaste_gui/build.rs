//! Embeds the Windows icon resource into the GUI executable.
//!
//! This ensures Explorer, taskbar, and Start-menu surfaces use the packaged
//! icon instead of the default PE placeholder.

// On non-Windows targets this script does nothing.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        // Path is relative to this build.rs (i.e. the crate root).
        res.set_icon("../../packaging/windows/localpaste.ico");
        res.compile()
            .expect("failed to embed Windows icon resource");
    }
}
