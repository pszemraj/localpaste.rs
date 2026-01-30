//! Native rewrite binary entry point.

fn main() {
    if let Err(err) = localpaste_gui::run() {
        eprintln!("native app error: {}", err);
    }
}
