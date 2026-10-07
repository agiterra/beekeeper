fn main() -> anyhow::Result<()> {
    // First: read BUZZ_* as BEEKEEPER_*. `run()` then maps the older
    // BEEKEEPER_ACP_* aliases (so BUZZ_ACP_PRIVATE_KEY reaches
    // BEEKEEPER_PRIVATE_KEY in two steps), and only then starts tokio.
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-acp");
    beekeeper_acp::run()
}
