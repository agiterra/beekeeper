//! Side-effect-free LOCAL inventory for the Portable Team Loop.
//!
//! This command is R1: it reads public metadata and already-live in-memory
//! facts only. It never prepares a host. R2's explicit UI Prepare transaction
//! owns the named mutations, in order: install roles with confirmed names,
//! provision the provider, start it, then request a fresh readiness inventory.
//! Only that path may prompt, mint, publish, migrate, back up, or hydrate keys.

use std::collections::HashSet;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::app_state::KEYCHAIN_UNAVAILABLE;
use crate::coding_sessions::workdir_store::CodingSessionWorkdirStore;
use crate::managed_agents::ManagedAgentReadinessMetadata;
use crate::session_provider::runtimes::StrictRuntimeDiagnostic;
use crate::session_provider::status::CodingSessionProviderProcessState;

const SCHEMA_VERSION: u32 = 3;
#[path = "team_readiness_git.rs"]
mod git_probe;
use git_probe::{checkout_source, embedded_source};
#[cfg(test)]
use git_probe::{parse_embedded_source, validate_dirty};
#[path = "team_readiness_packs.rs"]
mod packs;
use packs::{collect_team, probe_project_pack_source, ProjectPackSourceProbe};

#[path = "team_readiness_host.rs"]
mod host;
use host::{AppReadinessHost, ReadinessHost};
#[path = "team_readiness_auth_facts.rs"]
mod auth_facts;
use auth_facts::append_provider_auth_fact;

#[path = "team_readiness_actions.rs"]
pub mod actions;
#[path = "team_readiness_registry.rs"]
mod registry;
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
    /// Whether this project names a role source at all — a kind:30624 pack
    /// source, staged or not. `None` when nothing asked (a direct `gather`
    /// with no relay behind it).
    ///
    /// A *fact about the project*, not about this computer's staging: it is
    /// what "Use roles" defaults from, so a brand-new project whose agents
    /// repository was seeded with eight roles opens the founding form with
    /// roles on rather than silently off (ledger 207(2)).
    #[serde(default)]
    pub pack_source_present: Option<bool>,
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
    /// Which copy answered — `agents-repo` or `checkout` — or `None` when
    /// none did. Readiness reads the registry through the same resolver a
    /// hire does (ledger 207(1)), so this is the copy a hire would route on.
    #[serde(default)]
    pub origin: Option<String>,
    /// That origin as a clause a sentence can carry.
    #[serde(default)]
    pub origin_label: Option<String>,
    /// The absolute, symlink-resolved path the bytes came from.
    #[serde(default)]
    pub path: Option<String>,
    /// Every place that was looked at, in order — so an absence names both
    /// places rather than implying there was only ever one.
    #[serde(default)]
    pub looked_in: Vec<String>,
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
    workdirs: Result<&CodingSessionWorkdirStore, &String>,
    project_ref: &str,
    gathered: &mut Gathered,
) -> Option<PathBuf> {
    let store = match workdirs {
        Ok(store) => store,
        Err(error) => {
            gathered.facts.push(TeamReadinessFact::unknown(
                "project",
                "CHECKOUT_STORE_UNREADABLE",
                error.clone(),
                "Choose the project checkout again.",
            ));
            return None;
        }
    };
    let Some(entry) = store.by_project.get(project_ref) else {
        gathered.facts.push(TeamReadinessFact::blocked(
            "project",
            "CHECKOUT_NOT_RECORDED",
            "This computer has no repository folder recorded for this project, so a \
             session here would have nothing to cut its worktrees from",
            "Point this project at the folder its repository is checked out in — the \
             founding form offers the folder it already has, and Project settings → This \
             computer sets it directly.",
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
    store: Option<&CodingSessionWorkdirStore>,
    project_ref: &str,
    packs_from_project: bool,
    gathered: &mut Gathered,
) {
    gathered.runtimes = host.runtimes();
    gathered.facts.push(TeamReadinessFact::local(
        "runtime",
        "RUNTIME_METADATA_UNOBSERVED",
        TeamReadinessFactState::Limited,
        "Runtime install, authentication, adapter version, and model probes are not read locally",
    ));
    // The agents repository first, then the checkout — the hire host's own
    // order, through the hire host's own resolver (ledger 207(1)).
    let Some(text) =
        registry::resolve_registry_text(store, project_ref, packs_from_project, gathered)
    else {
        return;
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
    let origin = registry::registry_origin_clause(gathered);
    gathered.facts.push(TeamReadinessFact::local(
        "routing",
        "REGISTRY_COVERAGE_AWAITING_CATALOG",
        TeamReadinessFactState::Limited,
        format!("Registry targets await a trusted provider-signed catalog observation{origin}"),
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
                "The agent host is running but no provider is",
                "Use Prepare to start it.",
            ));
        }
        // **Blocked, never Ready and never unknown.** The agent host did not
        // answer, or answered that it will not start a provider — so nothing
        // about this machine's readiness is known, and a launch gate that let
        // that through would seat a team against a provider that may not
        // exist. `unknown` would do exactly that, because the gate treats
        // unknowns as passable.
        //
        // The remediation is the host's own words rather than a generic
        // "use Prepare": the causes are different (not installed, not running,
        // no identity, another host owns the lock) and so are the fixes.
        CodingSessionProviderProcessState::HostUnreachable { reason } => {
            gathered.provider.process = "host_unreachable".into();
            gathered.provider.key_state = Some(TeamReadinessKeyState::Unverified);
            gathered.facts.push(TeamReadinessFact::blocked(
                "provider",
                "PROVIDER_HOST_UNREACHABLE",
                "The agent host could not answer for the provider",
                &reason,
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

/// The one fact about the git this computer would reach the relay with.
///
/// **Ledger 168.** A bundle launched from Finder sees only
/// `/usr/bin:/bin:/usr/sbin:/sbin`, so it reaches Apple's git 2.39.5, which
/// has no `authtype` credential capability and therefore cannot carry a
/// Nostr credential at all; every seat re-stage and every hire whose pack
/// source needs a fetch failed with git's `could not read Username`, a
/// sentence about the credential for a problem that is the git.
///
/// `None` when the computer has a capable git: a readiness panel that
/// announces a satisfied requirement teaches nothing.
///
/// Limited, not Blocked, by default: an old git breaks only what must reach
/// the relay, and a launch whose packs are already cached does not. It blocks
/// when this project's pack source could not be read — the shape a branch
/// pin, or a commit this computer has not cached, produces, because both must
/// fetch and a fetch is exactly what this git cannot do.
fn git_capability_fact(
    capability: &crate::commands::project_git_version::GitCapability,
    packs_source_unavailable: bool,
) -> Option<TeamReadinessFact> {
    if capability.meets_minimum {
        return None;
    }
    let found = match (capability.version.as_deref(), capability.path.as_deref()) {
        (Some(version), Some(path)) => format!("git {version} at {path}"),
        // A machine with no git at all: named as the absence it is, never as
        // a version this check did not read.
        _ => "no git".to_string(),
    };
    let minimum = &capability.minimum;
    let (state, summary) = if packs_source_unavailable {
        (
            TeamReadinessFactState::Blocked,
            format!(
                "This computer's {found} cannot authenticate to the relay — the Nostr \
                 credential helper needs git {minimum} or newer — and this project's role \
                 packs could not be read, so no seat could be staged"
            ),
        )
    } else {
        (
            TeamReadinessFactState::Limited,
            format!(
                "This computer's {found} cannot authenticate to the relay: the Nostr \
                 credential helper needs git {minimum} or newer. Anything that must fetch \
                 from the relay — a pack this computer has not cached, a hire, a push — \
                 will be refused"
            ),
        )
    };
    let mut fact = TeamReadinessFact::local("host", "GIT_TOO_OLD_FOR_RELAY", state, summary);
    fact.remedy = Some(format!(
        "Install git {minimum} or newer (`brew install git` on macOS) and relaunch Beekeeper."
    ));
    Some(fact)
}

fn gather(
    host: &impl ReadinessHost,
    project_ref: String,
    selected_roles: Option<Vec<String>>,
    hiring_policy_enabled: Option<bool>,
    packs: &ProjectPackSourceProbe,
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
    // Read once: the checkout rung and the registry's two rungs are answers
    // from the same record, and a second read could disagree with the first.
    let workdirs = host.workdirs();
    let checkout = collect_checkout(workdirs.as_ref(), &project_ref, &mut gathered);
    gathered.team.pack_source_present = match packs {
        ProjectPackSourceProbe::NotProbed => None,
        ProjectPackSourceProbe::Absent => Some(false),
        // A source this computer cannot stage from is still a source the
        // project names: the fact the default follows is the project's, and a
        // staging failure is disclosed on its own fact.
        ProjectPackSourceProbe::Available { .. } | ProjectPackSourceProbe::Unavailable { .. } => {
            Some(true)
        }
    };
    collect_team(host, checkout.as_deref(), packs, &mut gathered);
    if let Some(fact) = git_capability_fact(
        &crate::commands::project_git_version::get_git_capability(),
        matches!(packs, ProjectPackSourceProbe::Unavailable { .. }),
    ) {
        gathered.facts.push(fact);
    }
    collect_runtimes_and_registry(
        host,
        workdirs.as_ref().ok(),
        &project_ref,
        matches!(packs, ProjectPackSourceProbe::Available { .. }),
        &mut gathered,
    );
    collect_provider(host, hiring_policy_enabled, &mut gathered);
    finish(project_ref, gathered)
}

/// Read local preparation facts without mutating the host or touching secrets.
#[tauri::command]
pub async fn team_readiness(
    app: AppHandle,
    state: tauri::State<'_, crate::app_state::AppState>,
    project_ref: String,
    selected_roles: Option<Vec<String>>,
    hiring_policy_enabled: Option<bool>,
    expected_relay_url: String,
    channel_ids: Vec<String>,
) -> Result<TeamReadinessResponse, String> {
    let project_ref = project_ref.trim().to_string();
    // Asked before the local inventory, because it decides whether the
    // in-checkout `personas/roles` rung is even consulted.
    let packs = if valid_project_ref(&project_ref) {
        probe_project_pack_source(&app, &state, &project_ref).await
    } else {
        ProjectPackSourceProbe::NotProbed
    };
    let local_app = app.clone();
    let local_project_ref = project_ref.clone();
    let local = tauri::async_runtime::spawn_blocking(move || {
        gather(
            &AppReadinessHost(&local_app),
            local_project_ref,
            selected_roles,
            hiring_policy_enabled,
            &packs,
        )
    })
    .await
    .map_err(|error| format!("team readiness task failed: {error}"))?;
    let wire =
        wire::observe_team_wire(&app, &project_ref, &expected_relay_url, &channel_ids).await?;
    let mut response = wire::fold_trusted_team_wire(local, wire);
    // Asked last, and asked at all because a session was founded on a goal it
    // could not finish: whether this session's team could publish and trigger
    // this project's actions (ledger 186). Appended after the fold so it sits
    // with the other `wire` facts, then re-summarized so `limitedCodes` and
    // `unknownCodes` carry it.
    if valid_project_ref(&project_ref) {
        actions::append_project_action_authority_fact(&app, &project_ref, &mut response).await;
        wire::summarize(&mut response);
    }
    Ok(response)
}

#[cfg(test)]
#[path = "team_readiness_host_tests.rs"]
mod host_tests;
#[cfg(test)]
#[path = "team_readiness_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "team_readiness_git_capability_tests.rs"]
mod git_capability_tests;

// Split from `tests` because that file sits at the repository's 1000-line
// ceiling; these are the cases that shell out to `git`.
#[cfg(test)]
#[path = "team_readiness_git_tests.rs"]
mod git_tests;
