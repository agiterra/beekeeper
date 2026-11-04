//! The durable record of one Manage Host agent setup, and the provider's local
//! copy of what it actually offers.
//!
//! Both files are **secret-free by construction**. The setup journal holds the
//! operator's choices, pre-minted operation ids and public receipt ids; the
//! identity's key stays in the management operation journal and the provider's
//! custody files, neither of which the Host's inventory opens. That is what
//! lets a key-free Host report setups: it reads this file and nothing that
//! could sign.
//!
//! The journal is written before any effect, so a process killed at any point
//! leaves a record a retry can resume with the same operation ids. A retry
//! never mints a second identity because the id it reuses is the one the
//! management journal already bound to that identity.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_catalog::Catalog;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

/// Directory of setup journals inside the provider state directory.
pub const AGENT_SETUPS_DIR: &str = "agent-setups";
/// Taken exclusively for the length of an apply, shared by a reader that wants
/// to tell a running apply from one whose process died.
pub const AGENT_SETUP_LOCK: &str = "apply.lock";
/// Name of the provider's local copy of its catalog.
pub const EFFECTIVE_CATALOG_FILE: &str = "effective-catalog.json";
/// Schema written by this build.
pub const AGENT_SETUP_VERSION: u32 = 1;
/// Refuses to read a directory an accident has made enormous.
pub const MAX_LISTED_SETUPS: usize = 200;

/// Who may give this agent instructions. `anyone` is deliberately not offered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "pubkeys")]
pub enum SetupAccess {
    /// The attesting owner only.
    OwnerOnly,
    /// The owner, and these named identities.
    Allowlist(Vec<String>),
}

/// Exactly what the operator chose. Immutable once journaled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSetupSpec {
    /// Authorized project coordinate.
    pub project: String,
    /// Channel the agent answers in. The signer must own it.
    pub channel: Uuid,
    /// Role slug in the project's versioned source.
    pub role: String,
    /// Public name.
    pub display_name: String,
    /// Commissioned runtime alias.
    pub provider_instance: String,
    /// Exact model id from the effective catalog.
    pub model: String,
    /// The effort chosen from what the catalog lists for that model. Absent only
    /// for a model that lists none, or a setup journaled before this field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    /// Concurrent responsibilities, including the primary context.
    pub parallelism: u32,
    /// Who may instruct it.
    pub access: SetupAccess,
    /// An identity this Host already holds through `adopt-identity`, to set up
    /// instead of minting one. Absent for every ordinary setup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub existing_actor: Option<String>,
}

impl AgentSetupSpec {
    /// What a session is asked to run: the provider's own `model[effort]` form,
    /// which it applies as the base model and then the effort, and signs back
    /// saying only what it applied.
    pub fn selection(&self) -> String {
        match &self.effort {
            Some(effort) => format!("{}[{effort}]", self.model),
            None => self.model.clone(),
        }
    }

    /// The digest a retry is held to.
    pub fn digest(&self) -> Result<String, String> {
        let bytes = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(hex::encode(Sha256::digest(bytes)))
    }
}

/// Stored state. `Applying` over a released lock is read as interrupted; see
/// [`effective_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    /// Journaled; preflight passed; nothing has been done.
    Planned,
    /// An apply or lifecycle action is, or was, running.
    Applying,
    /// Started, and the signed readback matched the spec.
    Running,
    /// Stopped with a signed receipt; identity retained.
    Stopped,
    /// The last attempt was refused or could not be confirmed. Retryable.
    NeedsAttention,
    /// The operator cancelled; what exists is listed in the record.
    Cancelled,
}

/// One chain step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepName {
    Preflight,
    Install,
    Context,
    Start,
    Verify,
    Stop,
}

/// One step's result. `running` with no process behind it is an interruption.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepRecord {
    pub step: StepName,
    /// `running`, `done` or `failed`.
    pub state: String,
    pub at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A refusal in stable form: the code a client switches on, and the sentence a
/// person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupRefusal {
    pub code: String,
    pub message: String,
    pub at: String,
    /// The accepted operation may have taken effect; retry the same setup
    /// rather than concluding it failed.
    #[serde(default)]
    pub uncertain: bool,
    /// The provider signed a terminal refusal of that attempt, so the next one
    /// may safely use a new operation id. Never true for an unknown outcome.
    #[serde(default)]
    pub attempt_closed: bool,
}

/// A start or stop requested after the initial chain. The id is persisted
/// before use, so a lost reply retries the same operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingAction {
    /// `start` or `stop`.
    pub action: String,
    pub operation_id: Uuid,
    /// Whether this id has been sent. An id never sent has no outcome to wait
    /// for; one that was sent must be resolved before a different action runs.
    #[serde(default)]
    pub attempted: bool,
}

/// An operation this setup stopped using, what closed it, and what replaced it.
/// Only an outcome known to be final closes one: a provider's signed refusal, a
/// signed event the relay no longer takes and does not hold, or a start that
/// landed and has since closed. An unknown outcome is never recorded here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttemptRecord {
    /// `start`, `stop` or `context`.
    pub action: String,
    pub operation_id: Uuid,
    /// A stable code.
    pub reason: String,
    /// What established it.
    pub evidence: String,
    pub replaced_by: Uuid,
    pub at: String,
}

/// A successful start, retained because a retry the provider already adopted leaves
/// that operation's own journal with no target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmedSetupStart {
    /// The setup attempt whose start was successfully observed.
    pub operation_id: Uuid,
    /// The exact execution returned by the management service.
    pub target: beekeeper_core::coding_session_command::CodingSessionTarget,
}

/// The journal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSetup {
    pub version: u32,
    pub setup_id: Uuid,
    pub spec: AgentSetupSpec,
    pub spec_digest: String,
    /// The signer that planned it, equal to the provider's attested owner.
    pub owner: String,
    pub relay_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub state: SetupState,
    /// Pre-minted so an interrupted install resumes the same identity.
    pub install_operation: Uuid,
    /// Pre-minted for the first start.
    pub start_operation: Uuid,
    /// Last successfully observed start, bound to its attempt and execution.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmed_start: Option<ConfirmedSetupStart>,
    /// Pre-minted so an interrupted founding republishes the same genesis.
    pub context_operation: Uuid,
    /// The umbrella the project context is scoped to; not an execution.
    pub session_ref: Uuid,
    /// The founded genesis, once the project context is bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub genesis_event_id: Option<String>,
    /// `bound`, or `not_applicable` for a role the provider gives no project
    /// context. Absent until the step has run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_action: Option<PendingAction>,
    /// Closed attempts, oldest first, so a replaced id can always be traced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attempts: Vec<AttemptRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_pubkey: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt_event_id: Option<String>,
    #[serde(default)]
    pub steps: Vec<StepRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_refusal: Option<SetupRefusal>,
    /// The signed readback the running state rests on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readback: Option<serde_json::Value>,
}

/// What a reader should call a stored state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EffectiveSetupState {
    Planned,
    /// A live process holds the apply lock.
    Applying,
    /// The journal says applying and nothing holds the lock: the process died.
    Interrupted,
    /// Started, and the provider is running and still holds it.
    Running,
    /// Stored as running, and the provider is not running, so neither is it.
    ProviderNotRunning,
    /// Stored as running, and the provider's record of it is closed or gone.
    NotHeld,
    /// Cancelled, and the provider still holds an open execution of it. The
    /// cancel recorded an intent; it did not stop anything.
    CancelledHeld,
    Stopped,
    NeedsAttention,
    Cancelled,
}

/// The journal's directory under a provider state directory.
pub fn setups_dir(provider_state_dir: &Path) -> PathBuf {
    provider_state_dir.join(AGENT_SETUPS_DIR)
}

/// One journal's path.
pub fn setup_path(provider_state_dir: &Path, setup_id: Uuid) -> PathBuf {
    setups_dir(provider_state_dir).join(format!("{setup_id}.json"))
}

/// Atomically replace a journal, creating its restricted directory first.
pub fn save_setup(provider_state_dir: &Path, setup: &AgentSetup) -> Result<(), String> {
    crate::atomic_write::create_dir_all_restricted(&setups_dir(provider_state_dir))?;
    let bytes = serde_json::to_vec_pretty(setup).map_err(|error| error.to_string())?;
    crate::atomic_write::atomic_write_json_restricted(
        &setup_path(provider_state_dir, setup.setup_id),
        &bytes,
    )
}

/// Read one journal; `None` when it does not exist.
pub fn load_setup(provider_state_dir: &Path, setup_id: Uuid) -> Result<Option<AgentSetup>, String> {
    match std::fs::read(setup_path(provider_state_dir, setup_id)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("setup journal {setup_id} is malformed: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("setup journal {setup_id} is unreadable: {error}")),
    }
}

/// Every readable journal, plus a sentence for each that is not. A malformed
/// journal is disclosed rather than skipped: a skipped one would let a second
/// setup mint a second identity.
pub fn list_setups(provider_state_dir: &Path) -> (Vec<AgentSetup>, Vec<String>) {
    let mut setups = Vec::new();
    let mut unreadable = Vec::new();
    let Ok(entries) = std::fs::read_dir(setups_dir(provider_state_dir)) else {
        return (setups, unreadable);
    };
    for entry in entries.flatten().take(MAX_LISTED_SETUPS) {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match std::fs::read(&path)
            .map_err(|error| error.to_string())
            .and_then(|bytes| {
                serde_json::from_slice::<AgentSetup>(&bytes).map_err(|error| error.to_string())
            }) {
            Ok(setup) => setups.push(setup),
            Err(error) => unreadable.push(format!("{}: {error}", path.display())),
        }
    }
    setups.sort_by(|a, b| a.created_at.cmp(&b.created_at));
    (setups, unreadable)
}

/// Take the apply lock, or `None` when another process holds it.
///
/// The operating system releases it however the holder dies, which is what
/// makes `kill -9` during an apply a resumable event rather than a stuck one.
pub fn try_apply_lock(provider_state_dir: &Path) -> Result<Option<std::fs::File>, String> {
    let dir = setups_dir(provider_state_dir);
    crate::atomic_write::create_dir_all_restricted(&dir)?;
    let path = dir.join(AGENT_SETUP_LOCK);
    if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err("the setup lock must not be a symlink".to_string());
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path).map_err(|error| error.to_string())?;
    // Three short attempts: a reader's shared probe holds the lock for
    // microseconds and must not read as a concurrent apply.
    for attempt in 0..3 {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) => {
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error.to_string()),
        }
    }
    Ok(None)
}

/// Whether a live process holds the apply lock right now.
pub fn apply_lock_held(provider_state_dir: &Path) -> bool {
    let Ok(file) = std::fs::File::open(setups_dir(provider_state_dir).join(AGENT_SETUP_LOCK))
    else {
        return false;
    };
    matches!(
        file.try_lock_shared(),
        Err(std::fs::TryLockError::WouldBlock)
    )
}

/// What to call a stored state, given whether the lock is held. A journal cannot
/// know what happened to the agent after it was written; [`read_against_host`]
/// is what a reader with the Host's facts says instead.
pub fn effective_state(stored: SetupState, lock_held: bool) -> EffectiveSetupState {
    match stored {
        SetupState::Planned => EffectiveSetupState::Planned,
        SetupState::Applying if lock_held => EffectiveSetupState::Applying,
        SetupState::Applying => EffectiveSetupState::Interrupted,
        SetupState::Running => EffectiveSetupState::Running,
        SetupState::Stopped => EffectiveSetupState::Stopped,
        SetupState::NeedsAttention => EffectiveSetupState::NeedsAttention,
        SetupState::Cancelled => EffectiveSetupState::Cancelled,
    }
}

/// A journal's state against the Host's own facts. "Running" over a stopped
/// provider is the dishonest-status bug this exists to prevent.
pub fn read_against_host(
    stored: EffectiveSetupState,
    provider_live: bool,
    session_held: bool,
) -> EffectiveSetupState {
    match stored {
        EffectiveSetupState::Running if !provider_live => EffectiveSetupState::ProviderNotRunning,
        EffectiveSetupState::Running if !session_held => EffectiveSetupState::NotHeld,
        EffectiveSetupState::Cancelled if provider_live && session_held => {
            EffectiveSetupState::CancelledHeld
        }
        other => other,
    }
}

/// One execution the provider's record says is open. `closed` is the provider's
/// own word, and the rows it keeps for closed sessions are most of its map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenExecution {
    pub session_id: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    pub generation: u64,
}

/// The file `adopt-identity` writes beside an adopted identity's custody.
pub const ADOPTION_FILE: &str = "adoption.json";

/// What `adopt-identity` recorded about one identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdoptionRecord {
    pub actor: String,
    pub owner: String,
    pub prior_sessions: std::collections::BTreeSet<String>,
    /// When the adoption took its snapshot, as written; absent trusts no fence.
    #[serde(default)]
    pub adopted_at: Option<String>,
}

/// The adoption record, strictly: absent is `Ok(None)`, and anything unreadable,
/// malformed or naming another actor is an error, never a stand-in for absent.
pub fn read_adoption(
    provider_state_dir: &Path,
    actor: &str,
) -> Result<Option<AdoptionRecord>, String> {
    let path = provider_state_dir
        .join("actors")
        .join(actor)
        .join(ADOPTION_FILE);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("the adoption record is unreadable".into()),
    };
    let record: AdoptionRecord = serde_json::from_slice(&bytes)
        .map_err(|_| "the adoption record is malformed".to_string())?;
    if record.actor != actor {
        return Err("the adoption record names another identity".into());
    }
    Ok(Some(record))
}

/// The sessions an identity already had when this Host adopted it. They are not
/// this Host's lineage, and do not count as its open work. For checks that fail
/// closed when these sessions count: a bad record is empty here, so a transition
/// refuses. Anything that would act on old sessions uses [`read_adoption`].
pub fn adopted_prior_sessions(
    provider_state_dir: &Path,
    actor: &str,
) -> std::collections::BTreeSet<String> {
    read_adoption(provider_state_dir, actor)
        .ok()
        .flatten()
        .map(|record| record.prior_sessions)
        .unwrap_or_default()
}

/// [`read_open_executions`], for a reader that discloses its own uncertainty: an
/// unreadable file reads as none.
pub fn open_executions(provider_state_dir: &Path) -> Vec<OpenExecution> {
    read_open_executions(provider_state_dir).unwrap_or_default()
}

/// Every open execution in the provider's snapshot, read narrowly. No file means
/// no provider has recorded anything, which is none. An unreadable one is an
/// error: a caller about to act on "nothing is open" must not guess.
pub fn read_open_executions(provider_state_dir: &Path) -> Result<Vec<OpenExecution>, String> {
    Ok(read_all_open_executions(provider_state_dir)?
        .into_iter()
        .filter(|row| {
            !adopted_prior_sessions(provider_state_dir, &row.actor).contains(&row.session_id)
        })
        .collect())
}

/// Every open record, including an adopted identity's prior sessions, for a reader
/// that shows them separately.
pub fn read_all_open_executions(provider_state_dir: &Path) -> Result<Vec<OpenExecution>, String> {
    #[derive(Deserialize)]
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
        actor: Option<String>,
        #[serde(default)]
        channel_id: Option<String>,
        #[serde(default)]
        generation: u64,
        #[serde(default)]
        closed: bool,
    }
    let bytes = match std::fs::read(provider_state_dir.join("state.json")) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("the provider's state is unreadable: {error}")),
    };
    let snapshot = serde_json::from_slice::<Snapshot>(&bytes)
        .map_err(|error| format!("the provider's state is malformed: {error}"))?;
    Ok(snapshot
        .sessions
        .into_iter()
        .filter(|(_, session)| !session.closed)
        .filter_map(|(key, session)| {
            Some(OpenExecution {
                session_id: if session.session_id.is_empty() {
                    key
                } else {
                    session.session_id
                },
                actor: session.actor?,
                channel_id: session.channel_id,
                generation: session.generation,
            })
        })
        .collect())
}

/// How a runtime's model list came to be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum DiscoveryOutcome {
    /// The adapter answered with its own list. Not proof of a signed-in
    /// account: only a signed turn result is.
    Discovered,
    /// The runtime did not opt in, so the list is what it was commissioned
    /// with and the adapter has not been asked.
    NotRequested,
    /// Discovery ran and failed; the list is the commissioned fallback.
    Failed(String),
    /// The runtime's credential could not be read, so discovery never ran.
    CredentialsUnavailable(String),
}

/// `effective-catalog.json`: what the running provider signed into channels,
/// readable with no relay and no key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EffectiveCatalogFile {
    pub version: u32,
    /// The provider that wrote it, so a reader can tell it from a stale copy.
    pub provider_pid: u32,
    pub written_at: String,
    pub catalog: Catalog,
    /// By `providerInstanceRef`.
    #[serde(default)]
    pub discovery: BTreeMap<String, DiscoveryOutcome>,
}

/// Read the provider's copy; `None` when no provider has written one.
pub fn load_effective_catalog(
    provider_state_dir: &Path,
) -> Result<Option<EffectiveCatalogFile>, String> {
    match std::fs::read(provider_state_dir.join(EFFECTIVE_CATALOG_FILE)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| format!("effective catalog is malformed: {error}")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("effective catalog is unreadable: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> AgentSetupSpec {
        AgentSetupSpec {
            project: "30621:aa:slug".into(),
            channel: Uuid::from_u128(7),
            role: "lead".into(),
            display_name: "Lead".into(),
            provider_instance: "acme-primary".into(),
            model: "acme-test".into(),
            effort: None,
            parallelism: 1,
            access: SetupAccess::OwnerOnly,
            existing_actor: None,
        }
    }

    fn setup(id: u128, state: SetupState) -> AgentSetup {
        AgentSetup {
            version: AGENT_SETUP_VERSION,
            setup_id: Uuid::from_u128(id),
            spec: spec(),
            spec_digest: spec().digest().unwrap(),
            owner: "aa".into(),
            relay_url: "ws://relay".into(),
            created_at: format!("2026-11-02T00:00:0{id}Z"),
            updated_at: "2026-11-02T00:00:00Z".into(),
            state,
            install_operation: Uuid::from_u128(id + 100),
            start_operation: Uuid::from_u128(id + 200),
            confirmed_start: None,
            context_operation: Uuid::from_u128(id + 300),
            session_ref: Uuid::from_u128(id + 400),
            genesis_event_id: None,
            project_context: None,
            pending_action: None,
            attempts: Vec::new(),
            agent_pubkey: None,
            revision: None,
            receipt_event_id: None,
            steps: Vec::new(),
            last_refusal: None,
            readback: None,
        }
    }

    #[test]
    fn a_changed_choice_changes_the_digest_and_the_same_choice_does_not() {
        let base = spec().digest().unwrap();
        assert_eq!(base, spec().digest().unwrap());
        let mut changed = spec();
        changed.model = "other".into();
        assert_ne!(base, changed.digest().unwrap());
        let mut widened = spec();
        widened.access = SetupAccess::Allowlist(vec!["bb".into()]);
        assert_ne!(base, widened.digest().unwrap());
    }

    #[test]
    fn a_spec_with_an_unknown_field_is_refused_rather_than_ignored() {
        let mut value = serde_json::to_value(spec()).unwrap();
        value["anyone"] = true.into();
        assert!(serde_json::from_value::<AgentSetupSpec>(value).is_err());
    }

    #[test]
    fn a_journal_round_trips_and_a_malformed_one_is_disclosed_not_skipped() {
        let dir = tempfile::tempdir().unwrap();
        save_setup(dir.path(), &setup(1, SetupState::Planned)).unwrap();
        save_setup(dir.path(), &setup(2, SetupState::Running)).unwrap();
        std::fs::write(setups_dir(dir.path()).join("broken.json"), b"{").unwrap();
        std::fs::write(setups_dir(dir.path()).join(AGENT_SETUP_LOCK), b"").unwrap();
        let (setups, unreadable) = list_setups(dir.path());
        assert_eq!(setups.len(), 2);
        assert_eq!(setups[0].setup_id, Uuid::from_u128(1));
        assert_eq!(unreadable.len(), 1);
        assert!(unreadable[0].contains("broken.json"));
        assert_eq!(
            load_setup(dir.path(), Uuid::from_u128(2)).unwrap().unwrap(),
            setup(2, SetupState::Running)
        );
        assert!(load_setup(dir.path(), Uuid::from_u128(9))
            .unwrap()
            .is_none());
    }

    /// Runs in a process of its own. `flock` belongs to the open file
    /// description, and while another thread's `Command::spawn` is between
    /// creating its child and `exec` the child holds a copy of every descriptor,
    /// so a sibling test that spawns keeps the lock alive past `drop` (CI 392).
    #[test]
    fn the_lock_excludes_a_second_apply_and_falls_with_its_holder() {
        const CHILD: &str = "BEEKEEPER_AGENT_SETUP_LOCK_TEST_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "agent_setup::tests::the_lock_excludes_a_second_apply_and_falls_with_its_holder",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            assert!(
                output.status.success() && stdout.contains("1 passed"),
                "{stdout}\n{}",
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        assert!(!apply_lock_held(dir.path()), "no file, no holder");
        let held = try_apply_lock(dir.path()).unwrap().expect("first");
        assert!(apply_lock_held(dir.path()));
        assert!(try_apply_lock(dir.path()).unwrap().is_none());
        drop(held);
        assert!(!apply_lock_held(dir.path()));
        assert!(try_apply_lock(dir.path()).unwrap().is_some());
    }

    #[test]
    fn a_journal_that_says_running_yields_to_the_hosts_facts() {
        let running = EffectiveSetupState::Running;
        assert_eq!(read_against_host(running, true, true), running);
        assert_eq!(
            read_against_host(running, false, true),
            EffectiveSetupState::ProviderNotRunning
        );
        assert_eq!(
            read_against_host(running, true, false),
            EffectiveSetupState::NotHeld
        );
        assert_eq!(
            read_against_host(EffectiveSetupState::Stopped, false, false),
            EffectiveSetupState::Stopped,
            "a stopped agent is stopped whatever the provider is doing"
        );
        let cancelled = EffectiveSetupState::Cancelled;
        assert_eq!(
            read_against_host(cancelled, true, true),
            EffectiveSetupState::CancelledHeld
        );
        assert_eq!(read_against_host(cancelled, true, false), cancelled);
    }

    #[test]
    fn only_an_open_record_with_an_actor_counts_as_an_execution() {
        let dir = tempfile::tempdir().unwrap();
        assert!(open_executions(dir.path()).is_empty());
        std::fs::write(
            dir.path().join("state.json"),
            serde_json::json!({"sessions": {
                "open": {"sessionId": "open", "actor": "aa", "channelId": "c", "generation": 2},
                "closed": {"sessionId": "closed", "actor": "aa", "closed": true},
                "human": {"sessionId": "human"},
            }})
            .to_string(),
        )
        .unwrap();
        let open = open_executions(dir.path());
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].session_id, "open");
        assert_eq!(open[0].generation, 2);
        std::fs::write(dir.path().join("state.json"), b"{").unwrap();
        assert!(
            read_open_executions(dir.path()).is_err(),
            "not read as none"
        );
    }

    #[test]
    fn applying_over_a_released_lock_reads_as_interrupted() {
        assert_eq!(
            effective_state(SetupState::Applying, true),
            EffectiveSetupState::Applying
        );
        assert_eq!(
            effective_state(SetupState::Applying, false),
            EffectiveSetupState::Interrupted
        );
        assert_eq!(
            effective_state(SetupState::Running, false),
            EffectiveSetupState::Running
        );
    }

    /// What a session is asked to run. The digest binds the effort, so a retry
    /// cannot quietly run another one, and a journal from before the field reads
    /// and digests as it always did.
    #[test]
    fn the_effort_is_part_of_the_selection_and_the_digest() {
        let plain = spec();
        assert_eq!(plain.selection(), plain.model);
        let bytes = serde_json::to_value(&plain).unwrap();
        assert!(bytes.get("effort").is_none());
        let low = AgentSetupSpec {
            effort: Some("low".into()),
            ..plain.clone()
        };
        assert_eq!(low.selection(), format!("{}[low]", plain.model));
        assert_ne!(low.digest().unwrap(), plain.digest().unwrap());
        let back: AgentSetupSpec =
            serde_json::from_value(serde_json::to_value(&low).unwrap()).unwrap();
        assert_eq!(back, low);
    }
}
