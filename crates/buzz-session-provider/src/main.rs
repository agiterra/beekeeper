//! Thin entry point for the coding-session provider.

#![deny(unsafe_code)]

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    buzz_session_provider::run().await
}
