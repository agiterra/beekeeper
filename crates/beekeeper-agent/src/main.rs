fn main() {
    if let Err(e) = beekeeper_agent::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
