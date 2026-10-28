//! A pinned known runtime command is the instance's runtime (ledger 401). The
//! C4 `builder` was pinned to `codex-acp` with a stale `runtime: "claude"`; its
//! effective runtime read Claude, so the hire host asked for `claude-primary`.
use super::*;

#[test]
fn a_pinned_known_runtime_command_is_the_instances_runtime() {
    let def = definition("crew-role:builder", None, None, "You are builder.");
    let mut agent = record(Some("crew-role:builder"), None, None, None);
    agent.runtime = Some("claude".into());
    agent.agent_command = "claude-agent-acp".into();
    agent.agent_command_override = Some("codex-acp".into());
    let resolved = |agent: &ManagedAgentRecord| match resolve_effective_config(
        agent,
        std::slice::from_ref(&def),
        &global(None, None),
    ) {
        EffectiveConfigResult::Resolved(cfg) => cfg.runtime.value,
        EffectiveConfigResult::OrphanedInstance { .. } => panic!("linked"),
    };
    assert_eq!(resolved(&agent).as_deref(), Some("codex"));

    // A custom command names no runtime; the record's pin still answers.
    agent.agent_command_override = Some("/opt/isolated/custom-adapter".into());
    assert_eq!(resolved(&agent).as_deref(), Some("claude"));

    // The same rule for an instance with no definition.
    let mut legacy = record(None, None, None, Some("standalone"));
    legacy.runtime = Some("claude".into());
    legacy.agent_command_override = Some("codex-acp".into());
    assert_eq!(resolved(&legacy).as_deref(), Some("codex"));
}
