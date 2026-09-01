//! Durable provider state: watermarks, command dedupe, session records,
//! per-generation sequence counters, and the catalog revision.
//!
//! Four files, all under `BUZZ_CSP_STATE_DIR`:
//!
//! - `state.json` — the whole mutable snapshot, replaced atomically
//!   (write to a temp file, fsync, rename). A torn write can therefore never
//!   produce a half-updated sequence counter.
//! - `commands.jsonl` — append-only record of consumed `commandId`s. Append is
//!   the right shape here because the only question ever asked is "have I
//!   already acted on this?", and an append cannot lose earlier answers.
//! - `refusals.jsonl` — append-only record of `commandId`s answered with a
//!   refusal, so a redelivery cannot republish the same answer.
//! - `operations.jsonl` — append-only record of team-wake *operations* this
//!   provider has started a turn for, one JSON object per line:
//!   `{"key": <operation fence key>, "commandId": <owner>, "at": <unix secs>}`.
//!   The key is [`crate::team_wake::operation_fence_key`]'s value: the exact
//!   target generation plus the canonicalised wake pointer. Two commands with
//!   different ids can carry the same pointer — the provider's wake sender and
//!   Desktop's fallback deliberately do — and only one of them may spend a
//!   lead turn on it.
//!
//! # The sequence-counter invariant
//!
//! `event_seq` is allocated **and durably persisted before** the event that
//! uses it is published. That ordering makes gaps possible — a crash between
//! reserving 5 and publishing it burns 5 — and duplicates impossible. That is
//! the correct trade: the consumer's `cst-key` dedupe treats two different
//! items at the same sequence as a conflict it must surface, while a gap is
//! simply a sequence it never sees.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_routing::RoutingRecord;

use crate::session::BootstrapTransport;

/// On-disk format version for `state.json`.
pub const STATE_VERSION: u32 = 1;

const STATE_FILE: &str = "state.json";
const COMMANDS_FILE: &str = "commands.jsonl";
/// Append-only record of `commandId`s this provider answered with a refusal.
///
/// Separate from [`COMMANDS_FILE`] because the two answer different questions.
/// A *consumed* command started a turn and must never start a second one. A
/// *refused* command never ran and never will: remembering it stops a relay
/// redelivery from publishing a byte-identical `turn_refused` a second time,
/// which is the difference between a refusal and a stutter.
const REFUSALS_FILE: &str = "refusals.jsonl";
/// Append-only record of team-wake operations a turn has actually *started*
/// for, keyed by [`crate::team_wake::operation_fence_key`].
///
/// Separate from [`COMMANDS_FILE`] because it answers a different question.
/// The command ledger says "did *this command* run"; this one says "is this
/// *operation* already somebody's". A command id is producer-specific, and
/// two producers mint different ids for one operation on purpose, so the
/// command ledger cannot answer it.
const OPERATIONS_FILE: &str = "operations.jsonl";
/// Single-instance lock file. See [`acquire_state_dir_lock`].
pub const LOCK_FILE: &str = "provider.lock";

/// Exclusive single-instance lock on one provider state directory.
///
/// Held for the owning process's lifetime; the OS releases it when the file
/// handle closes, including on a crash, so a stale lock can never outlive its
/// process. Dropping this value releases the lock.
#[derive(Debug)]
pub struct StateDirLock {
    // Held only for its advisory lock; the handle itself is never read again.
    _file: File,
}

impl Drop for StateDirLock {
    fn drop(&mut self) {
        // Release explicitly before closing the descriptor so callers can
        // immediately reacquire on every supported platform.
        let _ = self._file.unlock();
    }
}

/// Take the exclusive advisory lock that makes this process the *only*
/// provider allowed to touch `dir`.
///
/// Two providers sharing one state directory consume each command up to twice
/// (one create → several sessions) and interleave appends into the same
/// ledger files, physically corrupting them. This is the same
/// at-most-one-live-instance rule the managed-agents tier already enforces
/// (docs/remote-agents.md, invariant I4), applied to coding-session providers.
///
/// On conflict the error names the directory and, when readable, the pid the
/// current owner recorded — callers must fail fast and exit nonzero rather
/// than proceed unlocked. On success the caller's pid is recorded in the lock
/// file so a supervisor can identify (and take over from) an orphaned owner.
pub fn acquire_state_dir_lock(dir: &Path) -> io::Result<StateDirLock> {
    fs::create_dir_all(dir)?;
    restrict_directory(dir)?;
    let path = dir.join(LOCK_FILE);
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    restrict_new_file(&mut options);
    let mut file = options.open(&path)?;
    restrict_file(&path)?;
    if let Err(error) = file.try_lock() {
        let owner = fs::read_to_string(&path).unwrap_or_default();
        let owner = owner.trim();
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            format!(
                "another buzz-session-provider instance already owns {} (owner pid: {}): {error}",
                dir.display(),
                if owner.is_empty() { "unknown" } else { owner },
            ),
        ));
    }
    // Recorded only while the lock is held, so the pid is always the owner's.
    file.set_len(0)?;
    file.write_all(format!("{}\n", std::process::id()).as_bytes())?;
    file.sync_all()?;
    Ok(StateDirLock { _file: file })
}

/// Host-local record of one session generation this provider owns.
///
/// `cwd` is deliberately here and nowhere else: it is machine-local execution
/// state, and signed content must never carry a filesystem path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    /// Producer-minted session id (a UUID); half of the `cs-target`.
    pub session_id: String,
    /// Generation number. Starts at 1 and advances on each provider reattach.
    pub generation: u64,
    /// Channel every event for this session is published into.
    pub channel_id: Uuid,
    /// The create command that minted this session.
    pub command_id: String,
    /// Lifecycle command that minted the current exact generation.
    ///
    /// `None` is the backward-compatible representation for records written
    /// before generation commands were persisted; generation one was minted
    /// by [`SessionRecord::command_id`] in those records.
    #[serde(default)]
    pub generation_command_id: Option<String>,
    /// The runtime instance that serves this session. Defaults to the claude
    /// ref for records written before the field existed.
    #[serde(default = "default_provider_instance_ref")]
    pub provider_instance_ref: String,
    /// Runtime slug behind the driver. Defaults for pre-field records.
    #[serde(default = "default_runtime_slug")]
    pub runtime: String,
    /// Driver slug minted into every `cs-target` for this generation. Persisted
    /// so the wire identity survives the descriptor disappearing (adapter
    /// uninstalled across a restart). Defaults to the claude driver for records
    /// written before the field existed — the only driver those can have used.
    #[serde(default = "default_driver")]
    pub driver: String,
    /// Working directory the agent runs in. Never serialized into an event.
    pub cwd: PathBuf,
    /// NIP-MP project coordinate, or `None` for a standalone session.
    pub project_ref: Option<String>,
    /// Repository coordinate, or `None`.
    pub repo_ref: Option<String>,
    /// Umbrella session reference from the create, echoed into every 44223
    /// for this session. Defaults to `None` for records written before the
    /// field existed — those creates could not have claimed an umbrella.
    #[serde(default)]
    pub session_ref: Option<String>,
    /// Explicit genesis event id for authority-aware sessions.
    #[serde(default)]
    pub genesis_ref: Option<String>,
    /// The agent seat this execution runs as, lowercase 64-hex, or `None` for
    /// a human-created execution (plan D1).
    ///
    /// The pubkey only. The seat's key material is one-shot host-local custody
    /// ([`crate::actor_seats`]) and is deliberately *not* persisted here: a
    /// record that carried it would put a signing key in `state.json` for the
    /// lifetime of the execution, which is exactly the at-rest credential the
    /// custody file exists to avoid. Defaults to `None` for records written
    /// before the field existed.
    #[serde(default)]
    pub actor: Option<String>,
    /// The role slug this seat holds within its umbrella, or `None`.
    ///
    /// Set exactly when [`SessionRecord::actor`] is. Republished in every
    /// 44223 for this generation and consulted for the umbrella's `lead`
    /// interrupt authority (plan D7).
    #[serde(default)]
    pub role: Option<String>,
    /// Founder pubkey resolved from genesis, or the locally witnessed create
    /// signer for legacy sessions. `None` is retained for pre-field records.
    #[serde(default)]
    pub founder_pubkey: Option<String>,
    /// Operator pubkeys granted steering authority by this session's accepted
    /// authority chain (kind 44228 `grant-operator` transitions, each applied
    /// only after its relay-signed kind 40099 acceptance receipt and the
    /// referenced transition event were verified — see `crate::authority`).
    ///
    /// A verified local cache in the R21 sense: every entry was witnessed and
    /// checked by this provider at the moment it was applied, and the set is
    /// re-extended by backfill on load. Meaningful only when `genesis_ref` is
    /// set — enforcement never consults it for legacy sessions. Defaults empty
    /// for records written before the field existed.
    #[serde(default)]
    pub granted_operators: BTreeSet<String>,
    /// Pubkeys holding a live `grant-viewer` — read-only session shares.
    /// Never consulted for steering; tracked for diagnostics and so a later
    /// `revoke` of a viewer folds cleanly. Defaults empty for pre-field
    /// records.
    #[serde(default)]
    pub granted_viewers: BTreeSet<String>,
    /// Highest accepted authority-chain `seq` whose grant has been applied to
    /// `granted_operators`. `0` means no transition has been applied. Grants
    /// apply strictly contiguously (`seq == authority_seq + 1`); a gap
    /// triggers a backfill rather than a guess. Defaults to 0 for pre-field
    /// records.
    #[serde(default)]
    pub authority_seq: u32,
    /// Requested model, or `None` to let the adapter decide.
    pub model: Option<String>,
    /// The host's routing decision from the create, or `None` for an
    /// unrouted or pre-field session.
    #[serde(default)]
    pub routing: Option<RoutingRecord>,
    /// Opaque ACP session id used only to reattach this host's adapter.
    /// Never published or passed through the adapter environment.
    #[serde(default)]
    pub resume_cursor: Option<String>,
    /// Operator-facing title, or `None`.
    pub title: Option<String>,
    /// Creation time, milliseconds since the Unix epoch.
    pub created_at_ms: i64,
    /// Next `event_seq` to hand out. Sequences start at 1.
    pub next_seq: u64,
    /// Next ephemeral lease sequence to reserve for this exact generation.
    #[serde(default = "default_next_lease_sequence")]
    pub next_lease_sequence: u64,
    /// How this generation's rehydration continuity bootstrap was delivered, or
    /// `None` when the open needed no bootstrap. Host-local diagnostic: never
    /// published, and defaults to `None` for records written before the field
    /// existed.
    #[serde(default)]
    pub bootstrap_transport: Option<BootstrapTransport>,
    /// The turn currently believed to be in flight, if any.
    pub open_turn: Option<OpenTurn>,
    /// Whether this generation has been retired (no further turns accepted).
    pub closed: bool,
}

fn default_provider_instance_ref() -> String {
    crate::config::PROVIDER_INSTANCE_REF.to_owned()
}

fn default_runtime_slug() -> String {
    crate::config::RUNTIME.to_owned()
}

fn default_driver() -> String {
    crate::config::DRIVER.to_owned()
}

fn default_next_lease_sequence() -> u64 {
    1
}

fn legacy_open_turn_is_team_wake_eligible() -> bool {
    true
}

impl SessionRecord {
    /// The wire target naming this generation, minted with the persisted driver.
    pub fn target(&self, instance_id: &str) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: self.driver.clone(),
            instance_id: instance_id.to_owned(),
            session_id: self.session_id.clone(),
            generation: self.generation,
        }
    }

    /// Lifecycle command that minted this exact generation.
    pub fn generation_command_id(&self) -> &str {
        self.generation_command_id
            .as_deref()
            .unwrap_or(&self.command_id)
    }
}

/// A turn believed to be in flight, persisted so a crash can be repaired.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenTurn {
    /// Producer-minted turn id carried on every item of the turn.
    pub turn_id: String,
    /// The command that opened the turn, when one did.
    pub command_id: Option<String>,
    /// Whether the command was a verified kind-44220 thread turn.
    ///
    /// Lifecycle create/hire prompts also open model turns, but they can never
    /// name an assignment operation. Persisting the distinction prevents a
    /// lifecycle terminal from occupying the durable team-wake queue forever.
    #[serde(default = "legacy_open_turn_is_team_wake_eligible")]
    pub team_wake_eligible: bool,
    /// Turn start, milliseconds since the Unix epoch.
    pub started_at_ms: i64,
}

/// Persisted catalog advertisement state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogState {
    /// Monotonic revision last advertised. `0` means nothing published yet.
    pub revision: u64,
    /// SHA-256 of the canonical bytes behind `revision`.
    pub content_digest: Option<String>,
    /// Channels the current revision has already been advertised into.
    #[serde(default)]
    pub advertised_channels: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    version: u32,
    #[serde(default)]
    watermarks: BTreeMap<Uuid, u64>,
    #[serde(default)]
    sessions: BTreeMap<String, SessionRecord>,
    #[serde(default)]
    catalog: CatalogState,
    /// Turns started per umbrella (`sessionRef`), the durable half of the D9
    /// budget. Keyed by umbrella rather than by session because the budget is
    /// a bound on the *crew*: five seats sharing one `sessionRef` spend one
    /// allowance between them, and a seat that is stopped and recreated does
    /// not get a fresh one. Absent for every umbrella that has never started
    /// a turn, which reads as zero.
    #[serde(default)]
    turn_budget_used: BTreeMap<String, u64>,
    /// The founder of each umbrella (`sessionRef`) this provider has minted an
    /// execution under, recorded at the first such create and never moved.
    ///
    /// The D9 budget exempts the founder, and "founder" has to mean the
    /// *umbrella's* founder rather than each execution's own: a delegated seat
    /// that creates a session is the founder of that session, so an
    /// execution-scoped exemption would let one signed create buy an unbounded
    /// allowance while still charging the umbrella. Absent for umbrellas
    /// recorded before this field existed, which resolve from the earliest
    /// session record instead.
    #[serde(default)]
    umbrella_founders: BTreeMap<String, String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            watermarks: BTreeMap::new(),
            sessions: BTreeMap::new(),
            catalog: CatalogState::default(),
            turn_budget_used: BTreeMap::new(),
            umbrella_founders: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandRecord {
    command_id: String,
    at: u64,
}

/// One line of [`OPERATIONS_FILE`]: the operation, the command that owns it,
/// and when the owner started.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OperationRecord {
    key: String,
    command_id: String,
    at: u64,
}

/// Durable provider state rooted at one directory.
#[derive(Debug)]
pub struct StateStore {
    dir: PathBuf,
    snapshot: Snapshot,
    commands: HashSet<String>,
    refusals: HashSet<String>,
    /// Team-wake operation key → the `commandId` that owns it.
    operations: HashMap<String, String>,
}

impl StateStore {
    /// Open (creating if absent) the state directory and load everything in it.
    ///
    /// `command_retention_secs` drops consumed command ids older than the
    /// freshness horizon: a command that old is ignored on its own merits, so
    /// remembering it buys nothing and the ledger would grow without bound.
    pub fn open(dir: &Path, command_retention_secs: u64) -> io::Result<Self> {
        fs::create_dir_all(dir)?;
        restrict_directory(dir)?;
        for file in [STATE_FILE, COMMANDS_FILE, REFUSALS_FILE, OPERATIONS_FILE] {
            restrict_file_if_present(&dir.join(file))?;
        }
        let snapshot = load_snapshot(&dir.join(STATE_FILE))?;
        let mut store = Self {
            dir: dir.to_path_buf(),
            snapshot,
            commands: HashSet::new(),
            refusals: HashSet::new(),
            operations: HashMap::new(),
        };
        store.load_commands(command_retention_secs)?;
        Ok(store)
    }

    /// Newest `created_at` already consumed in a channel, if any.
    pub fn watermark(&self, channel_id: Uuid) -> Option<u64> {
        self.snapshot.watermarks.get(&channel_id).copied()
    }

    /// Advance a channel watermark. Never moves backwards.
    pub fn record_watermark(&mut self, channel_id: Uuid, created_at: u64) -> io::Result<()> {
        let previous = self.snapshot.clone();
        let entry = self.snapshot.watermarks.entry(channel_id).or_insert(0);
        if created_at <= *entry {
            return Ok(());
        }
        *entry = created_at;
        self.persist_or_restore(previous)
    }

    /// Whether this command has already been acted on.
    pub fn is_command_consumed(&self, command_id: &str) -> bool {
        self.commands.contains(command_id)
    }

    /// Record a command as consumed, durably, at the moment its side effect
    /// *begins*.
    ///
    /// For a turn that is the point the turn starts running, not the point it
    /// was accepted into the mailbox. A command accepted and not yet started is
    /// deliberately absent from this ledger: a crash before it ran leaves it
    /// unconsumed, so the replay from the channel watermark re-delivers it and
    /// the turn is not lost. See [`crate::Provider::handle_session_event`]'s
    /// `TurnStarted` arm.
    pub fn consume_command(&mut self, command_id: &str, at: u64) -> io::Result<()> {
        Self::append_ledger_record(
            &self.dir.join(COMMANDS_FILE),
            &mut self.commands,
            command_id,
            at,
        )
    }

    /// The `commandId` that durably owns one team-wake operation, if any.
    ///
    /// `key` comes from [`crate::team_wake::operation_fence_key`]. `None`
    /// means no turn has ever *started* for this operation on this target
    /// generation — which is the same answer for "never seen" and for "the
    /// owner was dropped before it ran", and deliberately so: a fence that
    /// outlived its owner's failure would lose the operation permanently.
    pub fn operation_owner(&self, key: &str) -> Option<&str> {
        self.operations.get(key).map(String::as_str)
    }

    /// Durably record that `command_id` owns the operation named by `key`.
    ///
    /// Idempotent for the same key: the first writer wins and a later call —
    /// including one after a restart replayed the owner — appends nothing.
    /// Written at the moment the turn *starts*, beside
    /// [`Self::consume_command`], because that is the moment the operation
    /// actually costs the lead a turn.
    pub fn consume_operation(&mut self, key: &str, command_id: &str, at: u64) -> io::Result<()> {
        if self.operations.contains_key(key) {
            return Ok(());
        }
        self.operations
            .insert(key.to_owned(), command_id.to_owned());
        let record = OperationRecord {
            key: key.to_owned(),
            command_id: command_id.to_owned(),
            at,
        };
        let result = (|| {
            let path = self.dir.join(OPERATIONS_FILE);
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            restrict_new_file(&mut options);
            let mut file = options.open(&path)?;
            restrict_file(&path)?;
            // One `write` on an O_APPEND handle, for the same reason the
            // command ledger takes one: a line can never be torn in half.
            let mut line = serde_json::to_string(&record)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
            file.sync_all()
        })();
        if result.is_err() {
            // The claim is not durable, so it is not a claim: roll the
            // in-memory half back rather than fencing on a record a restart
            // will not find.
            self.operations.remove(key);
        }
        result
    }

    /// Whether this command was already answered with a refusal.
    pub fn is_command_refused(&self, command_id: &str) -> bool {
        self.refusals.contains(command_id)
    }

    /// Record a command as refused, durably, before its refusal is published.
    ///
    /// A refused command never ran, so it must not be *consumed* — consumed
    /// means "this one started". It must also never be answered twice: the
    /// outbox fences duplicates only within one process lifetime, so without a
    /// durable set a restart plus a relay redelivery republishes the same
    /// refusal under the same semantic key.
    pub fn record_refusal(&mut self, command_id: &str, at: u64) -> io::Result<()> {
        Self::append_ledger_record(
            &self.dir.join(REFUSALS_FILE),
            &mut self.refusals,
            command_id,
            at,
        )
    }

    fn append_ledger_record(
        path: &Path,
        seen: &mut HashSet<String>,
        command_id: &str,
        at: u64,
    ) -> io::Result<()> {
        if !seen.insert(command_id.to_owned()) {
            return Ok(());
        }
        let record = CommandRecord {
            command_id: command_id.to_owned(),
            at,
        };
        let result = (|| {
            let mut options = OpenOptions::new();
            options.create(true).append(true);
            restrict_new_file(&mut options);
            let mut file = options.open(path)?;
            restrict_file(path)?;
            // The whole line — payload and newline — goes down in a single
            // `write` call on an O_APPEND handle, so even a second writer (a bug
            // the state-dir lock exists to prevent) could not tear it in half.
            let mut line = serde_json::to_string(&record)?;
            line.push('\n');
            file.write_all(line.as_bytes())?;
            file.sync_all()
        })();
        if result.is_err() {
            seen.remove(command_id);
        }
        result
    }

    /// Every session record this provider has ever minted and not pruned.
    pub fn sessions(&self) -> impl Iterator<Item = &SessionRecord> {
        self.snapshot.sessions.values()
    }

    /// Turns this provider has started under one umbrella, ever.
    ///
    /// Zero for an umbrella it has never run a turn for. This is deliberately
    /// a count of turns that *began*, not of commands accepted: a turn that
    /// was queued and lost to a crash cost the crew nothing and must not
    /// spend its budget.
    pub fn turns_used(&self, session_ref: &str) -> u64 {
        self.snapshot
            .turn_budget_used
            .get(session_ref)
            .copied()
            .unwrap_or(0)
    }

    /// Who founded an umbrella, as this provider witnessed it.
    ///
    /// The recorded claim first; failing that, the founder of the earliest
    /// session record claiming the same `sessionRef`, which is what an
    /// umbrella minted before the claim was persisted resolves to. `None` when
    /// this provider has never minted an execution under that umbrella, or
    /// when the only records for it predate `founderPubkey`.
    pub fn umbrella_founder(&self, session_ref: &str) -> Option<String> {
        if let Some(founder) = self.snapshot.umbrella_founders.get(session_ref) {
            return Some(founder.clone());
        }
        self.snapshot
            .sessions
            .values()
            .filter(|record| record.session_ref.as_deref() == Some(session_ref))
            .filter(|record| record.founder_pubkey.is_some())
            .min_by(|left, right| {
                (left.created_at_ms, &left.session_id)
                    .cmp(&(right.created_at_ms, &right.session_id))
            })
            .and_then(|record| record.founder_pubkey.clone())
    }

    /// Record `founder` as an umbrella's founder if none is known yet, and
    /// return whoever holds the claim afterwards.
    ///
    /// First claim wins and is never overwritten: the umbrella belongs to
    /// whoever opened it, and a later create — including one signed by a seat
    /// the founder delegated to — must not be able to take it over and exempt
    /// itself from the budget that delegation is bounded by.
    pub fn claim_umbrella_founder(
        &mut self,
        session_ref: &str,
        founder: &str,
    ) -> io::Result<String> {
        let resolved = self
            .umbrella_founder(session_ref)
            .unwrap_or_else(|| founder.to_owned());
        if self.snapshot.umbrella_founders.get(session_ref) == Some(&resolved) {
            return Ok(resolved);
        }
        let previous = self.snapshot.clone();
        self.snapshot
            .umbrella_founders
            .insert(session_ref.to_owned(), resolved.clone());
        self.persist_or_restore(previous)?;
        Ok(resolved)
    }

    /// Charge one started turn to an umbrella and persist before returning.
    ///
    /// Returns the new total. Persisted first for the same reason the sequence
    /// counters are: a crash may burn a count it never published, which is
    /// harmless, while a count that is published and then forgotten would hand
    /// a restarted crew a budget it had already spent. Saturating because a
    /// wrapped counter would silently refill the allowance.
    pub fn record_turn_spend(&mut self, session_ref: &str) -> io::Result<u64> {
        let previous = self.snapshot.clone();
        let entry = self
            .snapshot
            .turn_budget_used
            .entry(session_ref.to_owned())
            .or_insert(0);
        *entry = entry.saturating_add(1);
        let used = *entry;
        self.persist_or_restore(previous)?;
        Ok(used)
    }

    /// Number of sessions still accepting turns.
    pub fn live_session_count(&self) -> usize {
        self.snapshot
            .sessions
            .values()
            .filter(|session| !session.closed)
            .count()
    }

    /// Look up one session by its producer-minted id.
    pub fn session(&self, session_id: &str) -> Option<&SessionRecord> {
        self.snapshot.sessions.get(session_id)
    }

    /// Insert a freshly minted session record.
    pub fn insert_session(&mut self, record: SessionRecord) -> io::Result<()> {
        let previous = self.snapshot.clone();
        self.snapshot
            .sessions
            .insert(record.session_id.clone(), record);
        self.persist_or_restore(previous)
    }

    /// Mutate a session record in place and persist the result.
    ///
    /// Returns `false` when no such session exists, so callers can distinguish
    /// "updated" from "the session went away underneath me".
    pub fn update_session(
        &mut self,
        session_id: &str,
        mutate: impl FnOnce(&mut SessionRecord),
    ) -> io::Result<bool> {
        let previous = self.snapshot.clone();
        match self.snapshot.sessions.get_mut(session_id) {
            None => Ok(false),
            Some(record) => {
                mutate(record);
                self.persist_or_restore(previous)?;
                Ok(true)
            }
        }
    }

    /// Reserve the next `event_seq` for a generation and persist it *before*
    /// returning, so a crash can only ever burn the number, never reuse it.
    pub fn allocate_seq(&mut self, session_id: &str) -> io::Result<Option<u64>> {
        let previous = self.snapshot.clone();
        let Some(record) = self.snapshot.sessions.get_mut(session_id) else {
            return Ok(None);
        };
        let seq = record.next_seq.max(1);
        record.next_seq = seq + 1;
        self.persist_or_restore(previous)?;
        Ok(Some(seq))
    }

    /// Reserve and persist the next lease sequence for an exact generation.
    ///
    /// Persistence completes before the number is returned. A crash may leave
    /// a harmless gap, but can never reuse a signed lease sequence.
    pub fn allocate_lease_sequence(&mut self, session_id: &str) -> io::Result<Option<u64>> {
        let previous = self.snapshot.clone();
        let Some(record) = self.snapshot.sessions.get_mut(session_id) else {
            return Ok(None);
        };
        let sequence = record.next_lease_sequence.max(1);
        if sequence > buzz_core::coding_session_command::MAX_SAFE_GENERATION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "coding-session lease sequence is exhausted",
            ));
        }
        record.next_lease_sequence = sequence.checked_add(1).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "coding-session lease sequence is exhausted",
            )
        })?;
        self.persist_or_restore(previous)?;
        Ok(Some(sequence))
    }

    /// Current catalog advertisement state.
    pub fn catalog(&self) -> &CatalogState {
        &self.snapshot.catalog
    }

    /// Replace the catalog advertisement state.
    pub fn set_catalog(&mut self, catalog: CatalogState) -> io::Result<()> {
        let previous = self.snapshot.clone();
        self.snapshot.catalog = catalog;
        self.persist_or_restore(previous)
    }

    fn load_commands(&mut self, retention_secs: u64) -> io::Result<()> {
        let dir = self.dir.clone();
        Self::load_ledger(&dir.join(COMMANDS_FILE), &mut self.commands, retention_secs)?;
        Self::load_ledger(&dir.join(REFUSALS_FILE), &mut self.refusals, retention_secs)?;
        Self::load_operation_ledger(
            &dir.join(OPERATIONS_FILE),
            &mut self.operations,
            retention_secs,
        )
    }

    /// Load [`OPERATIONS_FILE`], dropping records past the freshness horizon
    /// and rewriting the file when anything was dropped.
    ///
    /// Same retention as the command ledger, for the same reason: a command
    /// older than the horizon is ignored on its own merits (`PastHorizon`), so
    /// an operation whose owner is that old can never be duplicated by a
    /// command the provider would still admit, and remembering it forever
    /// would grow the file without bound.
    fn load_operation_ledger(
        path: &Path,
        seen: &mut HashMap<String, String>,
        retention_secs: u64,
    ) -> io::Result<()> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let cutoff = now_secs().saturating_sub(retention_secs);
        let mut kept: Vec<OperationRecord> = Vec::new();
        let mut dropped = false;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<OperationRecord>(&line) {
                Ok(record) if record.at >= cutoff => {
                    // First writer wins, exactly as `consume_operation` does,
                    // so a rewritten file cannot change who owns what.
                    if seen.contains_key(&record.key) {
                        dropped = true;
                    } else {
                        seen.insert(record.key.clone(), record.command_id.clone());
                        kept.push(record);
                    }
                }
                Ok(_) => dropped = true,
                Err(error) => {
                    tracing::warn!(target: "csp::state", "dropping unreadable operation ledger line: {error}");
                    dropped = true;
                }
            }
        }
        if dropped {
            let mut body = String::new();
            for record in &kept {
                body.push_str(&serde_json::to_string(record)?);
                body.push('\n');
            }
            atomic_write(path, body.as_bytes())?;
        }
        Ok(())
    }

    /// Load one append-only `commandId` ledger, dropping records past the
    /// freshness horizon and rewriting the file when anything was dropped.
    fn load_ledger(path: &Path, seen: &mut HashSet<String>, retention_secs: u64) -> io::Result<()> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let cutoff = now_secs().saturating_sub(retention_secs);
        let mut kept: Vec<CommandRecord> = Vec::new();
        let mut dropped = false;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<CommandRecord>(&line) {
                Ok(record) if record.at >= cutoff => {
                    if seen.insert(record.command_id.clone()) {
                        kept.push(record);
                    } else {
                        dropped = true;
                    }
                }
                Ok(_) => dropped = true,
                Err(error) => {
                    tracing::warn!(target: "csp::state", "dropping unreadable command ledger line: {error}");
                    dropped = true;
                }
            }
        }
        if dropped {
            let mut body = String::new();
            for record in &kept {
                body.push_str(&serde_json::to_string(record)?);
                body.push('\n');
            }
            atomic_write(path, body.as_bytes())?;
        }
        Ok(())
    }

    fn persist(&self) -> io::Result<()> {
        let body = serde_json::to_vec_pretty(&self.snapshot)?;
        atomic_write(&self.dir.join(STATE_FILE), &body)
    }

    fn persist_or_restore(&mut self, previous: Snapshot) -> io::Result<()> {
        if let Err(error) = self.persist() {
            self.snapshot = previous;
            return Err(error);
        }
        Ok(())
    }
}

fn load_snapshot(path: &Path) -> io::Result<Snapshot> {
    let body = match fs::read(path) {
        Ok(body) => body,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Snapshot::default()),
        Err(error) => return Err(error),
    };
    match serde_json::from_slice::<Snapshot>(&body) {
        Ok(snapshot) if snapshot.version == STATE_VERSION => Ok(snapshot),
        Ok(snapshot) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "state.json version {} is not supported (expected {STATE_VERSION})",
                snapshot.version
            ),
        )),
        Err(error) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("state.json is unreadable: {error}"),
        )),
    }
}

/// Replace a file's contents atomically: write a sibling temp file, fsync it,
/// then rename over the target. A crash leaves either the old file or the new
/// one, never a truncated mixture.
pub(crate) fn atomic_write(path: &Path, body: &[u8]) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("state"),
        Uuid::new_v4()
    ));
    {
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        restrict_new_file(&mut options);
        let mut file = options.open(&temp)?;
        file.write_all(body)?;
        file.sync_all()?;
    }
    fs::rename(&temp, path)?;
    restrict_file(path)
}

#[cfg(unix)]
fn restrict_new_file(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn restrict_new_file(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn restrict_directory(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
}

fn restrict_file_if_present(path: &Path) -> io::Result<()> {
    match restrict_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

/// Milliseconds since the Unix epoch.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Index live session records by target for generation fencing.
pub fn index_by_target(
    sessions: impl Iterator<Item = SessionRecord>,
) -> HashMap<String, SessionRecord> {
    sessions
        .map(|record| (record.session_id.clone(), record))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session_id: &str) -> SessionRecord {
        SessionRecord {
            session_id: session_id.to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            command_id: format!("create-{session_id}"),
            generation_command_id: None,
            provider_instance_ref: "claude-primary".into(),
            runtime: "claude".into(),
            driver: "claude-agent-acp".into(),
            cwd: PathBuf::from("/Users/operator/checkout"),
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            actor: None,
            role: None,
            founder_pubkey: Some("ab".repeat(32)),
            granted_operators: BTreeSet::new(),
            granted_viewers: BTreeSet::new(),
            authority_seq: 0,
            model: None,
            routing: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 1_700_000_000_000,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
        }
    }

    #[test]
    fn round_trips_watermarks_sessions_and_catalog_across_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let channel = Uuid::new_v4();
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.record_watermark(channel, 100).expect("watermark");
            store.record_watermark(channel, 50).expect("watermark");
            let mut session = record("s1");
            session.resume_cursor = Some("host-private-acp-session".into());
            store.insert_session(session).expect("insert");
            store
                .set_catalog(CatalogState {
                    revision: 4,
                    content_digest: Some("abc".into()),
                    advertised_channels: vec![channel],
                })
                .expect("catalog");
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(store.watermark(channel), Some(100));
        assert_eq!(
            store.session("s1").map(|s| s.cwd.clone()),
            Some("/Users/operator/checkout".into())
        );
        assert_eq!(store.catalog().revision, 4);
        assert_eq!(store.catalog().advertised_channels, vec![channel]);
        assert_eq!(
            store
                .session("s1")
                .and_then(|session| session.resume_cursor.as_deref()),
            Some("host-private-acp-session")
        );
    }

    #[cfg(unix)]
    #[test]
    fn private_state_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.insert_session(record("s1")).expect("insert");
        store
            .consume_command("create-s1", now_secs())
            .expect("consume");

        assert_eq!(
            fs::metadata(dir.path())
                .expect("directory metadata")
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for file in [STATE_FILE, COMMANDS_FILE] {
            assert_eq!(
                fs::metadata(dir.path().join(file))
                    .expect("file metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600,
                "{file} must not expose the ACP resume cursor or command ledger"
            );
        }
    }

    /// The whole point of allocate-before-publish: a process that dies between
    /// reserving a sequence and publishing it must resume *past* the reserved
    /// number. A gap is legal; handing 2 out twice is not.
    #[test]
    fn a_crash_after_allocating_burns_the_sequence_rather_than_reusing_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let allocated = {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.insert_session(record("s1")).expect("insert");
            let first = store
                .allocate_seq("s1")
                .expect("allocate")
                .expect("session");
            let second = store
                .allocate_seq("s1")
                .expect("allocate")
                .expect("session");
            assert_eq!((first, second), (1, 2));
            // Simulate the crash here: `second` was persisted but never published.
            second
        };

        let mut store = StateStore::open(dir.path(), 3600).expect("reopen");
        let after_restart = store
            .allocate_seq("s1")
            .expect("allocate")
            .expect("session");
        assert_eq!(
            after_restart,
            allocated + 1,
            "restart must resume past the reserved sequence, leaving a gap"
        );
    }

    #[test]
    fn a_legacy_record_falls_back_to_its_create_and_starts_lease_sequences_at_one() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        let object = value.as_object_mut().expect("object");
        object.remove("generationCommandId");
        object.remove("nextLeaseSequence");
        object.remove("routing");

        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");

        assert_eq!(loaded.generation_command_id(), "create-s1");
        assert_eq!(loaded.next_lease_sequence, 1);
        assert_eq!(loaded.routing, None);
    }

    #[test]
    fn a_crash_after_reserving_a_lease_sequence_burns_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.insert_session(record("s1")).expect("insert");
            assert_eq!(
                store
                    .allocate_lease_sequence("s1")
                    .expect("allocate")
                    .expect("session"),
                1
            );
        }

        let mut store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(
            store
                .allocate_lease_sequence("s1")
                .expect("allocate")
                .expect("session"),
            2,
            "a persisted reservation must never be reused after a crash"
        );
    }

    #[test]
    fn consumed_commands_survive_restart_and_expire_past_retention() {
        let dir = tempfile::tempdir().expect("tempdir");
        let now = now_secs();
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            store.consume_command("fresh", now).expect("consume");
            store
                .consume_command("stale", now.saturating_sub(10_000))
                .expect("consume");
            // Duplicate consumption is a no-op, not a second ledger line.
            store.consume_command("fresh", now).expect("consume");
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert!(store.is_command_consumed("fresh"));
        assert!(
            !store.is_command_consumed("stale"),
            "entries older than the horizon are pruned, since they are ignored anyway"
        );

        let ledger = fs::read_to_string(dir.path().join(COMMANDS_FILE)).expect("read ledger");
        assert_eq!(
            ledger.lines().filter(|line| !line.is_empty()).count(),
            1,
            "pruning rewrites the ledger rather than growing it"
        );
    }

    #[test]
    fn live_session_count_excludes_closed_generations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.insert_session(record("s1")).expect("insert");
        let mut closed = record("s2");
        closed.closed = true;
        store.insert_session(closed).expect("insert");
        assert_eq!(store.live_session_count(), 1);
        assert_eq!(store.sessions().count(), 2);
    }

    /// The umbrella's founder is durable, first-claim-wins, and resolvable
    /// from records written before the claim existed.
    #[test]
    fn an_umbrellas_founder_is_claimed_once_and_survives_a_restart() {
        let dir = tempfile::tempdir().expect("tempdir");
        let founder = "aa".repeat(32);
        let seat = "ef".repeat(32);
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            assert_eq!(store.umbrella_founder("umbrella-1"), None);
            assert_eq!(
                store
                    .claim_umbrella_founder("umbrella-1", &founder)
                    .expect("claim"),
                founder
            );
            assert_eq!(
                store
                    .claim_umbrella_founder("umbrella-1", &seat)
                    .expect("second claim"),
                founder,
                "a later create never takes an umbrella over"
            );
        }
        let mut reopened = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(
            reopened.umbrella_founder("umbrella-1").as_deref(),
            Some(founder.as_str())
        );

        // An umbrella minted before the claim existed resolves from the
        // earliest session record that named it.
        let mut older = record("s1");
        older.session_ref = Some("umbrella-2".into());
        older.founder_pubkey = Some(founder.clone());
        older.created_at_ms = 10;
        let mut newer = record("s2");
        newer.session_ref = Some("umbrella-2".into());
        newer.founder_pubkey = Some(seat.clone());
        newer.created_at_ms = 20;
        reopened.insert_session(newer).expect("insert");
        reopened.insert_session(older).expect("insert");
        assert_eq!(
            reopened.umbrella_founder("umbrella-2").as_deref(),
            Some(founder.as_str()),
            "the earliest execution under an umbrella names its founder"
        );
        assert_eq!(
            reopened
                .claim_umbrella_founder("umbrella-2", &seat)
                .expect("claim"),
            founder,
            "claiming an already-resolvable umbrella records what it resolved to"
        );
    }

    /// D9's counter is durable state, not a process-lifetime tally: a crew
    /// that spent its allowance and then watched the provider restart must not
    /// come back with a full one. Reopening the same directory is the only
    /// honest test of that — a getter on the in-memory snapshot would pass
    /// even if nothing were ever written.
    #[test]
    fn umbrella_turn_spend_survives_reopening_the_state_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            assert_eq!(store.turns_used("umbrella-1"), 0);
            assert_eq!(store.record_turn_spend("umbrella-1").expect("spend"), 1);
            assert_eq!(store.record_turn_spend("umbrella-1").expect("spend"), 2);
            assert_eq!(store.record_turn_spend("umbrella-2").expect("spend"), 1);
        }
        let reopened = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(reopened.turns_used("umbrella-1"), 2);
        assert_eq!(
            reopened.turns_used("umbrella-2"),
            1,
            "each umbrella spends its own allowance"
        );
        assert_eq!(
            reopened.turns_used("umbrella-never-run"),
            0,
            "an umbrella with no recorded turns reads as zero, not as missing"
        );
    }

    #[test]
    fn updating_a_missing_session_reports_rather_than_creating_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        assert!(!store
            .update_session("ghost", |record| record.closed = true)
            .expect("update"));
        assert!(store.allocate_seq("ghost").expect("allocate").is_none());
    }

    #[test]
    fn a_failed_session_update_rolls_back_the_in_memory_record() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.insert_session(record("s1")).expect("insert");

        let blocker = dir.path().join("not-a-directory");
        fs::write(&blocker, b"file").expect("blocker");
        store.dir = blocker;
        let result = store.update_session("s1", |record| record.closed = true);

        assert!(result.is_err());
        assert_eq!(store.session("s1").map(|record| record.closed), Some(false));
    }

    /// Records written before the umbrella and authority fields existed still
    /// load as ungoverned legacy executions; no authority is inferred.
    #[test]
    fn pre_authority_records_load_ungoverned_without_inference() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        let object = value.as_object_mut().expect("object");
        for field in [
            "sessionRef",
            "genesisRef",
            "founderPubkey",
            "grantedOperators",
            "authoritySeq",
        ] {
            object
                .remove(field)
                .expect("field present in current records");
        }
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert!(loaded.session_ref.is_none());
        assert!(loaded.genesis_ref.is_none());
        assert!(loaded.founder_pubkey.is_none());
        assert!(loaded.granted_operators.is_empty());
        assert_eq!(loaded.authority_seq, 0);
    }

    /// Records written before the runtime fields existed must still load, and
    /// they load as claude — the only runtime that could have minted them.
    #[test]
    fn pre_runtime_session_records_default_to_claude() {
        let mut value = serde_json::to_value(record("s1")).expect("serialize");
        let object = value.as_object_mut().expect("object");
        object.remove("providerInstanceRef");
        object.remove("runtime");
        object.remove("driver");
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(loaded.provider_instance_ref, "claude-primary");
        assert_eq!(loaded.runtime, "claude");
        assert_eq!(loaded.driver, "claude-agent-acp");
    }

    /// The bootstrap transport is a diagnostic added after `STATE_VERSION` was
    /// pinned, so a record written without it must still load — as "no
    /// bootstrap recorded", which is exactly what those records mean.
    #[test]
    fn pre_bootstrap_transport_records_load_without_one() {
        let mut written = record("s1");
        written.bootstrap_transport = Some(BootstrapTransport::SystemPrompt);
        let mut value = serde_json::to_value(&written).expect("serialize");
        let object = value.as_object_mut().expect("object");
        assert_eq!(
            object.remove("bootstrapTransport"),
            Some(serde_json::json!("systemPrompt")),
            "the field is persisted under its camelCase name"
        );
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert_eq!(loaded.bootstrap_transport, None);
    }

    #[test]
    fn pre_team_wake_open_turns_default_eligible_so_assignment_crashes_are_not_lost() {
        let mut written = record("s1");
        written.open_turn = Some(OpenTurn {
            turn_id: "turn-1".into(),
            command_id: Some("assignment-command".into()),
            team_wake_eligible: true,
            started_at_ms: 1,
        });
        let mut value = serde_json::to_value(&written).expect("serialize");
        value["openTurn"]
            .as_object_mut()
            .expect("open turn object")
            .remove("teamWakeEligible")
            .expect("current field");
        let loaded: SessionRecord = serde_json::from_value(value).expect("deserialize");
        assert!(
            loaded
                .open_turn
                .expect("legacy open turn")
                .team_wake_eligible
        );
    }

    /// The persisted driver — not the live descriptor table — names the wire
    /// target, so a session's identity cannot mutate mid-life when its
    /// runtime's adapter is uninstalled across a restart.
    #[test]
    fn the_target_is_minted_from_the_persisted_driver() {
        let mut codex = record("s1");
        codex.driver = "codex-acp".into();
        let target = codex.target("instance-1");
        assert_eq!(target.driver, "codex-acp");
        assert_eq!(target.instance_id, "instance-1");
        assert_eq!(target.session_id, "s1");
        assert_eq!(target.generation, 1);
    }

    /// One state directory, one provider: the second lock attempt must fail
    /// while the first is held, and succeed once it is released. (flock
    /// conflicts apply across open file descriptions, so two handles in one
    /// process exercise the same contention two processes would.)
    #[test]
    fn the_state_dir_lock_admits_exactly_one_holder() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = acquire_state_dir_lock(dir.path()).expect("first lock");
        let second = acquire_state_dir_lock(dir.path());
        let error = second.expect_err("a second holder must be refused");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(
            error.to_string().contains(&std::process::id().to_string()),
            "the refusal names the owning pid: {error}"
        );
        drop(first);
        acquire_state_dir_lock(dir.path()).expect("lock is free after release");
    }

    /// The lock file records the owner's pid so a supervisor can identify an
    /// orphaned provider to take over from.
    #[test]
    fn the_lock_file_records_the_owner_pid() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _lock = acquire_state_dir_lock(dir.path()).expect("lock");
        let recorded = fs::read_to_string(dir.path().join(LOCK_FILE)).expect("read lock file");
        assert_eq!(recorded.trim(), std::process::id().to_string());
    }

    #[test]
    fn a_corrupt_snapshot_is_an_error_rather_than_a_silent_reset() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(STATE_FILE), b"{ not json").expect("write");
        let error = StateStore::open(dir.path(), 3600).expect_err("should refuse to start");
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
