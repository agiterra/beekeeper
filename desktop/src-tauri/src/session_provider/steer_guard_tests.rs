//! Tests for the native-steer idle guard the host declares per runtime.
//!
//! The env half moved to `buzz_session_host_core::contract_tests` with
//! `build_provider_env`. What stays here is the claim about *this host's*
//! runtime table: only the claude row declares the guard, because only
//! claude-agent-acp was verified to honour it.

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
