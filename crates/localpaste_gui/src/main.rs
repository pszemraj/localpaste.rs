//! Native rewrite binary entry point.
#![cfg_attr(all(target_os = "windows", not(debug_assertions)), windows_subsystem = "windows")]

fn main() {
    let exit_code = run_and_report(localpaste_gui::run);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

fn run_and_report<F, E>(runner: F) -> i32
where
    F: FnOnce() -> Result<(), E>,
    E: std::fmt::Display,
{
    match runner() {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("native app error: {}", err);
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::run_and_report;

    #[test]
    fn workspace_manifest_defaults_to_gui_member() {
        let root_manifest =
            std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml"))
                .expect("read workspace Cargo.toml");
        assert!(
            root_manifest.contains("default-members = [\"crates/localpaste_gui\"]"),
            "workspace should keep localpaste_gui as the default member for `cargo run`"
        );
    }

    #[test]
    fn run_and_report_returns_zero_on_success() {
        let exit_code = run_and_report(|| Ok::<(), &str>(()));
        assert_eq!(exit_code, 0);
    }

    #[test]
    fn run_and_report_returns_non_zero_on_failure() {
        let exit_code = run_and_report(|| Err::<(), &str>("boom"));
        assert_eq!(exit_code, 1);
    }
}
