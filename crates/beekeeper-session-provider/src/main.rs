//! Thin entry point for the coding-session provider.

#![deny(unsafe_code)]

fn main() -> anyhow::Result<()> {
    // Before clap, tokio or any thread: read BUZZ_* as BEEKEEPER_*.
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-session-provider");
    async_main()
}

#[tokio::main]
async fn async_main() -> anyhow::Result<()> {
    // reqwest and the ACP websocket stack can enable different rustls crypto
    // backends in the same binary. Select ring explicitly before either stack
    // builds a TLS client; rustls deliberately panics when both are linked and
    // no process-level provider has been chosen.
    let _ = rustls::crypto::ring::default_provider().install_default();
    beekeeper_session_provider::run().await
}
