//! Tests for the native-steer idle guard the host declares per runtime.
//!
//! Split from `tests.rs` so that file stays under the repository's
//! 1000-line ceiling. The two facts defended here: the wire carries
//! `steerIdleGuard` only for the runtime whose adapter was verified to honour
//! it, and the host table never declares one for anything else.

use std::path::Path;

use crate::session_provider::env::{build_provider_env, ProviderEnvInputs};
use crate::session_provider::tests::{sample_record, RELAY};

/// The idle guard is the fact that turns native steering on, so the wire
/// must carry it for the runtime whose adapter was verified to honour it and
/// carry *nothing* — not `null` — for one that was not.
#[test]
fn env_declares_the_steer_idle_guard_for_claude_and_omits_it_for_codex() {
    use buzz_core_pkg::coding_session_runtime::{RuntimeDescriptor, SteerIdleGuard};
    let record = sample_record();
    let env = build_provider_env(&ProviderEnvInputs {
        record: &record,
        relay_url: RELAY,
        state_dir: Path::new("/tmp/session-provider/aaaa"),
        agent_command: None,
        context_mcp_command: None,
        claude_code_executable: None,
        runtimes: vec![
            RuntimeDescriptor {
                steer_idle_guard: Some(SteerIdleGuard::PromptRequired),
                instance_ref: "claude-primary".into(),
                driver: "claude-agent-acp".into(),
                runtime: "claude".into(),
                agent_command: "claude-agent-acp".into(),
                agent_args: Vec::new(),
                cli_env: None,
                default_model: "default".into(),
                allowed_models: vec!["default".into()],
                discover_models: true,
                capabilities: None,
            },
            RuntimeDescriptor {
                steer_idle_guard: None,
                instance_ref: "codex-primary".into(),
                driver: "codex-acp".into(),
                runtime: "codex".into(),
                agent_command: "codex-acp".into(),
                agent_args: Vec::new(),
                cli_env: None,
                default_model: "default".into(),
                allowed_models: vec!["default".into()],
                discover_models: true,
                capabilities: None,
            },
        ],
        augmented_path: None,
        max_sessions: None,
        turn_idle_timeout_secs: None,
        rust_log: None,
        emit_raw_sdk_frames: false,
        turn_budget: None,
        app_checkout: None,
    });
    let raw = env.get("BUZZ_CSP_RUNTIMES").expect("runtimes in env");
    let list: Vec<serde_json::Value> = serde_json::from_str(raw).expect("json array");
    let claude = list
        .iter()
        .find(|entry| entry["instanceRef"] == "claude-primary")
        .expect("claude row");
    let codex = list
        .iter()
        .find(|entry| entry["instanceRef"] == "codex-primary")
        .expect("codex row");
    assert_eq!(claude["steerIdleGuard"], "promptRequired");
    assert!(
        raw.contains(r#""steerIdleGuard":"promptRequired""#),
        "the exact wire spelling is the contract: {raw}"
    );
    assert!(
        !codex
            .as_object()
            .expect("object")
            .contains_key("steerIdleGuard"),
        "codex must carry no guard key at all, not null: {codex}"
    );
    // And the sidecar's own parser reads both rows back exactly.
    let parsed = buzz_core_pkg::coding_session_runtime::parse_runtime_descriptors(raw)
        .expect("the sidecar parser must accept what the host writes");
    assert_eq!(
        parsed[0].steer_idle_guard,
        Some(SteerIdleGuard::PromptRequired)
    );
    assert_eq!(parsed[1].steer_idle_guard, None);
}

/// The host table itself: only the claude row declares the guard, because
/// only claude-agent-acp was verified to honour it (0.70.0,
/// `dist/acp-agent.js:1146-1150`). Whichever of codex/goose resolve on this
/// machine, none of them may claim one.
#[test]
fn the_host_runtime_table_declares_the_idle_guard_for_claude_only() {
    use buzz_core_pkg::coding_session_runtime::SteerIdleGuard;
    let descriptors = crate::session_provider::runtimes::build_runtime_descriptors();
    let claude = descriptors
        .iter()
        .find(|descriptor| descriptor.driver == "claude-agent-acp")
        .expect("claude is always offered");
    assert_eq!(
        claude.steer_idle_guard,
        Some(SteerIdleGuard::PromptRequired)
    );
    for other in descriptors
        .iter()
        .filter(|descriptor| descriptor.driver != "claude-agent-acp")
    {
        assert_eq!(
            other.steer_idle_guard, None,
            "{} must not declare an idle guard",
            other.driver
        );
    }
}
