//! Historical projection/redaction regression fixtures; excluded from product execution.
//!
//! This is a desired configuration snapshot. The desktop neither launches an
//! executor nor holds a running-process stamp. Accepted host revision and relay
//! execution evidence determine what actually runs. A projected difference must
//! not be presented as proof that a host runtime applied a change.

use std::collections::BTreeMap;

use serde::Serialize;

use super::super::{
    effective_config::{resolve_effective_config, EffectiveConfigResult},
    known_acp_runtime, normalize_agent_args,
    persona_events::preview_prospective_persona_snapshot,
    readiness::EffectiveHarnessDescriptor,
    runtime::{resolve_session_title, SESSION_TITLE_ENV_VAR},
    types::{AgentDefinition, ManagedAgentRecord, TeamRecord},
    AcpSessionPolicy, GlobalAgentConfig,
};

use super::{diff, effective_team_instructions};

/// The already-resolved values a spawn feeds into its `Command`.
///
/// Taking them rather than re-resolving is what makes the stamp describe the
/// process that was actually launched: a persona/harness/global edit landing
/// between spawn's resolution and the stamp can no longer suppress the badge.
pub(crate) struct SpawnConfigInputs<'a> {
    pub record: &'a ManagedAgentRecord,
    pub descriptor: &'a EffectiveHarnessDescriptor,
    /// Resolved workspace/pair relay — never the record's legacy pin.
    pub relay_url: &'a str,
    pub team_instructions: Option<&'a str>,
    pub system_prompt: Option<&'a str>,
    pub model: Option<&'a str>,
    pub provider: Option<&'a str>,
    /// Compile-time distribution capability projected at this runtime boundary.
    /// The stored record remains portable; only effective spawned access is stamped.
    pub enforced_owner_only: bool,
    /// The policy applied at launch and shown in restart-required comparisons.
    pub session_policy: AcpSessionPolicy,
}

/// The effective spawn configuration of one managed-agent process.
///
/// Serialization invariants (load-bearing — the drift comparison and the diff
/// walk both read `canonical()`):
/// - plain derived `Serialize`: no `flatten`, no `skip_serializing_if`, no
///   custom or fallible field serializers, no colliding serialized names, so
///   every field is always present on both sides of a comparison;
/// - `Option::None` serializes as JSON `null`; a *missing* key is reserved for
///   dynamic-map membership (`env.<KEY>` added/removed);
/// - arrays are atomic leaves — `args` and `respond_to_allowlist` compare and
///   render whole, never element-wise.
///
/// `Debug` is implemented by hand to redact environment values, auth tags and arguments.
#[derive(Clone, Serialize)]
pub(crate) struct SpawnConfigSnapshot {
    /// Desired ACP harness binary for the headless execution boundary.
    pub acp_command: String,
    /// The effective agent command the harness drives.
    pub command: String,
    pub args: Vec<String>,
    /// Catalog-derived from `command`; `""` when the runtime has none.
    pub mcp_command: String,
    /// Fully layered process env: baked floor -> runtime metadata ->
    /// definition -> global -> persona -> agent.
    pub env: BTreeMap<String, String>,
    pub relay_url: String,
    pub team_instructions: Option<String>,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
    pub provider: Option<String>,
    /// `None` when a user env override shadows `BEEKEEPER_ACP_SESSION_TITLE`: spawn
    /// writes the title BEFORE the user env layer, so the override is what
    /// actually runs and it already reaches this snapshot through `env`.
    /// Capturing the record-derived value under an override would badge a
    /// rename that changes nothing.
    pub session_title: Option<String>,
    pub auth_tag: Option<String>,
    pub respond_to: String,
    /// `None` outside allowlist mode — spawn sets
    /// `BEEKEEPER_ACP_RESPOND_TO_ALLOWLIST` only there, so edits to a dormant list
    /// must not badge. Normalized (trim/lowercase/dedup) as the env receives
    /// it, so edits that don't survive normalization must not badge either.
    pub respond_to_allowlist: Option<Vec<String>>,
    pub idle_timeout_seconds: Option<u64>,
    pub max_turn_duration_seconds: Option<u64>,
    pub parallelism: u32,
    /// The effective ACP session policy set directly on the launch command.
    pub session_policy: String,
}

impl SpawnConfigSnapshot {
    /// Assemble the snapshot from values a spawn has already resolved.
    pub(crate) fn from_inputs(inputs: SpawnConfigInputs<'_>) -> Self {
        let SpawnConfigInputs {
            record,
            descriptor,
            relay_url,
            team_instructions,
            system_prompt,
            model,
            provider,
            enforced_owner_only,
            session_policy,
        } = inputs;
        let (respond_to, respond_to_allowlist) =
            super::super::projected_access_with_policy(record, enforced_owner_only);
        Self {
            acp_command: comparable_command_name(&record.acp_command),
            command: comparable_command_name(&descriptor.command),
            args: descriptor.args.clone(),
            mcp_command: known_acp_runtime(&descriptor.command)
                .and_then(|runtime| runtime.mcp_command)
                .unwrap_or("")
                .to_string(),
            env: descriptor.env.clone(),
            relay_url: relay_url.to_string(),
            team_instructions: team_instructions.map(str::to_string),
            system_prompt: system_prompt.map(str::to_string),
            model: model.map(str::to_string),
            provider: provider.map(str::to_string),
            session_title: (!beekeeper_core_pkg::env_compat::both_spellings(SESSION_TITLE_ENV_VAR)
                .iter()
                .any(|key| descriptor.env.contains_key(key)))
            .then(|| resolve_session_title(record.display_name.as_deref(), &record.name))
            .flatten(),
            auth_tag: record.auth_tag.clone(),
            respond_to: respond_to.as_str().to_string(),
            respond_to_allowlist: (respond_to == super::super::types::RespondTo::Allowlist).then(
                || {
                    // A list spawn would reject is captured raw: the stamped
                    // snapshot comes from a successful spawn, so any invalid
                    // edit correctly compares unequal.
                    super::super::types::validate_respond_to_allowlist(&respond_to_allowlist)
                        .unwrap_or(respond_to_allowlist)
                },
            ),
            idle_timeout_seconds: record.idle_timeout_seconds,
            max_turn_duration_seconds: record.max_turn_duration_seconds,
            // Hash the effective parallelism so over-cap edits that don't change
            // the running pool size (e.g. 10 → 8, both clamp to 5 on OpenClaw)
            // do not raise a spurious "restart required" badge. Cap crossings
            // (e.g. 8 → 3, where 3 is below the cap) do change the effective
            // pool and must badge. The diff surface consequently displays the
            // effective value — that is correct, it is what actually runs.
            parallelism: super::super::effective_parallelism(
                &descriptor.command,
                record.parallelism,
            ),
            session_policy: session_policy.as_str().to_string(),
        }
    }

    /// Canonical JSON projection — the single representation both the drift
    /// comparison and the diff walk read, so a lit badge always has a
    /// non-empty diff and vice versa.
    ///
    /// Infallible by the serialization invariants documented on the struct
    /// (plain derive over strings, scalars, string maps, and string vectors);
    /// a failure here is a broken invariant, never a runtime condition, so it
    /// must not degrade into an empty diff.
    pub(crate) fn canonical(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("SpawnConfigSnapshot serializes infallibly")
    }
}

/// A command name as the restart comparison sees it: a bare pre-rename
/// `buzz-<x>` reads as `beekeeper-<x>`, which is what `resolve_command` falls
/// back to. Without this, the boot migration rewriting a stored
/// `buzz-acp` to `beekeeper-acp` would light "restart required" on an agent
/// whose launch would not change.
pub(crate) fn comparable_command_name(command: &str) -> String {
    super::super::discovery::renamed_commands::renamed_bundled_command(command)
        .unwrap_or_else(|| command.to_string())
}

impl std::fmt::Debug for SpawnConfigSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "SpawnConfigSnapshot({})",
            diff::redacted_canonical(&self.canonical())
        )
    }
}

/// Snapshot the effective spawn configuration `record` would get if it were
/// started right now under the current `personas`/`teams`/`global`, resolving
/// a blank record relay against `workspace_relay`.
///
/// Pure — no `AppHandle`, no disk, no keyring. This is the *prospective* side
/// of the comparison; the stamped side is built at spawn from the values that
/// actually fed the child's `Command`.
pub(crate) fn prospective_spawn_config_snapshot(
    record: &ManagedAgentRecord,
    personas: &[AgentDefinition],
    teams: &[TeamRecord],
    workspace_relay: &str,
    global: &GlobalAgentConfig,
    enforced_owner_only: bool,
    session_policy: AcpSessionPolicy,
) -> SpawnConfigSnapshot {
    // Prospective re-snapshot: apply the same `apply_persona_snapshot` the
    // start/restore paths run right before spawning, so this describes what a
    // restart would actually run. Idempotent, so a spawn-time stamp taken
    // after those paths saved the record compares equal when nothing changed.
    // The persona env itself arrives through the descriptor's layered env
    // below; `persona_source_version` is set on the clone but is not an input.
    let record = preview_prospective_persona_snapshot(record, personas);
    let record = &record;

    // Resolve desired command, arguments and environment through the typed
    // management descriptor. This projection does not authorize execution.
    let descriptor =
        crate::managed_agents::resolve_effective_harness_descriptor(record, personas, global)
            .unwrap_or_else(|_| {
                let command = crate::managed_agents::record_agent_command(record, personas);
                let args = normalize_agent_args(&command, record.agent_args.clone());
                EffectiveHarnessDescriptor {
                    command,
                    args,
                    env: Default::default(),
                }
            });

    // Desired prompt/model/provider use one resolver. A missing definition
    // cannot produce an applied-state claim or authorize a runtime fallback.
    let (prompt, model, provider) = match resolve_effective_config(record, personas, global) {
        EffectiveConfigResult::Resolved(cfg) => {
            (cfg.system_prompt.value, cfg.model.value, cfg.provider.value)
        }
        EffectiveConfigResult::OrphanedInstance { .. } => (None, None, None),
    };

    SpawnConfigSnapshot::from_inputs(SpawnConfigInputs {
        record,
        descriptor: &descriptor,
        // Project the caller's canonical community, not a process receipt.
        relay_url: &crate::relay::effective_agent_relay_url(&record.relay_url, workspace_relay),
        team_instructions: effective_team_instructions(record, teams).as_deref(),
        system_prompt: prompt.as_deref(),
        model: model.as_deref(),
        provider: provider.as_deref(),
        enforced_owner_only,
        session_policy,
    })
}
