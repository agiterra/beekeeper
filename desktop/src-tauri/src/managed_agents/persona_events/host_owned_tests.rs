//! Host-owned identity facts on `apply_persona_snapshot` (item 90).
//!
//! Kept beside `tests.rs` so that file stays under the repository file-size
//! gate; `#[path]`-included from `persona_events.rs`.

use super::tests::{sample_persona, sample_record};
use crate::managed_agents::persona_events::apply_persona_snapshot;
use crate::managed_agents::types::AgentDefinition;

// Model, provider and runtime are HOST-owned: they describe what this computer
// runs an identity on. Role packs are model-agnostic BY DESIGN and carry `null`
// for all three (`crew_roles.rs`), so a blank definition value must never clear
// a value the host set. The live failure this guards: Banksy's record was set
// to runtime `codex` / model `gpt-5.6-sol` by hand at 23:17 and the app rewrote
// both to `null` at 00:17 on the next whole-file save.

/// A blank definition must not clear a host-set model/provider/runtime.
#[test]
fn apply_persona_snapshot_keeps_host_set_quad_when_definition_is_blank() {
    let mut record = sample_record();
    record.model = Some("gpt-5.6-sol".into());
    record.provider = Some("openai".into());
    record.runtime = Some("codex".into());

    apply_persona_snapshot(
        &mut record,
        &AgentDefinition {
            model: None,
            provider: None,
            runtime: None,
            ..sample_persona()
        },
    );

    assert_eq!(
        record.model.as_deref(),
        Some("gpt-5.6-sol"),
        "a role pack that names no model must not clear the host's model"
    );
    assert_eq!(record.provider.as_deref(), Some("openai"));
    assert_eq!(
        record.runtime.as_deref(),
        Some("codex"),
        "a role pack that names no runtime must not clear the host's runtime"
    );
}

/// Whitespace-only definition values are blank too — they must not clear
/// either, and must never be materialized onto the record.
#[test]
fn apply_persona_snapshot_treats_blank_definition_values_as_absent() {
    let mut record = sample_record();
    record.model = Some("gpt-5.6-sol".into());
    record.provider = Some("openai".into());
    record.runtime = Some("codex".into());

    apply_persona_snapshot(
        &mut record,
        &AgentDefinition {
            model: Some("   ".into()),
            provider: Some(String::new()),
            runtime: Some("  ".into()),
            ..sample_persona()
        },
    );

    assert_eq!(record.model.as_deref(), Some("gpt-5.6-sol"));
    assert_eq!(record.provider.as_deref(), Some("openai"));
    assert_eq!(record.runtime.as_deref(), Some("codex"));
}

/// The definition still fills a field the host left unset.
#[test]
fn apply_persona_snapshot_fills_absent_record_quad_from_definition() {
    let mut record = sample_record();
    assert!(record.model.is_none(), "test setup: record starts blank");

    apply_persona_snapshot(&mut record, &sample_persona());

    assert_eq!(record.model.as_deref(), Some("claude-opus-4"));
    assert_eq!(record.provider.as_deref(), Some("anthropic"));
    assert_eq!(record.runtime.as_deref(), Some("goose"));
}

/// A blank definition runtime must not drop a host harness pin either: the pin
/// belongs to the runtime the host chose, and there is no new runtime to make
/// it stale.
#[test]
fn apply_persona_snapshot_keeps_harness_pin_when_definition_runtime_is_blank() {
    let mut record = sample_record();
    record.runtime = Some("codex".into());
    record.agent_command_override = Some("goose".into());

    apply_persona_snapshot(
        &mut record,
        &AgentDefinition {
            runtime: None,
            ..sample_persona()
        },
    );

    assert_eq!(
        record.agent_command_override.as_deref(),
        Some("goose"),
        "a pack that names no runtime must not drop the host's harness pin"
    );
    assert_eq!(record.runtime.as_deref(), Some("codex"));
}
