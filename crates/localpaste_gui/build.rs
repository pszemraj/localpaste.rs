//! Embeds the Windows icon resource into the GUI executable.
//!
//! This ensures Explorer, taskbar, and Start-menu surfaces use the packaged
//! icon instead of the default PE placeholder.

use std::{env, path::PathBuf};

// On non-Windows *targets* this script does nothing.
fn main() {
    let manifest_dir = PathBuf::from(
        env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR for build script"),
    );
    let icon_path = manifest_dir.join("../../packaging/windows/localpaste.ico");

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={}", icon_path.display());

    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        // Path is relative to this build.rs (i.e. the crate root).
        res.set_icon(icon_path.to_string_lossy().as_ref());
        res.compile()
            .expect("failed to embed Windows icon resource");
    }
}
