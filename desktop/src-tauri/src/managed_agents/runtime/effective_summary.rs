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

#[cfg(test)]
mod tests;
