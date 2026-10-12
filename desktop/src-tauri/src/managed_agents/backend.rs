//! Client adapters for the shared remote-provider protocol boundary.

pub use beekeeper_host_core::provider_boundary::{
    discover_provider_candidates, redact_env_values_in, redact_secrets_with,
    resolve_provider_binary, validate_provider_config, BackendProviderInfo,
};

/// Invoke the shared boundary with the desktop's established local working directory.
pub fn invoke_provider(
    binary: &std::path::Path,
    request: &serde_json::Value,
    timeout: std::time::Duration,
) -> Result<serde_json::Value, String> {
    let workdir = super::default_agent_workdir();
    beekeeper_host_core::provider_boundary::invoke_provider_at(
        binary,
        request,
        timeout,
        workdir.as_deref(),
    )
}

/// Negotiate and deploy on immutable bytes using resolved client custody and payload.
pub fn provider_deploy(
    binary: &std::path::Path,
    agent: &serde_json::Value,
    config: &serde_json::Value,
) -> Result<String, String> {
    let workdir = super::default_agent_workdir();
    beekeeper_host_core::provider_boundary::provider_deploy_at(
        binary,
        agent,
        config,
        workdir.as_deref(),
    )
}
