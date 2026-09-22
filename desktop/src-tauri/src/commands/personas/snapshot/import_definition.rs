//! The definition an imported agent snapshot becomes.
//!
//! Split out of `import.rs` when that file was at the repository's 1000-line
//! ceiling. It is a projection and nothing else: what a snapshot's declared
//! fields mean as an `AgentDefinition`, with every machine-local and lineage
//! field explicitly `None` so an import can never carry one in.

use crate::managed_agents::AgentDefinition;

use crate::managed_agents::agent_snapshot::AgentSnapshotDefinition;

/// Everything `imported_definition` needs, named rather than positional —
/// eight same-typed strings in a row is a bug waiting to be introduced.
pub(super) struct ImportedDefinition<'a> {
    pub(super) persona_id: String,
    pub(super) display_name: String,
    pub(super) effective_avatar: Option<String>,
    pub(super) definition: &'a AgentSnapshotDefinition,
    pub(super) respond_to_wire: Option<String>,
    pub(super) respond_to_allowlist: Vec<String>,
    pub(super) parallelism: Option<u32>,
    pub(super) now: String,
}

pub(super) fn imported_definition(input: ImportedDefinition<'_>) -> AgentDefinition {
    AgentDefinition {
        id: input.persona_id,
        display_name: input.display_name,
        avatar_url: input.effective_avatar,
        system_prompt: input.definition.system_prompt.clone().unwrap_or_default(),
        runtime: input.definition.runtime.clone(),
        model: input.definition.model.clone(),
        provider: input.definition.provider.clone(),
        name_pool: input.definition.name_pool.clone(),
        is_builtin: false,
        is_active: true,
        shared: false,
        source_team: None,
        source_team_persona_slug: None,
        catalog_source: None,
        env_vars: std::collections::BTreeMap::new(),
        respond_to: input.respond_to_wire,
        respond_to_allowlist: input.respond_to_allowlist,
        parallelism: input.parallelism,
        created_at: input.now.clone(),
        updated_at: input.now,
    }
}
