//! Shared entrypoint helpers for `localpaste_tools` binaries.

/// Run a fallible entrypoint and exit with status code `1` on failure.
pub fn run_or_exit(run: impl FnOnce() -> Result<(), String>) {
    if let Err(message) = run() {
        eprintln!("error: {}", message);
        std::process::exit(1);
    }
}
