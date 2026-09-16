//! The effective-config projection the Agents card reads.

use crate::managed_agents::types::ManagedAgentRecord;

/// The four effective-config fields the summary reports: model, provider,
/// system prompt, and the tier the *model* came from (`model_source`).
///
/// Pure and `AppHandle`-free so the precedence the card renders is unit
/// testable. `model_source` names the winning tier — `instance` when this host
/// set the model on the record itself (item 90), `definition` when the pack
/// named it, `global` when it fell through to the app default. An orphaned
/// link reports all four as absent and logs once.
pub(super) fn summary_effective_fields(
    record: &ManagedAgentRecord,
    personas: &[crate::managed_agents::types::AgentDefinition],
    global_config: &crate::managed_agents::GlobalAgentConfig,
) -> (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<crate::managed_agents::effective_config::ConfigSource>,
) {
    match crate::managed_agents::effective_config::resolve_effective_config(
        record,
        personas,
        global_config,
    ) {
        crate::managed_agents::effective_config::EffectiveConfigResult::Resolved(cfg) => {
            let source = cfg.model.source.clone();
            (
                cfg.model.value,
                cfg.provider.value,
                cfg.system_prompt.value,
                Some(source),
            )
        }
        crate::managed_agents::effective_config::EffectiveConfigResult::OrphanedInstance {
            record_pubkey,
            missing_persona_id,
        } => {
            eprintln!(
                "orphaned agent instance: pubkey={record_pubkey}, missing_persona_id={missing_persona_id}"
            );
            (None, None, None, None)
        }
    }
}

/// The runtime the summary reports: the effective harness/runtime id and the
/// tier it came from (`runtime_source`), same shape as `model`/`model_source`
/// above.
///
/// Reuses `resolve_effective_config` rather than `resolve_effective_runtime_id`
/// so the source tier travels with the value — `resolve_effective_runtime_id`
/// deliberately drops it, since its own callers (spawn, the harness
/// descriptor) only ever needed the winning id. An orphaned link reports both
/// as absent; `summary_effective_fields` above already logs that case once,
/// so this does not log again.
pub(super) fn summary_effective_runtime(
    record: &ManagedAgentRecord,
    personas: &[crate::managed_agents::types::AgentDefinition],
    global_config: &crate::managed_agents::GlobalAgentConfig,
) -> (
    Option<String>,
    Option<crate::managed_agents::effective_config::ConfigSource>,
) {
    match crate::managed_agents::effective_config::resolve_effective_config(
        record,
        personas,
        global_config,
    ) {
        crate::managed_agents::effective_config::EffectiveConfigResult::Resolved(cfg) => {
            (cfg.runtime.value, Some(cfg.runtime.source))
        }
        crate::managed_agents::effective_config::EffectiveConfigResult::OrphanedInstance {
            ..
        } => (None, None),
    }
}

#[cfg(test)]
mod tests;
