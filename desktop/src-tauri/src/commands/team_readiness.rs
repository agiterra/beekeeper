//! Side-effect-free LOCAL inventory for the Portable Team Loop.
//!
//! This command is R1: it reads public metadata and already-live in-memory
//! facts only. It never prepares a host. R2's explicit UI Prepare transaction
//! owns the named mutations, in order: install roles with confirmed names,
//! provision the provider, start it, then request a fresh readiness inventory.
//! Only that path may prompt, mint, publish, migrate, back up, or hydrate keys.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use tauri::AppHandle;

use crate::app_state::KEYCHAIN_UNAVAILABLE;
use crate::managed_agents::crew_roles::{
    project_role_packs_dir, role_pack_source_version, scan_role_packs,
};
use crate::managed_agents::ManagedAgentReadinessMetadata;
use crate::session_provider::runtimes::StrictRuntimeDiagnostic;
use crate::session_provider::supervisor::CodingSessionProviderProcessState;

const SCHEMA_VERSION: u32 = 3;
#[path = "team_readiness_git.rs"]
mod git_probe;
use git_probe::{checkout_source, embedded_source};
#[cfg(test)]
use git_probe::{parse_embedded_source, validate_dirty};
#[path = "team_readiness_host.rs"]
mod host;
use host::{AppReadinessHost, ReadinessHost};
#[path = "team_readiness_auth_facts.rs"]
mod auth_facts;
use auth_facts::{append_agent_auth_facts, append_provider_auth_fact};

#[path = "team_readiness_wire.rs"]
mod wire;
#[cfg(test)]
use wire::{fold_trusted_team_wire, TeamReadinessWireObservation, TrustedTeamCatalogSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReadinessStatus {
    Ready,
    AwaitingFirstSession,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReadinessHostClass {
    Cold,
    Partial,
    PreparedForFirstSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReadinessFactState {
    Ready,
    Limited,
    AwaitingFirstSession,
    Blocked,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReadinessScope {
    Local,
    Wire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessFact {
    pub category: String,
    pub code: String,
    pub scope: TeamReadinessScope,
    pub state: TeamReadinessFactState,
    pub summary: String,
    pub remedy: Option<String>,
}

impl TeamReadinessFact {
    fn local(
        category: &str,
        code: &str,
        state: TeamReadinessFactState,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            category: category.into(),
            code: code.into(),
            scope: TeamReadinessScope::Local,
            state,
            summary: summary.into(),
            remedy: None,
        }
    }

    fn blocked(category: &str, code: &str, summary: impl Into<String>, remedy: &str) -> Self {
        let mut fact = Self::local(category, code, TeamReadinessFactState::Blocked, summary);
        fact.remedy = Some(remedy.into());
        fact
    }

    fn unknown(category: &str, code: &str, summary: impl Into<String>, remedy: &str) -> Self {
        let mut fact = Self::local(category, code, TeamReadinessFactState::Unknown, summary);
        fact.remedy = Some(remedy.into());
        fact
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessSource {
    pub app_commit: Option<String>,
    pub app_source_dirty: Option<bool>,
    pub checkout_path: Option<String>,
    pub checkout_commit: Option<String>,
    pub checkout_dirty: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TeamReadinessKeyState {
    LiveProcess,
    InMemory,
    Unverified,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessIdentity {
    pub role: Option<String>,
    pub persona_name: Option<String>,
    pub name: String,
    pub pubkey: String,
    pub auth_tag_present: bool,
    pub profile_sync: String,
    pub source_revision: Option<String>,
    pub source_digest: Option<String>,
    pub key_state: TeamReadinessKeyState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessRolePack {
    pub role: String,
    pub persona_name: String,
    pub path: String,
    pub installed_pubkey: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessTeam {
    pub selected_roles: Vec<String>,
    pub available_roles: Vec<String>,
    pub packs_digest: Option<String>,
    pub packs_revision: Option<String>,
    pub packs: Vec<TeamReadinessRolePack>,
    pub identities: Vec<TeamReadinessIdentity>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessRegistryCoverage {
    pub provider_targets: Vec<String>,
    pub covered_targets: Vec<String>,
    pub uncovered_targets: Vec<String>,
    pub pending_targets: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessProvider {
    pub relay_url: String,
    pub provisioned: bool,
    pub provider_pubkey: Option<String>,
    pub instance_id: Option<String>,
    pub auth_tag_present: Option<bool>,
    pub key_state: Option<TeamReadinessKeyState>,
    pub process: String,
    pub child_pid: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessPolicy {
    pub hiring_policy: String,
    pub max_sessions: Option<usize>,
    pub turn_idle_timeout_secs: Option<u64>,
    pub turn_budget: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessCatalog {
    pub state: TeamReadinessFactState,
    pub revision: Option<u64>,
    pub targets: Vec<String>,
    pub source: TeamReadinessScope,
    pub provenance: Vec<wire::TeamReadinessCatalogProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessRelay {
    pub state: TeamReadinessFactState,
    pub reachable: Option<bool>,
    pub source: TeamReadinessScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamReadinessResponse {
    pub schema_version: u32,
    pub project_ref: String,
    pub generated_at: String,
    pub ready_for_first_session: bool,
    pub ready: bool,
    pub status: TeamReadinessStatus,
    pub host_class: TeamReadinessHostClass,
    pub key_store_safe: bool,
    pub source: TeamReadinessSource,
    pub owner_pubkey: Option<String>,
    pub team: TeamReadinessTeam,
    pub runtimes: Vec<StrictRuntimeDiagnostic>,
    pub registry: TeamReadinessRegistryCoverage,
    pub provider: TeamReadinessProvider,
    pub policy: TeamReadinessPolicy,
    pub relay: TeamReadinessRelay,
    pub catalog: TeamReadinessCatalog,
    pub facts: Vec<TeamReadinessFact>,
    pub blocking_codes: Vec<String>,
    pub unknown_codes: Vec<String>,
    pub awaiting_codes: Vec<String>,
    pub limited_codes: Vec<String>,
}

#[derive(Default)]
struct Gathered {
    source: TeamReadinessSource,
    owner_pubkey: Option<String>,
    key_store_safe: bool,
    team: TeamReadinessTeam,
    runtimes: Vec<StrictRuntimeDiagnostic>,
    registry: TeamReadinessRegistryCoverage,
    provider: TeamReadinessProvider,
    policy: TeamReadinessPolicy,
    facts: Vec<TeamReadinessFact>,
}

fn valid_project_ref(value: &str) -> bool {
    let mut parts = value.splitn(3, ':');
    parts.next() == Some("30621")
        && parts.next().is_some_and(|owner| {
            owner.len() == 64
                && owner
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
        && parts.next().is_some_and(|slug| !slug.trim().is_empty())
}

fn collect_identity(host: &impl ReadinessHost, gathered: &mut Gathered) {
    match host.owner_pubkey() {
        Ok(pubkey) => {
            gathered.key_store_safe = true;
            gathered.owner_pubkey = Some(pubkey);
            gathered.facts.push(TeamReadinessFact::local(
                "identity",
                "OWNER_KEY_IN_MEMORY",
                TeamReadinessFactState::Ready,
                "Owner signing key was loaded at boot",
            ));
        }
        Err(_) => gathered.facts.push(TeamReadinessFact::unknown(
            "identity",
            KEYCHAIN_UNAVAILABLE,
            "Key accessibility is unavailable or unverified",
            "Unlock the keychain and relaunch Beekeeper.",
        )),
    }
}

fn collect_checkout(
    host: &impl ReadinessHost,
    project_ref: &str,
    gathered: &mut Gathered,
) -> Option<PathBuf> {
    let store = match host.workdirs() {
        Ok(store) => store,
        Err(error) => {
            gathered.facts.push(TeamReadinessFact::unknown(
                "project",
                "CHECKOUT_STORE_UNREADABLE",
                error,
                "Choose the project checkout again.",
            ));
            return None;
        }
    };
    let Some(entry) = store.by_project.get(project_ref) else {
        gathered.facts.push(TeamReadinessFact::blocked(
            "project",
            "CHECKOUT_NOT_RECORDED",
            "No checkout is recorded for this project",
            "Choose a checkout for this project.",
        ));
        return None;
    };
    gathered.source.checkout_path = Some(entry.path.display().to_string());
    if !entry.path.is_absolute() || !entry.path.is_dir() {
        gathered.facts.push(TeamReadinessFact::blocked(
            "project",
            "CHECKOUT_UNAVAILABLE",
            "The recorded checkout is not an accessible absolute directory",
            "Restore or choose the checkout.",
        ));
        return None;
    }
    let path = entry.path.clone();
    match checkout_source(&path) {
        Ok((commit, dirty)) => {
            gathered.source.checkout_commit = Some(commit);
            gathered.source.checkout_dirty = Some(dirty);
            gathered.facts.push(TeamReadinessFact::local(
                "project",
                "CHECKOUT_READY",
                TeamReadinessFactState::Ready,
                "Checkout and Git source are readable",
            ));
        }
        Err(error) => gathered.facts.push(TeamReadinessFact::unknown(
            "project",
            "GIT_SOURCE_UNKNOWN",
            error,
            "Use a readable Git checkout.",
        )),
    }
    Some(path)
}

fn identity_projection(
    row: &ManagedAgentReadinessMetadata,
    live: &HashSet<String>,
) -> TeamReadinessIdentity {
    let source_is_digest = row.persona_source_version.as_ref().is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    });
    TeamReadinessIdentity {
        role: row.home_role.clone(),
        persona_name: row.persona_name_in_team.clone(),
        name: row.name.clone(),
        pubkey: row.pubkey.clone(),
        auth_tag_present: row.auth_tag_present,
        profile_sync: "unverified".into(),
        source_revision: (!source_is_digest)
            .then(|| row.persona_source_version.clone())
            .flatten(),
        source_digest: source_is_digest
            .then(|| row.persona_source_version.clone())
            .flatten(),
        key_state: if live.contains(&row.pubkey) {
            TeamReadinessKeyState::LiveProcess
        } else {
            TeamReadinessKeyState::Unverified
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectedRolePackState {
    Current,
    Dirty,
    SourceUnknown,
    WrongProject,
    Missing,
}

fn selected_role_pack_state(
    metadata: &[ManagedAgentReadinessMetadata],
    pack: &crate::managed_agents::crew_roles::DiscoveredRolePack,
) -> SelectedRolePackState {
    if let Some(row) = metadata.iter().find(|row| {
        row.persona_team_dir.as_deref() == Some(pack.dir.as_path())
            && row.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
    }) {
        return match row.persona_source_version.as_deref() {
            Some(source) if source == role_pack_source_version(pack, &row.name) => {
                SelectedRolePackState::Current
            }
            Some(_) => SelectedRolePackState::Dirty,
            None => SelectedRolePackState::SourceUnknown,
        };
    }
    if metadata.iter().any(|row| {
        row.home_role.as_deref() == Some(pack.role.as_str())
            && (row.persona_team_dir.as_deref() != Some(pack.dir.as_path())
                || row.persona_name_in_team.as_deref() != Some(pack.persona_name.as_str()))
    }) {
        SelectedRolePackState::WrongProject
    } else {
        SelectedRolePackState::Missing
    }
}

fn collect_team(host: &impl ReadinessHost, checkout: Option<&Path>, gathered: &mut Gathered) {
    let metadata = match host.agents(gathered.owner_pubkey.as_deref()) {
        Ok(rows) => rows,
        Err(error) => {
            let mut fact = TeamReadinessFact::local(
                "team",
                "AGENT_METADATA_UNREADABLE",
                if gathered.team.selected_roles.is_empty() {
                    TeamReadinessFactState::Limited
                } else {
                    TeamReadinessFactState::Unknown
                },
                error,
            );
            fact.remedy = Some("Repair the managed-agent metadata store.".into());
            gathered.facts.push(fact);
            Vec::new()
        }
    };
    append_agent_auth_facts(&metadata, gathered);
    let live = host.live_agent_pubkeys().unwrap_or_default();
    gathered.team.identities = metadata
        .iter()
        .map(|row| identity_projection(row, &live))
        .collect();
    let Some(checkout) = checkout else {
        return;
    };
    let directory = project_role_packs_dir(checkout);
    if !directory.is_dir() {
        let mut fact = TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_MISSING",
            if gathered.team.selected_roles.is_empty() {
                TeamReadinessFactState::Limited
            } else {
                TeamReadinessFactState::Blocked
            },
            "The project exposes no role-pack directory",
        );
        fact.remedy = Some("Restore personas/roles.".into());
        gathered.facts.push(fact);
        return;
    }
    let scan = match scan_role_packs(&directory) {
        Ok(scan) => scan,
        Err(error) => {
            let mut fact = TeamReadinessFact::local(
                "team",
                "ROLE_PACKS_UNREADABLE",
                if gathered.team.selected_roles.is_empty() {
                    TeamReadinessFactState::Limited
                } else {
                    TeamReadinessFactState::Unknown
                },
                error,
            );
            fact.remedy = Some("Repair the role-pack directory.".into());
            gathered.facts.push(fact);
            return;
        }
    };
    let mut hasher = Sha256::new();
    let mut wrong_project_roles = HashSet::new();
    for pack in scan.packs {
        hasher.update(pack.role.as_bytes());
        hasher.update([0]);
        hasher.update(pack.persona_name.as_bytes());
        hasher.update([0]);
        hasher.update(pack.display_name.as_bytes());
        hasher.update([0]);
        hasher.update(pack.system_prompt.as_bytes());
        hasher.update([0]);
        for value in [
            pack.runtime.as_deref(),
            pack.model.as_deref(),
            pack.provider.as_deref(),
        ] {
            hasher.update(value.unwrap_or("").as_bytes());
            hasher.update([0]);
        }
        hasher.update(pack.avatar_url.as_deref().unwrap_or("").as_bytes());
        hasher.update([0]);
        let installed = metadata.iter().find(|row| {
            row.persona_team_dir.as_deref() == Some(pack.dir.as_path())
                && row.persona_name_in_team.as_deref() == Some(pack.persona_name.as_str())
        });
        if gathered.team.selected_roles.contains(&pack.role) {
            match selected_role_pack_state(&metadata, &pack) {
                SelectedRolePackState::Current | SelectedRolePackState::Missing => {}
                SelectedRolePackState::Dirty => gathered.facts.push(TeamReadinessFact::blocked(
                    "team",
                    "SELECTED_ROLE_PACK_DIRTY",
                    format!(
                        "Selected role {} differs from its installed source",
                        pack.role
                    ),
                    "Use Prepare to refresh the selected role.",
                )),
                SelectedRolePackState::SourceUnknown => {
                    gathered.facts.push(TeamReadinessFact::unknown(
                        "team",
                        "SELECTED_ROLE_PACK_STATE_UNKNOWN",
                        format!("Selected role {} has no source digest", pack.role),
                        "Use Prepare to refresh the selected role.",
                    ))
                }
                SelectedRolePackState::WrongProject => {
                    wrong_project_roles.insert(pack.role.clone());
                }
            }
        }
        gathered.team.packs.push(TeamReadinessRolePack {
            role: pack.role.clone(),
            persona_name: pack.persona_name,
            path: pack.dir.display().to_string(),
            installed_pubkey: installed.map(|row| row.pubkey.clone()),
        });
        gathered.team.available_roles.push(pack.role);
    }
    gathered.team.available_roles.sort();
    gathered.team.available_roles.dedup();
    if gathered.team.packs.is_empty() {
        let mut fact = TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_EMPTY",
            if gathered.team.selected_roles.is_empty() {
                TeamReadinessFactState::Limited
            } else {
                TeamReadinessFactState::Blocked
            },
            "The role-pack directory contains no usable roles",
        );
        fact.remedy = Some("Add at least one role-bearing pack.".into());
        gathered.facts.push(fact);
    } else {
        gathered.team.packs_digest = Some(hex::encode(hasher.finalize()));
        gathered.team.packs_revision = gathered.source.checkout_commit.clone();
        gathered.facts.push(TeamReadinessFact::local(
            "team",
            "ROLE_PACKS_READY",
            TeamReadinessFactState::Ready,
            "Project role packs are readable",
        ));
    }
    for role in &gathered.team.selected_roles {
        let Some(pack) = gathered.team.packs.iter().find(|pack| &pack.role == role) else {
            gathered.facts.push(TeamReadinessFact::blocked(
                "team",
                "SELECTED_ROLE_UNAVAILABLE",
                format!("Selected role {role} has no project pack"),
                "Choose an available role.",
            ));
            continue;
        };
        let Some(pubkey) = pack.installed_pubkey.as_deref() else {
            if wrong_project_roles.contains(role) {
                gathered.facts.push(TeamReadinessFact::blocked(
                    "team",
                    "SELECTED_ROLE_WRONG_PROJECT",
                    format!("Selected role {role} is installed from another project"),
                    "Use Prepare to install this project's role pack.",
                ));
                continue;
            }
            gathered.facts.push(TeamReadinessFact::blocked(
                "team",
                "SELECTED_ROLE_NOT_INSTALLED",
                format!("Selected role {role} has no installed identity"),
                "Use Prepare to install the selected role.",
            ));
            continue;
        };
        if gathered
            .team
            .identities
            .iter()
            .find(|row| row.pubkey == pubkey)
            .is_none_or(|row| row.key_state != TeamReadinessKeyState::LiveProcess)
        {
            gathered.facts.push(TeamReadinessFact::unknown(
                "team",
                "SELECTED_ROLE_KEY_UNVERIFIED",
                format!("Selected role {role} key accessibility is unverified"),
                "Use Prepare to start and re-check the selected identity.",
            ));
        }
    }
}

fn registry_targets(text: &str) -> Result<Vec<(String, String)>, String> {
    buzz_core_pkg::coding_session_routing::parse_registry(text)
        .map_err(|error| error.to_string())
        .map(|registry| {
            registry
                .targets
                .into_iter()
                .map(|target| {
                    let provider = target.provider;
                    (provider.clone(), format!("{provider}:{}", target.model))
                })
                .collect()
        })
}

fn collect_runtimes_and_registry(
    host: &impl ReadinessHost,
    checkout: Option<&Path>,
    gathered: &mut Gathered,
) {
    gathered.runtimes = host.runtimes();
    gathered.facts.push(TeamReadinessFact::local(
        "runtime",
        "RUNTIME_METADATA_UNOBSERVED",
        TeamReadinessFactState::Limited,
        "Runtime install, authentication, adapter version, and model probes are not read locally",
    ));
    let Some(checkout) = checkout else {
        return;
    };
    let text = match super::project_files::read_allowlisted_project_file(
        checkout,
        super::project_files::MODEL_REGISTRY_RELATIVE_PATH,
    ) {
        Ok(source) => source.text,
        Err(refusal) => {
            gathered.facts.push(TeamReadinessFact::blocked(
                "routing",
                "REGISTRY_UNREADABLE",
                format!("{}: {}", refusal.code, refusal.message),
                "Restore team/model-registry.yaml.",
            ));
            return;
        }
    };
    let targets = match registry_targets(&text) {
        Ok(rows) => rows,
        Err(error) => {
            gathered.facts.push(TeamReadinessFact::blocked(
                "routing",
                "REGISTRY_INVALID",
                error,
                "Repair the version 1 registry.",
            ));
            return;
        }
    };
    gathered.registry.provider_targets = targets
        .iter()
        .map(|(provider, _)| provider.clone())
        .collect();
    gathered.registry.provider_targets.sort();
    gathered.registry.provider_targets.dedup();
    gathered.registry.pending_targets = targets.into_iter().map(|(_, target)| target).collect();
    gathered.facts.push(TeamReadinessFact::local(
        "routing",
        "REGISTRY_COVERAGE_AWAITING_CATALOG",
        TeamReadinessFactState::Limited,
        "Registry targets await a trusted provider-signed catalog observation",
    ));
}

fn collect_provider(
    host: &impl ReadinessHost,
    hiring_policy_enabled: Option<bool>,
    gathered: &mut Gathered,
) {
    gathered.provider.relay_url = host.relay_url();
    let store = match host.provider_store(gathered.owner_pubkey.as_deref()) {
        Ok(store) => store,
        Err(error) => {
            gathered.facts.push(TeamReadinessFact::unknown(
                "provider",
                "PROVIDER_METADATA_UNREADABLE",
                error,
                "Repair the provider metadata store.",
            ));
            return;
        }
    };
    gathered.policy = TeamReadinessPolicy {
        hiring_policy: hiring_policy_enabled
            .map_or(
                "unknown",
                |value| if value { "enabled" } else { "disabled" },
            )
            .into(),
        max_sessions: store.max_sessions,
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        turn_budget: store.turn_budget,
    };
    match hiring_policy_enabled {
        None => gathered.facts.push(TeamReadinessFact::unknown(
            "policy",
            "HIRING_POLICY_UNKNOWN",
            "Frontend hiring policy was not supplied",
            "Open Prepare so its explicit policy can be checked.",
        )),
        Some(false) => gathered.facts.push(TeamReadinessFact::blocked(
            "policy",
            "HIRING_POLICY_DISABLED",
            "Automatic team hiring is disabled",
            "Enable team hiring in Prepare.",
        )),
        Some(true) => gathered.facts.push(TeamReadinessFact::local(
            "policy",
            "HIRING_POLICY_ENABLED",
            TeamReadinessFactState::Ready,
            "Automatic team hiring is enabled",
        )),
    }
    let Some(record) = store.get(&gathered.provider.relay_url) else {
        gathered.facts.push(TeamReadinessFact::blocked(
            "provider",
            "PROVIDER_NOT_PROVISIONED",
            "No provider identity is provisioned for this relay",
            "Use Prepare to provision it.",
        ));
        return;
    };
    gathered.provider.provisioned = true;
    gathered.provider.provider_pubkey = Some(record.provider_pubkey.clone());
    gathered.provider.instance_id = Some(record.instance_id.clone());
    gathered.provider.auth_tag_present = Some(record.auth_tag_present);
    append_provider_auth_fact(record, gathered);
    match host.provider_process(&record.provider_pubkey) {
        CodingSessionProviderProcessState::Live { pid } => {
            gathered.provider.process = "live".into();
            gathered.provider.child_pid = Some(pid);
            gathered.provider.key_state = Some(TeamReadinessKeyState::LiveProcess);
            gathered.facts.push(TeamReadinessFact::local(
                "provider",
                "PROVIDER_CHILD_LIVE",
                TeamReadinessFactState::Ready,
                "The provisioned provider has a live child process",
            ));
        }
        CodingSessionProviderProcessState::Backoff => {
            gathered.provider.process = "backoff".into();
            gathered.provider.key_state = Some(TeamReadinessKeyState::Unverified);
            gathered.facts.push(TeamReadinessFact::unknown(
                "provider",
                "PROVIDER_IN_BACKOFF",
                "The supervisor exists but no provider child is live",
                "Wait for recovery or restart the provider.",
            ));
        }
        CodingSessionProviderProcessState::NotSupervised => {
            gathered.provider.process = "not_supervised".into();
            gathered.provider.key_state = Some(TeamReadinessKeyState::Unverified);
            gathered.facts.push(TeamReadinessFact::blocked(
                "provider",
                "PROVIDER_NOT_RUNNING",
                "The provider is not supervised",
                "Use Prepare to start it.",
            ));
        }
        CodingSessionProviderProcessState::Unknown => {
            gathered.provider.process = "unknown".into();
            gathered.provider.key_state = Some(TeamReadinessKeyState::Unverified);
            gathered.facts.push(TeamReadinessFact::unknown(
                "provider",
                "PROVIDER_PROCESS_UNKNOWN",
                "Provider process state is unknown",
                "Restart Beekeeper and retry.",
            ));
        }
    }
}

fn finish(project_ref: String, mut gathered: Gathered) -> TeamReadinessResponse {
    if gathered.source.app_commit.is_none() {
        gathered.facts.push(TeamReadinessFact::unknown(
            "source",
            "APP_SOURCE_REVISION_UNKNOWN",
            "Build source revision is unknown",
            "Rebuild from a traceable Git checkout.",
        ));
    }
    if gathered.source.app_source_dirty.is_none() {
        gathered.facts.push(TeamReadinessFact::local(
            "source",
            "APP_SOURCE_DIRTY_UNOBSERVED",
            TeamReadinessFactState::Limited,
            "Build dirty state is unobserved because incremental builds cannot track every path",
        ));
    }
    gathered.facts.push(TeamReadinessFact {
        category: "catalog".into(),
        code: "CATALOG_AWAITING_FIRST_SESSION".into(),
        scope: TeamReadinessScope::Wire,
        state: TeamReadinessFactState::AwaitingFirstSession,
        summary: "No trusted provider-signed catalog snapshot was supplied".into(),
        remedy: Some(
            "Launch the first session, then fold its trusted catalog snapshot in the frontend."
                .into(),
        ),
    });
    gathered.facts.push(TeamReadinessFact {
        category: "relay".into(),
        code: "RELAY_UNOBSERVED".into(),
        scope: TeamReadinessScope::Wire,
        state: TeamReadinessFactState::AwaitingFirstSession,
        summary: "Relay reachability is not a backend inventory fact".into(),
        remedy: None,
    });
    response_from_facts(
        project_ref,
        gathered,
        TeamReadinessCatalog {
            state: TeamReadinessFactState::AwaitingFirstSession,
            revision: None,
            targets: Vec::new(),
            source: TeamReadinessScope::Wire,
            provenance: Vec::new(),
        },
        TeamReadinessRelay {
            state: TeamReadinessFactState::AwaitingFirstSession,
            reachable: None,
            source: TeamReadinessScope::Wire,
        },
    )
}

fn response_from_facts(
    project_ref: String,
    gathered: Gathered,
    catalog: TeamReadinessCatalog,
    relay: TeamReadinessRelay,
) -> TeamReadinessResponse {
    let codes = |state| {
        gathered
            .facts
            .iter()
            .filter(|fact| fact.state == state)
            .map(|fact| fact.code.clone())
            .collect::<Vec<_>>()
    };
    let blocking_codes = codes(TeamReadinessFactState::Blocked);
    let unknown_codes = codes(TeamReadinessFactState::Unknown);
    let awaiting_codes = codes(TeamReadinessFactState::AwaitingFirstSession);
    let limited_codes = codes(TeamReadinessFactState::Limited);
    let ready_for_first_session = gathered
        .facts
        .iter()
        .filter(|fact| fact.scope == TeamReadinessScope::Local)
        .all(|fact| {
            !matches!(
                fact.state,
                TeamReadinessFactState::Blocked | TeamReadinessFactState::Unknown
            )
        });
    let status = if !blocking_codes.is_empty() {
        TeamReadinessStatus::Blocked
    } else if !unknown_codes.is_empty() {
        TeamReadinessStatus::Unknown
    } else if !awaiting_codes.is_empty() {
        TeamReadinessStatus::AwaitingFirstSession
    } else {
        TeamReadinessStatus::Ready
    };
    let cold = gathered.source.checkout_path.is_none()
        && gathered.team.packs.is_empty()
        && !gathered.provider.provisioned;
    let host_class = if cold {
        TeamReadinessHostClass::Cold
    } else if ready_for_first_session {
        TeamReadinessHostClass::PreparedForFirstSession
    } else {
        TeamReadinessHostClass::Partial
    };
    TeamReadinessResponse {
        schema_version: SCHEMA_VERSION,
        project_ref,
        generated_at: crate::util::now_iso(),
        ready_for_first_session,
        ready: status == TeamReadinessStatus::Ready,
        status,
        host_class,
        key_store_safe: gathered.key_store_safe,
        source: gathered.source,
        owner_pubkey: gathered.owner_pubkey,
        team: gathered.team,
        runtimes: gathered.runtimes,
        registry: gathered.registry,
        provider: gathered.provider,
        policy: gathered.policy,
        relay,
        catalog,
        facts: gathered.facts,
        blocking_codes,
        unknown_codes,
        awaiting_codes,
        limited_codes,
    }
}

fn gather(
    host: &impl ReadinessHost,
    project_ref: String,
    selected_roles: Option<Vec<String>>,
    hiring_policy_enabled: Option<bool>,
) -> TeamReadinessResponse {
    let mut gathered = Gathered {
        source: embedded_source(),
        ..Default::default()
    };
    gathered.team.selected_roles = selected_roles
        .unwrap_or_default()
        .into_iter()
        .map(|role| role.trim().to_ascii_lowercase())
        .filter(|role| !role.is_empty())
        .collect();
    gathered.team.selected_roles.sort();
    gathered.team.selected_roles.dedup();
    if !valid_project_ref(&project_ref) {
        gathered.facts.push(TeamReadinessFact::blocked(
            "project",
            "PROJECT_REF_INVALID",
            "Project coordinate is invalid",
            "Open a signed project.",
        ));
        return finish(project_ref, gathered);
    }
    collect_identity(host, &mut gathered);
    let checkout = collect_checkout(host, &project_ref, &mut gathered);
    collect_team(host, checkout.as_deref(), &mut gathered);
    collect_runtimes_and_registry(host, checkout.as_deref(), &mut gathered);
    collect_provider(host, hiring_policy_enabled, &mut gathered);
    finish(project_ref, gathered)
}

/// Read local preparation facts without mutating the host or touching secrets.
#[tauri::command]
pub async fn team_readiness(
    app: AppHandle,
    project_ref: String,
    selected_roles: Option<Vec<String>>,
    hiring_policy_enabled: Option<bool>,
    expected_relay_url: String,
    channel_ids: Vec<String>,
) -> Result<TeamReadinessResponse, String> {
    let project_ref = project_ref.trim().to_string();
    let local_app = app.clone();
    let local_project_ref = project_ref.clone();
    let local = tauri::async_runtime::spawn_blocking(move || {
        gather(
            &AppReadinessHost(&local_app),
            local_project_ref,
            selected_roles,
            hiring_policy_enabled,
        )
    })
    .await
    .map_err(|error| format!("team readiness task failed: {error}"))?;
    let wire =
        wire::observe_team_wire(&app, &project_ref, &expected_relay_url, &channel_ids).await?;
    Ok(wire::fold_trusted_team_wire(local, wire))
}

#[cfg(test)]
#[path = "team_readiness_tests.rs"]
mod tests;
