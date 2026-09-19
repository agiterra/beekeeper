//! The runtime a crew install lands an identity on (ledger 165).
//!
//! Role packs are model- and runtime-agnostic, so a freshly installed record
//! pins no runtime and its stored harness resolves to the app's default —
//! `buzz-agent`, the chat harness — which no coding-session provider runs.
//! Every hire of such an identity was refused `HIRE_PROVIDER_NOT_ALLOWED`
//! naming it. The installers seed this computer's preferred runtime here,
//! the way creating an agent from a definition already does.

use super::CrewRoleInstall;
use crate::managed_agents::types::{AgentDefinition, ManagedAgentRecord};

/// Store the harness a spawn would resolve for `record`: `record_agent_command`
/// is the same resolver spawn and the summary use (pin → record runtime →
/// pack), and the args and MCP command follow the command.
pub(super) fn derive_harness_fields(
    record: &mut ManagedAgentRecord,
    definitions: &[AgentDefinition],
) {
    record.agent_command = crate::managed_agents::record_agent_command(record, definitions);
    record.agent_args =
        crate::managed_agents::normalize_agent_args(&record.agent_command, Vec::new());
    record.mcp_command = crate::managed_agents::known_acp_runtime(&record.agent_command)
        .and_then(|runtime| runtime.mcp_command)
        .unwrap_or("")
        .to_string();
}

/// Pin this computer's preferred runtime onto every identity this install
/// wrote that pins none, and re-derive its stored harness. Returns the roles
/// pinned.
///
/// Role packs are model- and runtime-agnostic by design, so a fresh record
/// resolves to the app's default harness — `buzz-agent`, the chat harness —
/// and no coding-session provider runs that: every hire of the identity was
/// refused `HIRE_PROVIDER_NOT_ALLOWED` naming it (ledger 165). Creating an
/// agent from a definition already seeds `preferred_runtime` onto the
/// instance (`commands/personas/snapshot.rs`); this is the same seed at the
/// same moment for an install. A runtime the pack declares or the host
/// seated earlier is non-blank here and is left alone; with no preference
/// nothing is pinned and the record stays honest about that.
///
/// Applied after the install rather than inside it because the installer is
/// pure and the preference is host configuration the callers hold.
pub(crate) fn pin_default_runtime(
    result: &mut CrewRoleInstall,
    default_runtime: Option<&str>,
) -> Vec<String> {
    let Some(default_runtime) = default_runtime
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Vec::new();
    };
    let mut pinned = Vec::new();
    for role in &result.installed {
        let Some(record) = result
            .agents
            .iter_mut()
            .find(|record| record.pubkey == role.agent_pubkey)
        else {
            continue;
        };
        if record
            .runtime
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            continue;
        }
        record.runtime = Some(default_runtime.to_string());
        derive_harness_fields(record, &result.definitions);
        pinned.push(role.role.clone());
    }
    pinned
}
