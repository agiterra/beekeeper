//! Tests for `crew_roles_runtime.rs`: the runtime a fresh install pins
//! (ledger 165). A child of `crew_roles::tests`, like the naming tests, for
//! its pack and install helpers.

use super::*;
use crate::managed_agents::crew_roles::pin_default_runtime;

/// A pack pins no runtime, so a fresh record resolved to the app's default
/// harness (`buzz-agent`, the chat harness) — which no coding-session
/// provider runs, so every hire of the identity was refused
/// `HIRE_PROVIDER_NOT_ALLOWED` naming it. The install now pins this
/// computer's preferred runtime onto records that pin none, and the stored
/// harness follows the pin the way a spawn would resolve it.
#[test]
fn a_fresh_install_pins_the_hosts_preferred_runtime_and_the_harness_follows() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");
    let mut result = install(&scan, Vec::new(), Vec::new(), &[]);
    assert_eq!(result.agents[0].runtime, None, "the pack pins no runtime");
    assert_eq!(
        crate::managed_agents::known_acp_runtime(&result.agents[0].agent_command).map(|r| r.id),
        Some("buzz-agent"),
        "without a pin the record lands on the app default, which no coding-session provider runs"
    );

    let pinned = pin_default_runtime(&mut result, Some("claude"));
    assert_eq!(pinned, vec!["lead".to_string()]);
    let record = &result.agents[0];
    assert_eq!(record.runtime.as_deref(), Some("claude"));
    assert_eq!(
        crate::managed_agents::known_acp_runtime(&record.agent_command).map(|r| r.id),
        Some("claude"),
        "the stored harness follows the pin"
    );
}

/// The pin is a default, not an override: a runtime the host seated an
/// identity on (item 90) is a fact this must not move.
#[test]
fn the_default_runtime_never_moves_an_identity_the_host_already_seated() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");
    let first = install(&scan, Vec::new(), Vec::new(), &[]);
    let mut seated = first.agents.clone();
    seated[0].runtime = Some("codex".into());
    let mut second = install(
        &scan,
        first.definitions.clone(),
        seated,
        std::slice::from_ref(&first.team),
    );

    let pinned = pin_default_runtime(&mut second, Some("claude"));
    assert!(pinned.is_empty(), "nothing was pinned: {pinned:?}");
    assert_eq!(second.agents[0].runtime.as_deref(), Some("codex"));
    assert_eq!(
        crate::managed_agents::known_acp_runtime(&second.agents[0].agent_command).map(|r| r.id),
        Some("codex")
    );
}

/// No preference pins nothing: the record stays unpinned rather than being
/// moved onto a runtime nobody chose.
#[test]
fn no_preferred_runtime_pins_nothing() {
    let root = tempfile::tempdir().expect("temp dir");
    write_pack(root.path(), "lead", "lead", Some("lead"));
    let scan = scan_role_packs(root.path()).expect("scan succeeds");
    let mut result = install(&scan, Vec::new(), Vec::new(), &[]);
    for preference in [None, Some(""), Some("  ")] {
        assert!(pin_default_runtime(&mut result, preference).is_empty());
        assert_eq!(result.agents[0].runtime, None);
    }
}
