use std::net::SocketAddr;
use std::sync::Arc;

use beekeeper_pair_relay::{run_server, Relay};
use tokio::net::TcpListener;

fn main() {
    // Before clap, tokio or any thread: read BUZZ_* as BEEKEEPER_*.
    beekeeper_pair_relay::legacy_env::adopt_legacy_env("beekeeper-pair-relay");
    async_main()
}

#[tokio::main]
async fn async_main() {
    let addr_raw = std::env::var("BEEKEEPER_PAIR_RELAY_BIND_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5000".to_string());
    let addr: SocketAddr = match addr_raw.parse() {
        Ok(addr) => addr,
        Err(e) => {
            eprintln!("fatal: invalid BEEKEEPER_PAIR_RELAY_BIND_ADDR {addr_raw:?}: {e}");
            std::process::exit(1);
        }
    };
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("fatal: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    let relay = Arc::new(Relay::new());
    run_server(listener, relay).await;
}
