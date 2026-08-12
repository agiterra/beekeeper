//! Thin entry point for the coding-session provider.

#![deny(unsafe_code)]

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // reqwest and the ACP websocket stack can enable different rustls crypto
    // backends in the same binary. Select ring explicitly before either stack
    // builds a TLS client; rustls deliberately panics when both are linked and
    // no process-level provider has been chosen.
    let _ = rustls::crypto::ring::default_provider().install_default();
    buzz_session_provider::run().await
}
