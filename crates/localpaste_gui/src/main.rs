//! Native rewrite binary entry point.

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
