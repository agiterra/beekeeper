use super::types::RuntimeFileConfig;

/// The Beekeeper agent harness (runtime id `buzz-agent`) has no config file — returns an empty config.
/// All config comes from env vars (tier 2a) set at spawn time.
pub(super) fn read_config_file() -> Option<RuntimeFileConfig> {
    None
}
