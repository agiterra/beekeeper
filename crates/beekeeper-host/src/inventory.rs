//! What this Host can honestly say about its agents, runtimes and setups, with
//! no key, no relay and no desktop.
//!
//! The answer to the `inventory` op. Everything here is read from files the
//! provider and the management operations already keep, and it deliberately
//! stays away from the ones that can hold secrets: `actor-seats.json`,
//! `provider-key`, `actors/` and the management operation journal are never
//! opened, so a capture of this socket reveals nothing signable.
//!
//! Every field that could be a comfortable guess is a named state with its
//! reason instead. A provider that stopped does not get to keep advertising
//! the models it offered when it was running.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use beekeeper_core::coding_session_catalog::{CatalogModel, CatalogProvider};
use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use beekeeper_host_core::agent_setup::{
    self, AgentSetup, DiscoveryOutcome, EffectiveCatalogFile, EffectiveSetupState,
};
use beekeeper_host_core::config::HostConfig;
use beekeeper_host_core::env::PROJECTS_FILE_NAME;
use beekeeper_host_core::layout::Instance;
use beekeeper_host_core::project::ProjectsFile;
use serde::{Deserialize, Serialize};

use crate::protocol::Warning;
use crate::sessions::{SessionRow, SessionSnapshot};
use crate::state::{ProviderChildState, RelayConnectionState};

/// How much the row can be relied on, in order of what stops a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeReadiness {
    /// The provider is not running, so nothing is on offer now.
    ProviderNotLive,
    /// No provider has written a catalog this Host can read.
    CatalogUnavailable,
    /// A catalog exists, written by a provider that is not the one running.
    CatalogStale,
    /// Commissioned, but the running provider does not list it.
    NotOffered,
    /// The runtime's credential could not be read.
    CredentialsUnavailable,
    /// The adapter was asked and failed; the list is the commissioned fallback.
    DiscoveryFailed,
    /// Listed from commissioning; the adapter has not been asked.
    Unconfirmed,
    /// The adapter answered with this list. The account is still unconfirmed:
    /// only a signed turn result proves that.
    AdapterAnswered,
}

/// One runtime: what was commissioned beside what is really on offer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeRow {
    pub instance_ref: String,
    pub runtime: String,
    pub driver: String,
    pub commissioned_default: String,
    pub commissioned_models: Vec<String>,
    pub discover_models: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_source: Option<String>,
    /// Names of the environment variables it reads; never values.
    pub credential_env: Vec<String>,
    pub readiness: RuntimeReadiness,
    /// One line for a person, including what to do.
    pub readiness_detail: String,
    /// What can be selected right now. Empty unless the running provider
    /// offers it.
    pub selectable_models: Vec<SelectableModel>,
    /// Commissioned, and absent from the effective offer. A discovery replaced
    /// them (ledger 410(h)); shown so the replacement is not silent.
    pub commissioned_not_offered: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catalog_revision: Option<u64>,
}

/// One model the running provider offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectableModel {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    pub is_default: bool,
    /// The id is the adapter's word `default`, which names no model; the real
    /// one is known only from a signed turn result.
    pub is_label: bool,
}

/// One project this Host can work in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRow {
    pub coordinate: String,
    /// Recorded by `bee host trust-project`; stays on this machine.
    pub checkout: Option<String>,
    pub checkout_present: bool,
    pub agents_repo: Option<String>,
    pub agents_repo_present: bool,
    pub agents_ref: Option<String>,
    /// Both recorded and present. Authority is still checked on the relay.
    pub usable: bool,
}

/// One identity with anything on this Host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRow {
    pub actor: String,
    /// From a Manage Host setup, when there is one. Otherwise the signed
    /// profile is the only source, and this Host cannot read the relay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub roles: Vec<String>,
    pub runtimes: Vec<String>,
    pub projects: Vec<String>,
    /// Rows the provider remembers, most of them closed.
    pub session_count: usize,
    /// How many of those the provider's record says are open.
    pub open_sessions: usize,
    pub turns_in_flight: usize,
    /// An adopted identity's open sessions from before its adoption. Not in
    /// `open_sessions`, and not this Host's to stop.
    pub prior_open_sessions: usize,
    /// The turns in flight on those. Not in `turns_in_flight`.
    pub prior_turns_in_flight: usize,
    /// `host_enrolled`, or `sessions_only` when the identity appears in the
    /// provider's sessions and the host enrollment journal does not hold it.
    pub custody: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<String>,
}

/// One setup, with its stored state read against the live lock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRow {
    pub effective_state: EffectiveSetupState,
    /// The provider's record holds an open execution of this agent, whatever the
    /// journal says. A person can stop what this is true of.
    pub execution_held: bool,
    pub setup: AgentSetup,
}

/// One unresolved seat-custody request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryRow {
    pub command_id: String,
    pub actor: String,
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_ref: Option<String>,
    pub session_id: String,
    pub generation: u64,
    /// The command that ends this generation, when the row can name its channel.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack_sha: Option<String>,
    pub fenced: bool,
    pub project_trusted_here: bool,
    pub session_present: bool,
    /// Stable code for why this row stands.
    pub code: String,
    /// One line for a person: what is true and what can be done.
    pub remedy: String,
}

/// The whole recovery list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recovery {
    pub rows: Vec<RecoveryRow>,
    /// Why nothing here has a button.
    pub write_operation: String,
    /// Set when the file exists and could not be read: not the same as none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unavailable: Option<String>,
}

/// Facts about the Host itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostFacts {
    pub instance: Instance,
    pub host_pid: u32,
    pub host_version: String,
    pub relay_url: String,
    pub provider_pubkey: String,
    /// The owner the provider's attestation verifies to, or `None` with
    /// [`Self::owner_unverified`] saying why.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_pubkey: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_unverified: Option<String>,
    pub provider: ProviderChildState,
    pub relay_connection: RelayConnectionState,
}

/// The answer to `inventory`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub read_at: String,
    pub host: HostFacts,
    pub runtimes: Vec<RuntimeRow>,
    pub projects: Vec<ProjectRow>,
    pub agents: Vec<AgentRow>,
    pub setups: Vec<SetupRow>,
    pub recovery: Recovery,
    /// Facts a client must show, such as a malformed journal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<Warning>,
}

/// Everything [`read`] needs from the running Host.
pub struct InventoryInput<'a> {
    pub home: &'a Path,
    pub instance: Instance,
    pub config: &'a HostConfig,
    pub provider: ProviderChildState,
    pub host_pid: u32,
    pub host_version: String,
    pub sessions: &'a SessionSnapshot,
}

/// A client's words for a failed `inventory`, naming the case an older Host
/// produces: it refuses the op as an unknown request.
pub fn describe_client_error(error: &crate::client::ClientError) -> String {
    if let crate::client::ClientError::Refused { message } = error {
        if message.contains("unknown variant") {
            return "HOST_UPGRADE_REQUIRED: the running Host is older than this client and has no inventory. Update the app and restart the Host.".to_string();
        }
    }
    format!("HOST_UNAVAILABLE: {}", error.message())
}

/// The `default` model id names no model.
fn is_label(id: &str) -> bool {
    id.eq_ignore_ascii_case("default")
}

fn selectable(provider: &CatalogProvider) -> Vec<SelectableModel> {
    provider
        .allowed_models
        .iter()
        .map(|id| {
            let detail: Option<&CatalogModel> = provider.models.iter().find(|m| &m.id == id);
            SelectableModel {
                id: id.clone(),
                name: detail.and_then(|m| m.name.clone()),
                efforts: detail.map(|m| m.efforts.clone()).unwrap_or_default(),
                is_default: id == &provider.default_model,
                is_label: is_label(id),
            }
        })
        .collect()
}

fn runtime_row(
    descriptor: &RuntimeDescriptor,
    live_pid: Option<u32>,
    file: Option<&EffectiveCatalogFile>,
) -> RuntimeRow {
    let provider = file.and_then(|file| {
        file.catalog
            .providers
            .iter()
            .find(|p| p.provider_instance_ref == descriptor.instance_ref)
    });
    let discovery = file.and_then(|file| file.discovery.get(&descriptor.instance_ref));
    let current = matches!((file, live_pid), (Some(file), Some(pid)) if file.provider_pid == pid);
    let (readiness, detail) = match (live_pid, file, current, provider, discovery) {
        (None, _, _, _, _) => (
            RuntimeReadiness::ProviderNotLive,
            "The session provider is not running, so nothing is on offer. Start it from the menu bar."
                .to_string(),
        ),
        (Some(_), None, _, _, _) => (
            RuntimeReadiness::CatalogUnavailable,
            "The running provider has not written a local catalog. A provider older than this Host does not write one; update the app and restart agents."
                .to_string(),
        ),
        (Some(_), Some(_), false, _, _) => (
            RuntimeReadiness::CatalogStale,
            "The local catalog was written by an earlier provider process and is not treated as current. Restart agents."
                .to_string(),
        ),
        (Some(_), Some(_), true, None, _) => (
            RuntimeReadiness::NotOffered,
            "The running provider does not offer this commissioned runtime.".to_string(),
        ),
        (_, _, _, Some(_), Some(DiscoveryOutcome::CredentialsUnavailable(code))) => (
            RuntimeReadiness::CredentialsUnavailable,
            format!("The runtime's credential could not be read ({code}), so nothing can run on it. Restore the credential and restart agents."),
        ),
        (_, _, _, Some(_), Some(DiscoveryOutcome::Failed(reason))) => (
            RuntimeReadiness::DiscoveryFailed,
            format!("The adapter did not answer ({reason}). It may be signed out. Only the commissioned fallback is left, and the word `default` is not offered because it names no model."),
        ),
        (_, _, _, Some(_), Some(DiscoveryOutcome::Discovered)) => (
            RuntimeReadiness::AdapterAnswered,
            "The adapter answered with this list. The account behind it is unconfirmed until a signed turn result."
                .to_string(),
        ),
        (_, _, _, Some(_), _) => (
            RuntimeReadiness::Unconfirmed,
            "This runtime is not asked for its models, so this is the commissioned list and its account is unconfirmed."
                .to_string(),
        ),
    };
    let offered = if current { provider } else { None };
    // A list nobody vouched for. With no credential nothing can run; after a failed
    // discovery a concrete commissioned model may, but `default` names no model, so
    // there is nothing to confirm and it is not on offer.
    let selectable_models: Vec<SelectableModel> = offered
        .map(selectable)
        .unwrap_or_default()
        .into_iter()
        .filter(|model| match discovery {
            Some(DiscoveryOutcome::CredentialsUnavailable(_)) => false,
            Some(DiscoveryOutcome::Failed(_)) => !model.is_label,
            _ => true,
        })
        .collect();
    let offered_ids: BTreeSet<&str> = offered
        .map(|p| p.allowed_models.iter().map(String::as_str).collect())
        .unwrap_or_default();
    let commissioned_not_offered = if offered.is_some() {
        descriptor
            .allowed_models
            .iter()
            .filter(|id| !offered_ids.contains(id.as_str()))
            .cloned()
            .collect()
    } else {
        Vec::new()
    };
    RuntimeRow {
        instance_ref: descriptor.instance_ref.clone(),
        runtime: descriptor.runtime.clone(),
        driver: descriptor.driver.clone(),
        commissioned_default: descriptor.default_model.clone(),
        commissioned_models: descriptor.allowed_models.clone(),
        discover_models: descriptor.discover_models,
        credential_source: descriptor.credential_source.clone(),
        credential_env: descriptor.credential_env.clone(),
        readiness,
        readiness_detail: detail,
        selectable_models,
        commissioned_not_offered,
        catalog_revision: offered.and(file.map(|f| f.catalog.revision)),
    }
}

fn project_rows(projects: &ProjectsFile) -> Vec<ProjectRow> {
    let mut coordinates: BTreeSet<&String> = projects.projects.keys().collect();
    coordinates.extend(projects.agents_repos.keys());
    coordinates
        .into_iter()
        .map(|coordinate| {
            let checkout = projects.projects.get(coordinate);
            let agents = projects.agents_repos.get(coordinate);
            let checkout_present = checkout.is_some_and(|path| path.is_dir());
            let agents_repo_present = agents.is_some_and(|record| record.path.is_dir());
            ProjectRow {
                coordinate: coordinate.clone(),
                checkout: checkout.map(|path| path.display().to_string()),
                checkout_present,
                agents_repo: agents.map(|record| record.path.display().to_string()),
                agents_repo_present,
                agents_ref: agents.map(|record| record.ref_name.clone()),
                usable: checkout_present && agents_repo_present,
            }
        })
        .collect()
}

/// Session id to project, read here rather than widening [`SessionRow`], whose
/// shape the menu bar's fixtures and an older client rely on.
fn session_projects(state_dir: &Path) -> BTreeMap<String, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Snapshot {
        #[serde(default)]
        sessions: BTreeMap<String, Session>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Session {
        #[serde(default)]
        session_id: String,
        #[serde(default)]
        project_ref: Option<String>,
    }
    let Ok(bytes) = std::fs::read(state_dir.join("state.json")) else {
        return BTreeMap::new();
    };
    let Ok(snapshot) = serde_json::from_slice::<Snapshot>(&bytes) else {
        return BTreeMap::new();
    };
    snapshot
        .sessions
        .into_iter()
        .filter_map(|(key, session)| {
            let id = if session.session_id.is_empty() {
                key
            } else {
                session.session_id
            };
            session.project_ref.map(|project| (id, project))
        })
        .collect()
}

fn agent_rows(
    state_dir: &Path,
    sessions: &[SessionRow],
    projects_by_session: &BTreeMap<String, String>,
    open: &[agent_setup::OpenExecution],
    prior_open: &[agent_setup::OpenExecution],
    enrolled: &[String],
    setups: &[AgentSetup],
) -> Vec<AgentRow> {
    let mut rows: BTreeMap<String, AgentRow> = BTreeMap::new();
    let enrolled: BTreeSet<&str> = enrolled.iter().map(String::as_str).collect();
    let prior_of = |actor: &str| agent_setup::adopted_prior_sessions(state_dir, actor);
    let blank = |actor: &str| AgentRow {
        actor: actor.to_string(),
        display_name: None,
        roles: Vec::new(),
        runtimes: Vec::new(),
        projects: Vec::new(),
        session_count: 0,
        open_sessions: open.iter().filter(|row| row.actor == actor).count(),
        turns_in_flight: 0,
        prior_open_sessions: prior_open.iter().filter(|row| row.actor == actor).count(),
        prior_turns_in_flight: 0,
        custody: if enrolled.contains(actor) {
            "host_enrolled".to_string()
        } else {
            "sessions_only".to_string()
        },
        setup_id: None,
    };
    let push = |list: &mut Vec<String>, value: &str| {
        if !value.is_empty() && !list.iter().any(|held| held == value) {
            list.push(value.to_string());
        }
    };
    for session in sessions {
        let Some(actor) = session.actor.as_deref() else {
            continue;
        };
        let row = rows
            .entry(actor.to_string())
            .or_insert_with(|| blank(actor));
        row.session_count += 1;
        let working = usize::from(session.turn_started_at_ms.is_some());
        if prior_of(actor).contains(&session.session_id) {
            row.prior_turns_in_flight += working;
        } else {
            row.turns_in_flight += working;
        }
        if let Some(role) = &session.role {
            push(&mut row.roles, role);
        }
        if let Some(runtime) = &session.runtime {
            push(&mut row.runtimes, runtime);
        }
        if let Some(project) = projects_by_session.get(&session.session_id) {
            push(&mut row.projects, project);
        }
    }
    for actor in &enrolled {
        rows.entry((*actor).to_string())
            .or_insert_with(|| blank(actor));
    }
    for setup in setups {
        let Some(actor) = setup.agent_pubkey.as_deref() else {
            continue;
        };
        let row = rows
            .entry(actor.to_string())
            .or_insert_with(|| blank(actor));
        row.display_name = Some(setup.spec.display_name.clone());
        row.setup_id = Some(setup.setup_id.to_string());
        push(&mut row.roles, &setup.spec.role);
        push(&mut row.projects, &setup.spec.project);
    }
    rows.into_values().collect()
}

const RECOVERY_WRITE_OPERATION: &str = "Read-only here. Re-staging seat custody exists only in Beekeeper Desktop, because it needs the seat's key from the Desktop's Keychain and this Host never reads a Keychain. Desktop attempts it for each new provider process while it is open as this Host's owner. There is no Host or CLI write operation yet. The stop command shown for a row must be run by that session's founder with their key in BEEKEEPER_PRIVATE_KEY, and a stopped session cannot be resumed.";

fn recovery(
    state_dir: &Path,
    provider_pubkey: &str,
    projects: &ProjectsFile,
    sessions: &[SessionRow],
) -> Recovery {
    #[derive(Deserialize)]
    struct File {
        #[serde(default)]
        requests: Vec<Row>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Row {
        command_id: String,
        actor: String,
        #[serde(default)]
        role: String,
        #[serde(default)]
        project_ref: Option<String>,
        session_id: String,
        #[serde(default)]
        generation: u64,
        #[serde(default)]
        pack_ref: Option<PackRow>,
        #[serde(default)]
        fenced: bool,
    }
    #[derive(Deserialize)]
    struct PackRow {
        #[serde(default)]
        sha: Option<String>,
    }
    let bytes = match std::fs::read(state_dir.join("seat-requests.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Recovery {
                rows: Vec::new(),
                write_operation: RECOVERY_WRITE_OPERATION.to_string(),
                unavailable: None,
            }
        }
        Err(error) => {
            return Recovery {
                rows: Vec::new(),
                write_operation: RECOVERY_WRITE_OPERATION.to_string(),
                unavailable: Some(format!("the seat-request file could not be read: {error}")),
            }
        }
    };
    let file: File = match serde_json::from_slice(&bytes) {
        Ok(file) => file,
        Err(error) => {
            return Recovery {
                rows: Vec::new(),
                write_operation: RECOVERY_WRITE_OPERATION.to_string(),
                unavailable: Some(format!("the seat-request file is malformed: {error}")),
            }
        }
    };
    let held: BTreeMap<&str, &SessionRow> = sessions
        .iter()
        .map(|session| (session.session_id.as_str(), session))
        .collect();
    let rows = file
        .requests
        .into_iter()
        .map(|row| {
            let project_trusted_here = row
                .project_ref
                .as_ref()
                .is_some_and(|project| projects.projects.contains_key(project));
            let held_session = held.get(row.session_id.as_str());
            let session_present = held_session.is_some();
            let stop_command = held_session
                .and_then(|session| session.channel_id.as_deref())
                .map(|channel| {
                    format!(
                        "bee sessions stop --channel {channel} --session {} --provider-authority {provider_pubkey}",
                        row.session_id
                    )
                });
            let (code, remedy) = if row.fenced {
                (
                    "FENCED_BY_HANDOVER",
                    "Another holder has this session, so no custody is staged for it. Nothing to do until it is handed back.",
                )
            } else if !session_present {
                (
                    "SESSION_NOT_HELD",
                    "The provider no longer lists this session. Its next start rewrites this file, so restarting agents should clear the row.",
                )
            } else if row.project_ref.is_none() {
                (
                    "NO_PROJECT",
                    "This generation has no project, so its pack cannot be re-resolved from one. Restoring it needs its original custody; otherwise stop it.",
                )
            } else if !project_trusted_here {
                (
                    "PROJECT_NOT_TRUSTED_HERE",
                    "Its project is not recorded on this Host, so nothing can re-stage it here. Trust the project, or stop this generation.",
                )
            } else {
                (
                    "RESTAGE_NEEDS_DESKTOP",
                    "Restoring it needs the seat's key from the Desktop's Keychain, so only Beekeeper Desktop, open as this Host's owner, can try; this Host cannot tell whether that would succeed for this row. Otherwise stop this generation.",
                )
            };
            RecoveryRow {
                command_id: row.command_id,
                actor: row.actor,
                role: row.role,
                project_ref: row.project_ref,
                session_id: row.session_id,
                generation: row.generation,
                stop_command,
                pack_sha: row.pack_ref.and_then(|pack| pack.sha),
                fenced: row.fenced,
                project_trusted_here,
                session_present,
                code: code.to_string(),
                remedy: remedy.to_string(),
            }
        })
        .collect();
    Recovery {
        rows,
        write_operation: RECOVERY_WRITE_OPERATION.to_string(),
        unavailable: None,
    }
}

/// The attested owner of this Host's provider, or why it could not be verified.
fn owner(config: &HostConfig) -> Result<String, String> {
    let store = beekeeper_host_core::record::load_provider_store_from(&config.record_store_path())?;
    let record = store
        .get(&config.relay_url)
        .ok_or_else(|| "the provider record is missing".to_string())?;
    if record.provider_pubkey != config.provider_pubkey {
        return Err("the provider record and host.json name different identities".to_string());
    }
    let tag = record
        .auth_tag
        .as_deref()
        .ok_or_else(|| "the provider has no owner attestation".to_string())?;
    let key = nostr::PublicKey::from_hex(&config.provider_pubkey)
        .map_err(|_| "the provider public key is invalid".to_string())?;
    beekeeper_sdk::nip_oa::verify_auth_tag(tag, &key)
        .map(|owner| owner.to_hex())
        .map_err(|error| format!("the provider's owner attestation does not verify: {error}"))
}

/// Build the whole answer. Never fails: a part that cannot be read becomes a
/// warning, so an unreadable project file does not hide the running agents.
pub fn read(input: InventoryInput<'_>) -> Inventory {
    let state_dir = &input.config.provider_state_dir;
    let mut warnings = Vec::new();
    let mut warn = |code: &str, message: String| {
        warnings.push(Warning {
            code: code.to_string(),
            message,
        })
    };

    let catalog_file = agent_setup::load_effective_catalog(state_dir).unwrap_or_else(|error| {
        warn("EFFECTIVE_CATALOG_UNREADABLE", error);
        None
    });
    let projects: ProjectsFile = match std::fs::read(state_dir.join(PROJECTS_FILE_NAME)) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
            warn(
                "PROJECTS_FILE_MALFORMED",
                format!("The Host's project list could not be parsed: {error}"),
            );
            ProjectsFile::default()
        }),
        Err(_) => ProjectsFile::default(),
    };
    let (setups, unreadable) = agent_setup::list_setups(state_dir);
    for sentence in unreadable {
        warn("SETUP_JOURNAL_UNREADABLE", sentence);
    }
    let enrolled: Vec<String> =
        match beekeeper_host_core::ownership::all_transfers(input.home, input.instance) {
            Ok(transfers) => transfers.into_iter().map(|t| t.actor_pubkey).collect(),
            Err(error) => {
                warn("OWNERSHIP_JOURNAL_UNREADABLE", error);
                Vec::new()
            }
        };
    let (owner_pubkey, owner_unverified) = match owner(input.config) {
        Ok(owner) => (Some(owner), None),
        Err(reason) => (None, Some(reason)),
    };

    let live_pid = input.provider.live_pid();
    let runtimes = input
        .config
        .runtimes
        .iter()
        .map(|descriptor| runtime_row(descriptor, live_pid, catalog_file.as_ref()))
        .collect();
    let lock_held = agent_setup::apply_lock_held(state_dir);
    let open = agent_setup::open_executions(state_dir);
    let all_open = agent_setup::read_all_open_executions(state_dir).unwrap_or_default();
    let prior_open: Vec<_> = all_open
        .into_iter()
        .filter(|row| {
            agent_setup::adopted_prior_sessions(state_dir, &row.actor).contains(&row.session_id)
        })
        .collect();
    let agents = agent_rows(
        state_dir,
        &input.sessions.sessions,
        &session_projects(state_dir),
        &open,
        &prior_open,
        &enrolled,
        &setups,
    );
    let recovery = recovery(
        state_dir,
        &input.config.provider_pubkey,
        &projects,
        &input.sessions.sessions,
    );
    Inventory {
        read_at: beekeeper_host_core::logs::now_iso(),
        host: HostFacts {
            instance: input.instance,
            host_pid: input.host_pid,
            host_version: input.host_version,
            relay_url: input.config.relay_url.clone(),
            provider_pubkey: input.config.provider_pubkey.clone(),
            owner_pubkey,
            owner_unverified,
            provider: input.provider,
            relay_connection: RelayConnectionState::unknowable(),
        },
        runtimes,
        projects: project_rows(&projects),
        agents,
        setups: setups
            .into_iter()
            .map(|setup| {
                let held = setup
                    .agent_pubkey
                    .as_deref()
                    .is_some_and(|actor| open.iter().any(|row| row.actor == actor));
                SetupRow {
                    effective_state: agent_setup::read_against_host(
                        agent_setup::effective_state(setup.state, lock_held),
                        live_pid.is_some(),
                        held,
                    ),
                    execution_held: held && live_pid.is_some(),
                    setup,
                }
            })
            .collect(),
        recovery,
        warnings,
    }
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
