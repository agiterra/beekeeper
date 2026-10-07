fn main() {
    // Before clap, tokio or any thread: read BUZZ_* as BEEKEEPER_*.
    beekeeper_core::env_compat::adopt_legacy_env("bee");
    async_main()
}

#[tokio::main]
async fn async_main() {
    std::process::exit(beekeeper_cli::run_from_args(std::env::args()).await);
}
