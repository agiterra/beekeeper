fn main() {
    beekeeper_agent::legacy_env::adopt_legacy_env("beekeeper-agent");
    if let Err(e) = beekeeper_agent::run() {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
