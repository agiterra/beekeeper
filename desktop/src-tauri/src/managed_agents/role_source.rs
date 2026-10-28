//! A role agent's shared definition carries instructions only; its runtime,
//! model, provider and behaviour are this machine's agent configuration
//! (ledger 400–401). Pure rules, so the commands that apply them stay thin.

use serde::Serialize;

use super::crew_roles::{DiscoveredRolePack, CREW_ROLE_ID_PREFIX};
use super::persona_events::{persona_content_hash, persona_event_content};
use super::types::{AgentDefinition, ManagedAgentRecord};

/// True for a definition minted by the role-pack installer.
pub(crate) fn is_role_definition(definition: &AgentDefinition) -> bool {
    definition.id.starts_with(CREW_ROLE_ID_PREFIX)
}

/// Where an agent's pin stands against its definition as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum SourceState {
    /// The agent was never pinned to a definition.
    Missing,
    /// The pin is the definition's current content hash.
    Current,
    /// The definition changed after the agent was pinned.
    Diverged,
}

/// The pin state of a linked agent; `None` when it links no definition.
pub(crate) fn source_state(
    record: &ManagedAgentRecord,
    definition: Option<&AgentDefinition>,
) -> Option<SourceState> {
    let definition = definition?;
    let current = persona_content_hash(&persona_event_content(definition));
    Some(match record.persona_source_version.as_deref() {
        None => SourceState::Missing,
        Some(pin) if pin == current => SourceState::Current,
        Some(_) => SourceState::Diverged,
    })
}

const EXECUTION_FIELDS_MESSAGE: &str = "a role's runtime, model, provider and who can instruct it are set per agent in Provider & settings; the shared role holds its instructions only";

/// Refuse a definition edit that would put execution settings on a role.
pub(crate) fn refuse_role_execution_edit(
    source: &AgentDefinition,
    prospective: &AgentDefinition,
) -> Result<(), String> {
    if !is_role_definition(source) {
        return Ok(());
    }
    let changed = source.runtime != prospective.runtime
        || source.model != prospective.model
        || source.provider != prospective.provider
        || source.respond_to != prospective.respond_to
        || source.respond_to_allowlist != prospective.respond_to_allowlist
        || source.parallelism != prospective.parallelism;
    if changed {
        return Err(EXECUTION_FIELDS_MESSAGE.into());
    }
    Ok(())
}

/// Prefix of the refusal that names edited instructions, so a caller can offer
/// the explicit confirm instead of parsing prose.
pub(crate) const INSTRUCTIONS_EDITED: &str = "ROLE_INSTRUCTIONS_EDITED";

/// Execution settings found on the definition, moved to the agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MovedExecution {
    pub runtime: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
}

impl MovedExecution {
    pub(crate) fn is_empty(&self) -> bool {
        self.runtime.is_none() && self.model.is_none() && self.provider.is_none()
    }
}

/// What re-applying a role's pack does, decided before anything is written.
#[derive(Debug, Clone)]
pub(crate) struct ReapplyPlan {
    pub definition: AgentDefinition,
    pub pin: String,
    pub moved: MovedExecution,
    /// Behaviour fields the definition carried and the restore clears.
    pub cleared: Vec<&'static str>,
    /// Instruction fields that differed from the pack and are replaced.
    pub replaced: Vec<&'static str>,
}

/// Restore `current` to what `pack` installs, as the installer writes it.
/// Instruction edits are refused unless `discard_instruction_edits`; execution
/// settings on the definition are returned to be applied to the agent.
pub(crate) fn plan_reapply(
    current: &AgentDefinition,
    pack: &DiscoveredRolePack,
    discard_instruction_edits: bool,
    now: &str,
) -> Result<ReapplyPlan, String> {
    if !is_role_definition(current) {
        return Err("only a role's definition can be re-applied from its pack".into());
    }
    let mut restored = current.clone();
    restored.system_prompt = pack.system_prompt.clone();
    restored.avatar_url = pack.avatar_url.clone();
    restored.runtime = pack.runtime.clone();
    restored.model = pack.model.clone();
    restored.provider = pack.provider.clone();
    restored.name_pool = Vec::new();
    restored.respond_to = None;
    restored.respond_to_allowlist = Vec::new();
    restored.parallelism = None;
    restored.updated_at = now.to_string();

    let mut replaced = Vec::new();
    if current.system_prompt != restored.system_prompt {
        replaced.push("instructions");
    }
    if current.avatar_url != restored.avatar_url {
        replaced.push("avatar");
    }
    if current.name_pool != restored.name_pool {
        replaced.push("name pool");
    }
    if !replaced.is_empty() && !discard_instruction_edits {
        return Err(format!(
            "{INSTRUCTIONS_EDITED}: this role's {} were edited since its pack was installed; re-applying the pack replaces them (a recovery copy is kept)",
            replaced.join(", ")
        ));
    }

    let differs = |now: &Option<String>, pack: &Option<String>| {
        now.as_ref()
            .filter(|value| Some(*value) != pack.as_ref())
            .cloned()
    };
    let moved = MovedExecution {
        runtime: differs(&current.runtime, &pack.runtime),
        model: differs(&current.model, &pack.model),
        provider: differs(&current.provider, &pack.provider),
    };
    let mut cleared = Vec::new();
    if current.respond_to.is_some() || !current.respond_to_allowlist.is_empty() {
        cleared.push("who can instruct");
    }
    if current.parallelism.is_some() {
        cleared.push("parallelism");
    }
    let pin = persona_content_hash(&persona_event_content(&restored));
    Ok(ReapplyPlan {
        definition: restored,
        pin,
        moved,
        cleared,
        replaced,
    })
}

#[cfg(test)]
#[path = "role_source_tests.rs"]
pub(crate) mod tests;
