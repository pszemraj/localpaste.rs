fn main() {
    if let Err(err) = localpaste_native::run() {
        eprintln!("localpaste native failed: {}", err);
    }
}
