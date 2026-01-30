//! Native rewrite binary entry point.

fn main() {
    if let Err(err) = localpaste_native::run() {
        eprintln!("native app error: {}", err);
    }
}
