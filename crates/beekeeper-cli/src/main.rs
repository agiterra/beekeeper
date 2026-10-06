#[tokio::main]
async fn main() {
    std::process::exit(beekeeper_cli::run_from_args(std::env::args()).await);
}
