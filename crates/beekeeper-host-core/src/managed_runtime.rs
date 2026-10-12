//! Commissioned slot for adapters selected by protected managed-agent custody.
use beekeeper_core::{
    coding_session_payload::Capabilities, coding_session_runtime::RuntimeDescriptor,
};

/// Stable provider instance for a managed actor's explicitly configured adapter.
pub const INSTANCE_REF: &str = "managed-actor";

/// Describe the managed adapter slot without claiming a particular vendor.
///
/// The execution worker must require protected `effective_command` settings
/// before launching this slot. The sentinel command is deliberately unavailable;
/// commissioning alone cannot turn an unspecified adapter into a fallback.
pub fn descriptor() -> RuntimeDescriptor {
    RuntimeDescriptor {
        instance_ref: INSTANCE_REF.into(),
        driver: "managed-acp".into(),
        runtime: "managed".into(),
        agent_command: "beekeeper-managed-adapter-required".into(),
        agent_args: Vec::new(),
        cli_env: None,
        default_model: "default".into(),
        allowed_models: vec!["default".into()],
        discover_models: false,
        capabilities: Some(Capabilities::v1_baseline()),
        steer_idle_guard: None,
        title_model: Some(None),
    }
}
