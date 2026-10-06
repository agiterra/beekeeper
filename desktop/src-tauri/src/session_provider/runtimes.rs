//! The host-side runtime table for the coding-session provider.
//!
//! One static table names the runtimes this desktop can offer to the sidecar
//! (`BUZZ_CSP_RUNTIMES`) and to the frontend picker
//! (`coding_session_provider_runtimes`). The table is deliberately small and
//! v1-conservative: claude is always offered (today's zero-config default),
//! codex and goose only when their adapter binaries actually resolve.
//! `buzz-agent` is deliberately absent until its API-key env wiring is proven.

use std::path::PathBuf;

use beekeeper_core_pkg::coding_session_payload::Capabilities;
use beekeeper_core_pkg::coding_session_runtime::{CliEnvVar, RuntimeDescriptor, SteerIdleGuard};

use crate::managed_agents::{
    known_acp_runtime_exact, probe_auth_status, resolve_command, AuthStatus,
};
use crate::session_provider::status::resolve_claude_code_executable;

/// One row of the host runtime table.
struct HostRuntime {
    /// Id in `KNOWN_ACP_RUNTIMES` (label, underlying CLI, auth probe).
    runtime_id: &'static str,
    /// `providerInstanceRef` advertised for this runtime.
    instance_ref: &'static str,
    /// Driver slug minted into `cs-target`s.
    driver: &'static str,
    /// Adapter commands, first resolved wins.
    adapter_commands: &'static [&'static str],
    /// Adapter argv after the command.
    agent_args: &'static [&'static str],
    /// Whether the sidecar probes the adapter's live model catalog at startup.
    discover_models: bool,
    /// Whether the runtime is offered even when its adapter does not resolve.
    /// Only claude: falling back to the bare command preserves today's
    /// PATH-lookup behavior for the zero-config default.
    always_offered: bool,
    /// The idle guard the pinned adapter honours on `_session/steering`.
    ///
    /// A declared fact about the installed package — no adapter advertises
    /// it — recorded here because this host is what installs and pins the
    /// adapter. The sidecar offers native mid-turn injection only for a
    /// runtime that declares [`SteerIdleGuard::PromptRequired`]; `None` keeps
    /// every steer at the boundary.
    steer_idle_guard: Option<SteerIdleGuard>,
}

/// The v1 table. Order here is the descriptor order handed to the sidecar;
/// the sidecar's catalog sorts by instance ref regardless.
const HOST_RUNTIMES: &[HostRuntime] = &[
    HostRuntime {
        runtime_id: "claude",
        instance_ref: "claude-primary",
        driver: "claude-agent-acp",
        adapter_commands: &["claude-agent-acp", "claude-code-acp"],
        agent_args: &[],
        discover_models: true,
        always_offered: true,
        // claude-agent-acp 0.70.0, dist/acp-agent.js:1146-1150 honours
        // `_meta.steering.idleBehavior: promptRequired` — with no running
        // turn it answers `{outcome:"promptRequired"}` and leaves the content
        // with the caller; while a turn runs it pushes the input into the
        // same SDK stream and answers `injected` (:1160-1184). Verified
        // 2026-09-11 against the installed package.
        steer_idle_guard: Some(SteerIdleGuard::PromptRequired),
    },
    HostRuntime {
        runtime_id: "codex",
        instance_ref: "codex-primary",
        driver: "codex-acp",
        adapter_commands: &["codex-acp"],
        agent_args: &[],
        // codex-acp answers the same ACP model probe claude-agent-acp does.
        // Verified 2026-08-24 against codex-acp 1.6.2: `buzz-acp models --json`
        // returns a `model` config option whose `currentValue` is the real
        // model (`gpt-5.6-terra`) and whose options list every selectable one.
        // Left `false`, every Codex execution rendered as `Codex · default` —
        // the "default label hiding the real model" bug (§2 item 39).
        discover_models: true,
        always_offered: false,
        // codex-acp 1.6.2 has no idle guard (dist/index.js:31387-31400 starts
        // a detached turn when no turn is steerable, and answers
        // `startedNewTurn` only once that turn is running). A steer racing a
        // turn end would start a native turn nobody observes, so codex stays
        // boundary-only until it honours an idle guard.
        steer_idle_guard: None,
    },
    HostRuntime {
        runtime_id: "goose",
        instance_ref: "goose-primary",
        driver: "goose-acp",
        adapter_commands: &["goose"],
        agent_args: &["acp"],
        // Deliberately not opted in: the same probe against goose
        // answers `-32603 Internal error` (tested 2026-08-24), so discovery
        // would spend its timeout to learn nothing. Flip it when goose answers.
        discover_models: false,
        always_offered: false,
        // Not installed here; nothing verified, so nothing declared.
        steer_idle_guard: None,
    },
];

/// Auth readiness of one runtime, as reported to the frontend picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CodingSessionRuntimeAuthState {
    /// Adapter and CLI resolve, and the auth probe (if any) did not report a
    /// signed-out state.
    Ready,
    /// Installed but the CLI's auth probe reports signed out or misconfigured.
    NeedsAuth,
    /// The adapter or its underlying CLI is not installed.
    Missing,
}

/// One runtime row of the picker contract.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodingSessionProviderRuntime {
    /// `providerInstanceRef` a create names.
    pub instance_ref: String,
    /// Runtime slug ("claude", "codex", "goose").
    pub runtime: String,
    /// Driver slug — needed for the pre-catalog bootstrap target.
    pub driver: String,
    /// Human label from the managed-agent registry ("Claude Code").
    pub label: String,
    /// Install/sign-in readiness.
    pub auth_state: CodingSessionRuntimeAuthState,
    /// Static default model alias; live models come from the models command.
    pub default_model: String,
    /// Static allowed models.
    pub allowed_models: Vec<String>,
    /// v1 capability vector for the runtime.
    pub capabilities: Capabilities,
}

/// Fail-closed auth state for readiness diagnostics.
///
/// The picker intentionally treats an inconclusive probe optimistically; a
/// launch gate may not. This parallel contract preserves Unknown exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StrictRuntimeAuthState {
    Ready,
    NeedsAuth,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StrictRuntimeDiagnostic {
    pub instance_ref: String,
    pub runtime: String,
    pub label: String,
    pub adapter_path: Option<String>,
    /// Version probing is not yet a side-effect-free contract for every
    /// adapter. Absence is explicit rather than guessed from a filename.
    pub adapter_version: Option<String>,
    pub auth: StrictRuntimeAuthState,
    /// Live model discovery is provider/wire work, never this local inventory.
    pub model_probe: String,
}

/// Resolve the first installed adapter command for a table row.
fn resolve_adapter(runtime: &HostRuntime) -> Option<PathBuf> {
    runtime
        .adapter_commands
        .iter()
        .find_map(|command| resolve_command(command))
}

/// Whether the models command should treat `instance_ref` as a known runtime,
/// and whether that runtime does live model discovery.
pub(crate) fn known_instance_ref(instance_ref: &str) -> Option<bool> {
    HOST_RUNTIMES
        .iter()
        .find(|runtime| runtime.instance_ref == instance_ref)
        .map(|runtime| runtime.discover_models)
}

/// What the models command needs to probe one runtime's own adapter.
///
/// The probe is driver-agnostic — `buzz-acp models --json` drives whatever
/// `BUZZ_ACP_AGENT_COMMAND` names — but the desktop command used to resolve
/// `claude-agent-acp` by name whatever runtime it was asked about, so opting a
/// second runtime into discovery would have reported Claude's models under its
/// label (§2 item 39).
#[derive(Debug)]
pub(crate) struct RuntimeProbeTarget {
    pub label: &'static str,
    pub agent_command: PathBuf,
    pub agent_args: Vec<String>,
    pub needs_claude_executable: bool,
}

/// Resolve one runtime's adapter for the model probe, or say which is missing.
pub(crate) fn runtime_probe_target(instance_ref: &str) -> Result<RuntimeProbeTarget, String> {
    let runtime = HOST_RUNTIMES
        .iter()
        .find(|runtime| runtime.instance_ref == instance_ref)
        .ok_or_else(|| format!("unknown coding-session runtime instanceRef: {instance_ref}"))?;
    let agent_command = resolve_adapter(runtime).ok_or_else(|| {
        format!(
            "the {} ACP adapter is not installed ({})",
            runtime.runtime_id,
            runtime.adapter_commands.join(" or ")
        )
    })?;
    Ok(RuntimeProbeTarget {
        label: runtime.runtime_id,
        agent_command,
        agent_args: runtime
            .agent_args
            .iter()
            .map(|arg| arg.to_string())
            .collect(),
        needs_claude_executable: runtime.runtime_id == "claude",
    })
}

/// Build the descriptor list written to `BUZZ_CSP_RUNTIMES` for one spawn.
///
/// Claude is always included — with the bare command name when the adapter did
/// not resolve, preserving the sidecar's own PATH fallback. Codex and goose are
/// included only when their adapter resolves. Auth is deliberately *not*
/// checked here: a signed-out runtime still advertises, and a create against it
/// fails through the existing `PROVIDER_AUTH_REQUIRED` receipt path.
pub(crate) fn build_runtime_descriptors() -> Vec<RuntimeDescriptor> {
    HOST_RUNTIMES
        .iter()
        .filter_map(|runtime| {
            let resolved = resolve_adapter(runtime);
            let agent_command = match (&resolved, runtime.always_offered) {
                (Some(path), _) => path.to_string_lossy().into_owned(),
                (None, true) => runtime.adapter_commands[0].to_string(),
                (None, false) => return None,
            };
            let cli_env = (runtime.runtime_id == "claude")
                .then(resolve_claude_code_executable)
                .flatten()
                .map(|path| CliEnvVar {
                    name: "CLAUDE_CODE_EXECUTABLE".to_string(),
                    value: path.to_string_lossy().into_owned(),
                });
            Some(RuntimeDescriptor {
                steer_idle_guard: runtime.steer_idle_guard,
                title_model: None,
                instance_ref: runtime.instance_ref.to_string(),
                driver: runtime.driver.to_string(),
                runtime: runtime.runtime_id.to_string(),
                agent_command,
                agent_args: runtime
                    .agent_args
                    .iter()
                    .map(|arg| arg.to_string())
                    .collect(),
                cli_env,
                default_model: "default".to_string(),
                allowed_models: vec!["default".to_string()],
                discover_models: runtime.discover_models,
                capabilities: None,
            })
        })
        .collect()
}

/// Classify one runtime's install/auth readiness from resolution facts and an
/// optional probe result. Pure, so the mapping is unit-testable.
///
/// Optimistic on `Unknown`, matching managed-agent behavior: a wrong "ready"
/// fails later through the `PROVIDER_AUTH_REQUIRED` receipt, while a wrong
/// "needs_auth" hides a working runtime from the picker.
fn classify_auth_state(
    adapter_resolved: bool,
    cli_resolved: Option<bool>,
    probe: Option<AuthStatus>,
) -> CodingSessionRuntimeAuthState {
    if !adapter_resolved || cli_resolved == Some(false) {
        return CodingSessionRuntimeAuthState::Missing;
    }
    match probe {
        Some(AuthStatus::LoggedOut) | Some(AuthStatus::ConfigInvalid { .. }) => {
            CodingSessionRuntimeAuthState::NeedsAuth
        }
        _ => CodingSessionRuntimeAuthState::Ready,
    }
}

/// Static runtime metadata for the side-effect-free readiness inventory.
///
/// This performs no PATH lookup and runs no adapter, login shell, auth CLI,
/// or model probe. Install/auth/model state remains Unknown until an already
/// trusted provider-signed catalog is folded into the local report.
pub(crate) fn runtime_readiness_metadata() -> Vec<StrictRuntimeDiagnostic> {
    let mut rows = HOST_RUNTIMES
        .iter()
        .map(|runtime| {
            let registry = known_acp_runtime_exact(runtime.runtime_id);
            StrictRuntimeDiagnostic {
                instance_ref: runtime.instance_ref.to_string(),
                runtime: runtime.runtime_id.to_string(),
                label: registry
                    .map(|entry| entry.label.to_string())
                    .unwrap_or_else(|| runtime.runtime_id.to_string()),
                adapter_path: None,
                adapter_version: None,
                auth: StrictRuntimeAuthState::Unknown,
                model_probe: "unobserved".into(),
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|left, right| left.instance_ref.cmp(&right.instance_ref));
    rows
}

/// The full picker list: one entry per table row, installed or not, sorted by
/// instance ref. Runs the fast CLI auth probes but never spawns ACP adapters.
pub(crate) fn list_runtimes() -> Vec<CodingSessionProviderRuntime> {
    let mut runtimes: Vec<CodingSessionProviderRuntime> = HOST_RUNTIMES
        .iter()
        .map(|runtime| {
            let registry = known_acp_runtime_exact(runtime.runtime_id);
            let adapter_resolved = resolve_adapter(runtime).is_some();
            let cli_resolved = registry
                .and_then(|entry| entry.underlying_cli)
                .map(|cli| resolve_command(cli).is_some());
            let probe = if adapter_resolved && cli_resolved != Some(false) {
                registry
                    .and_then(|entry| entry.auth_probe_args)
                    .and_then(|args| {
                        let binary = resolve_command(args[0])?;
                        Some(probe_auth_status(&binary, args))
                    })
            } else {
                None
            };
            CodingSessionProviderRuntime {
                instance_ref: runtime.instance_ref.to_string(),
                runtime: runtime.runtime_id.to_string(),
                driver: runtime.driver.to_string(),
                label: registry
                    .map(|entry| entry.label.to_string())
                    .unwrap_or_else(|| runtime.runtime_id.to_string()),
                auth_state: classify_auth_state(adapter_resolved, cli_resolved, probe),
                default_model: "default".to_string(),
                allowed_models: vec!["default".to_string()],
                capabilities: Capabilities::v1_for_runtime(runtime.runtime_id),
            }
        })
        .collect();
    runtimes.sort_by(|left, right| left.instance_ref.cmp(&right.instance_ref));
    runtimes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_table_row_names_a_registry_runtime() {
        for runtime in HOST_RUNTIMES {
            assert!(
                known_acp_runtime_exact(runtime.runtime_id).is_some(),
                "{} is missing from KNOWN_ACP_RUNTIMES",
                runtime.runtime_id
            );
            assert_eq!(
                runtime.instance_ref,
                format!("{}-primary", runtime.runtime_id),
                "instance refs follow the <runtime>-primary convention"
            );
        }
    }

    #[test]
    fn auth_state_classification_matches_the_contract() {
        use CodingSessionRuntimeAuthState::*;
        // Adapter missing, or adapter present but the underlying CLI absent.
        assert_eq!(classify_auth_state(false, None, None), Missing);
        assert_eq!(classify_auth_state(true, Some(false), None), Missing);
        // Signed out or misconfigured probes surface as needs_auth.
        assert_eq!(
            classify_auth_state(true, Some(true), Some(AuthStatus::LoggedOut)),
            NeedsAuth
        );
        assert_eq!(
            classify_auth_state(
                true,
                Some(true),
                Some(AuthStatus::ConfigInvalid {
                    diagnostic: "bad".into()
                })
            ),
            NeedsAuth
        );
        // Logged in, unknown, not-applicable, and probe-less are all ready.
        assert_eq!(
            classify_auth_state(true, Some(true), Some(AuthStatus::LoggedIn)),
            Ready
        );
        assert_eq!(
            classify_auth_state(true, Some(true), Some(AuthStatus::Unknown)),
            Ready
        );
        assert_eq!(classify_auth_state(true, None, None), Ready);
    }

    #[test]
    fn readiness_metadata_never_resolves_or_probes() {
        let rows = runtime_readiness_metadata();
        assert_eq!(rows.len(), HOST_RUNTIMES.len());
        assert!(rows.iter().all(|row| {
            row.adapter_path.is_none()
                && row.adapter_version.is_none()
                && row.auth == StrictRuntimeAuthState::Unknown
                && row.model_probe == "unobserved"
        }));
    }

    /// Discovery is per adapter-capability, not per favourite runtime.
    ///
    /// This assertion used to read `discover_models == (runtime_id ==
    /// "claude")`, which is how every Codex execution came to render as
    /// `Codex · default` (§2 item 39): codex-acp answers the probe, it was
    /// simply never asked. goose stays out because it answers `-32603
    /// Internal error`, tested the same day.
    #[test]
    fn codex_discovers_models_with_claude_and_goose_does_not() {
        for runtime in HOST_RUNTIMES {
            assert_eq!(
                runtime.discover_models,
                runtime.runtime_id != "goose",
                "unexpected discovery policy for {}",
                runtime.runtime_id
            );
            assert_eq!(runtime.always_offered, runtime.runtime_id == "claude");
        }
        assert_eq!(known_instance_ref("claude-primary"), Some(true));
        assert_eq!(known_instance_ref("codex-primary"), Some(true));
        assert_eq!(known_instance_ref("goose-primary"), Some(false));
        assert_eq!(known_instance_ref("ghost-primary"), None);
    }

    /// Claude survives an unresolved adapter as the bare command name — the
    /// sidecar's own PATH fallback stays in charge, exactly today's behavior.
    #[test]
    fn descriptors_always_include_claude() {
        let descriptors = build_runtime_descriptors();
        let claude = descriptors
            .iter()
            .find(|descriptor| descriptor.instance_ref == "claude-primary")
            .expect("claude descriptor is always present");
        assert_eq!(claude.driver, "claude-agent-acp");
        assert!(claude.discover_models);
        assert!(!claude.agent_command.is_empty());
        // Non-claude descriptors appear only with a resolved absolute command.
        for descriptor in &descriptors {
            if descriptor.instance_ref != "claude-primary" {
                assert!(std::path::Path::new(&descriptor.agent_command).is_absolute());
            }
        }
        // Goose, when offered, must carry its `acp` subcommand.
        if let Some(goose) = descriptors
            .iter()
            .find(|descriptor| descriptor.instance_ref == "goose-primary")
        {
            assert_eq!(goose.agent_args, vec!["acp".to_string()]);
        }
    }

    #[test]
    fn the_picker_lists_every_row_sorted_with_static_models() {
        let runtimes = list_runtimes();
        let refs: Vec<&str> = runtimes
            .iter()
            .map(|runtime| runtime.instance_ref.as_str())
            .collect();
        assert_eq!(
            refs,
            vec!["claude-primary", "codex-primary", "goose-primary"]
        );
        for runtime in &runtimes {
            assert_eq!(runtime.default_model, "default");
            assert_eq!(runtime.allowed_models, vec!["default".to_string()]);
            assert!(runtime.capabilities.thread_turn_start);
        }
        assert_eq!(
            runtimes[0].capabilities,
            Capabilities::v1_claude(),
            "claude advertises its v1 vector"
        );
    }
}
