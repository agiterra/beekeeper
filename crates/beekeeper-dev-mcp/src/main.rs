fn main() -> Result<(), Box<dyn std::error::Error>> {
    beekeeper_core::env_compat::adopt_legacy_env("beekeeper-dev-mcp");
    beekeeper_dev_mcp::run()
}
