#![cfg(feature = "gui")]
//! Primary desktop GUI entrypoint for the rewrite.

fn main() {
    if let Err(err) = localpaste_native::run() {
        eprintln!("localpaste gui failed: {}", err);
    }
}
