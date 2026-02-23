// build.rs - embed the .ico into the Windows PE binary so Explorer, the
// taskbar, and Start-Menu shortcuts all show the correct icon.
// On non-Windows targets this script does nothing.
fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winresource::WindowsResource::new();
        // Path is relative to this build.rs (i.e. the crate root).
        res.set_icon("../../packaging/windows/localpaste.ico");
        res.compile().expect("failed to embed Windows icon resource");
    }
}
