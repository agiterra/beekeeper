//! Durable provider state: watermarks, command dedupe, session records,
//! per-generation sequence counters, and the catalog revision.
//!
//! Four files, all under `BEEKEEPER_CSP_STATE_DIR`:
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

use beekeeper_core::coding_session_authority_claim::{fold_current_claim, ClaimLink, ClaimState};
use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
use beekeeper_core::coding_session_command::CodingSessionTarget;
use beekeeper_core::coding_session_routing::RoutingRecord;

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
/// Append-only ledger of native mid-turn steer attempts, one JSON record per
/// line; the last record for an `attemptId` is that attempt's current state.
///
/// Separate from the command and refusal ledgers because it answers a
/// question neither can: "was this input *written* to a runtime, and what
/// became of it?" A command is consumed when its turn starts and refused when
/// it is answered terminally; a steer attempt sits between those — durably
/// intended before the mailbox takes it, so a crash in the window can never
/// replay a write that may already have gone out.
const STEER_ATTEMPTS_FILE: &str = "steer_attempts.jsonl";
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
                "another beekeeper-session-provider instance already owns {} (owner pid: {}): {error}",
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
    /// The role pack this seat was staged with, as the wire describes it.
    ///
    /// Unlike the pack's *directory* — host-local, deliberately not persisted
    /// for the same reason the nsec is not — this is a repository coordinate,
    /// a commit, a role and a path, all of which mean the same thing on every
    /// machine. Held here so every 44223 of this generation republishes the
    /// same answer to "which pack ran", including the ones written long after
    /// the one-shot seat file was consumed.
    ///
    /// `None` for a seat staged from a pack installed on the launching
    /// computer (no repository can vouch for it), for a packless seat, and for
    /// every record written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pack_ref: Option<crate::actor_seats::PackRef>,
    /// How the staged pack was composed — the app version whose templates
    /// resolved it and the digest of the result — read from the pack's
    /// `compose.json` at seat start. `None` for an uncomposed pack, a
    /// packless seat, and every record written before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_ref: Option<beekeeper_core::coding_session_payload::ComposeRef>,
    /// The project, seat and rights scope this generation's native history
    /// was written under (`crate::execution_scope`). A native cursor is only
    /// reattached when the next launch prepares the same scope. `None` for a
    /// record written before binding existed, whose native history is
    /// therefore never silently reattached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_binding: Option<crate::execution_scope::ExecutionBinding>,
    /// What boundary this generation actually ran inside, for metadata and
    /// diagnostics. `None` for a record written before the boundary existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_boundary: Option<crate::execution_scope::RecordedBoundary>,
    /// Why this execution's own authority was withdrawn — its seat revoked,
    /// or this provider removed from its channel. Set when the process is
    /// stopped for it; a resume or restore of the record is refused while it
    /// is set.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority_withdrawn: Option<String>,
    /// The newest head of this execution's project the host has seen served
    /// (`created_at`): what a later project deletion is judged against, kept
    /// here so a deletion made while the provider was down is still
    /// recognised after it restarts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_head_seen_at: Option<u64>,
    /// The verified signer of the create that opened this execution.
    ///
    /// Not the same fact as [`Self::founder_pubkey`]: for a genesis-bearing
    /// create the founder is the genesis's signer, and the person who asked
    /// for *this* execution may be a granted operator or the session's
    /// claimant. Persisted because the create's own `initialTurn` is dispatched
    /// straight to the actor's mailbox and never passes through the turn path
    /// that would otherwise record who sent it — so without this the handover
    /// fence has to guess, and guessing "the founder" refuses a claimant's own
    /// continuation on the body they claimed.
    ///
    /// `None` for records written before the field existed; the fence then
    /// falls back to asking whether this body may act at all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
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
    /// The model label the create asked for, verbatim (`default`,
    /// `opus[1m]`, …), or `None` when it named none. Kept apart from
    /// [`Self::model`], which startup overwrites with what the adapter
    /// applied, so a turn result can name both sides (ledger 268(e)).
    #[serde(default)]
    pub model_requested: Option<String>,
    /// The latest model id the adapter itself reported answering — a turn's
    /// own report, or the adapter's session-level report when it is a model
    /// rather than the `default` picker label. `None` until the adapter says.
    #[serde(default)]
    pub model_effective: Option<String>,
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
    /// What this session's accepted authority chain says about its handover
    /// claim — who holds the whole umbrella, and on which execution body.
    ///
    /// Folded from the same verified links that extend
    /// [`Self::granted_operators`] (`crate::authority`), persisted so the
    /// fence survives a restart, and re-derived from the chain on recovery
    /// before any metadata is published. The claim is **umbrella-wide**
    /// (`docs/HANDOVER_IMPL.md` §1), so every local record rooted at the same
    /// genesis carries the same value.
    ///
    /// [`ClaimState::NoClaim`] — the state of every session nobody has ever
    /// handed over — serializes as **absent**, so a `state.json` written
    /// before this field existed decodes unchanged and one written by this
    /// build gains no key for the sessions that have no claim.
    #[serde(default, skip_serializing_if = "claim_state_is_absent")]
    pub handover: ClaimState,
    /// The accepted whole-session deletion that retired this umbrella, if one
    /// has been verified.
    ///
    /// Terminal and one-way: a retired record publishes no metadata, asks for
    /// no seat custody, restores nothing, and answers
    /// [`crate::payload::SESSION_RETIRED`] to every command
    /// (`docs/HANDOVER_IMPL.md` §3.2). Absent for every record whose umbrella
    /// has not been deleted, which is the shape every pre-field `state.json`
    /// already has.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired: Option<Retirement>,
}

/// The accepted deletion that retired an umbrella, as this provider verified it.
///
/// Kept rather than reduced to a boolean because "deleted" is a claim a
/// reader is entitled to check: the deletion event id is the kind 5 the relay
/// applied, and `receipt_event_id` names the relay-signed 40099 receipt when
/// the retirement came from one rather than from a founder-signed request
/// plus a confirmed absence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Retirement {
    /// Event id (lowercase 64-hex) of the accepted kind 5 deletion.
    pub deletion_event_id: String,
    /// Event id of the relay-signed 40099 deletion receipt, when the
    /// retirement was proven by one. `None` means it was proven the other
    /// way: a founder-signed kind 5 plus an authenticated exact-id read of
    /// the genesis returning zero rows.
    pub receipt_event_id: Option<String>,
    /// When this provider recorded the retirement, milliseconds since the
    /// Unix epoch. The provider's own observation time, never the deletion's.
    pub at: i64,
}

/// Whether a claim state is the one that writes no key at all.
///
/// A free function rather than a method on [`ClaimState`] because the absence
/// rule is this file's storage decision, not the shared type's: `buzz-core`
/// owns what the three states *mean*, and the provider owns which of them its
/// `state.json` omits so older files keep decoding.
fn claim_state_is_absent(state: &ClaimState) -> bool {
    matches!(state, ClaimState::NoClaim)
}

/// The shortest link prefix that folds to exactly `state`.
///
/// The provider learns its chain one accepted link at a time — a live 40099
/// receipt, or one step of a backfill — but
/// [`beekeeper_core::coding_session_authority_claim::fold_current_claim`] is
/// deliberately a whole-chain fold that starts at
/// [`ClaimState::NoClaim`]. Rather than reimplement the claim rule for the
/// incremental case (two copies of a rule this careful is exactly how a
/// regrant quietly resurrects a claim), the provider replays the *persisted*
/// state as the shortest chain that produces it and then folds the new link
/// with the canonical function.
///
/// The prefix is faithful because the fold's whole state is the three
/// variants: `Active(claim)` is what one `takeover` of that claimant and body
/// produces, and `Voided { last, voided_by }` is what that same `takeover`
/// followed by a `revoke` of its claimant produces. `resume_claim_links`
/// therefore round-trips: folding the prefix alone returns `state`, which
/// `folding_a_prefix_alone_returns_the_state_it_came_from` pins.
pub fn resume_claim_links(state: &ClaimState) -> Vec<ClaimLink> {
    match state {
        ClaimState::NoClaim => Vec::new(),
        ClaimState::Active(claim) => vec![ClaimLink {
            seq: claim.seq,
            accepted_event_id: claim.accepted_event_id.clone(),
            transition_type: CodingSessionAuthorityTransitionType::Takeover,
            grantee_pubkey: claim.claimant.clone(),
            body_pubkey: Some(claim.body_pubkey.clone()),
        }],
        ClaimState::Voided {
            last,
            voided_by,
            seq,
        } => vec![
            ClaimLink {
                seq: last.seq,
                accepted_event_id: last.accepted_event_id.clone(),
                transition_type: CodingSessionAuthorityTransitionType::Takeover,
                grantee_pubkey: last.claimant.clone(),
                body_pubkey: Some(last.body_pubkey.clone()),
            },
            ClaimLink {
                seq: *seq,
                accepted_event_id: voided_by.clone(),
                transition_type: CodingSessionAuthorityTransitionType::Revoke,
                grantee_pubkey: last.claimant.clone(),
                body_pubkey: None,
            },
        ],
    }
}

/// Apply one newly accepted chain link to a persisted claim state.
///
/// The single place the provider advances a claim. Both consumption paths —
/// the live 40099 receipt and the backfill — call this, and both get the
/// canonical rule (including "a regrant never resurrects a claim") because
/// the decision itself is made by
/// [`fold_current_claim`] over [`resume_claim_links`] plus the new link, not
/// by a second copy of the rule living here.
pub fn extend_claim(state: &ClaimState, link: ClaimLink) -> ClaimState {
    fold_current_claim(
        resume_claim_links(state)
            .into_iter()
            .chain(std::iter::once(link)),
    )
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

    /// Whether an accepted whole-session deletion has retired this umbrella.
    pub fn is_retired(&self) -> bool {
        self.retired.is_some()
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
    /// Who drove this turn, when the provider knows.
    ///
    /// Persisted so the handover fence can tell *whose* work is running when a
    /// claim lands mid-turn. A `transfer` that moves the claim to a new
    /// claimant on the **same** body has to stop the old claimant's turn and
    /// leave the new one's alone, and without this the two are
    /// indistinguishable — the record knows a turn is open and nothing about
    /// who opened it.
    ///
    /// `None` for a lifecycle-opened turn (a create's `initialTurn`) and for
    /// every record written before the field existed; the fence then falls
    /// back to asking whether this **body** may act at all, which is the
    /// question it asked before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operator_pubkey: Option<String>,
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
    /// Terminal decisions awaiting outbox and legacy-ledger projection.
    #[serde(default)]
    terminal_dispositions: BTreeMap<String, TerminalDisposition>,
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
            terminal_dispositions: BTreeMap::new(),
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

/// Where one native steer attempt stands.
///
/// Serialized in `snake_case`, matching the disposition names in
/// `docs/NATIVE_STEERING_IMPL.md` §2. [`Self::Intent`] is the only open state;
/// everything else is terminal for the attempt (a `reconciled_*` disposition
/// is a terminal answer to an attempt that was already terminal as
/// [`Self::Unknown`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SteerDisposition {
    /// Durably intended; the runtime may or may not have received it.
    Intent,
    /// The runtime acknowledged the input joined the running turn.
    Injected,
    /// The runtime acknowledged it started a new, unobserved turn.
    StartedNewTurn,
    /// Nothing reached the runtime; the command fell back to a boundary turn.
    NotDelivered,
    /// Written (or possibly written) with no answer; never replayed.
    Unknown,
    /// Refused before any runtime write (authority, generation, fence).
    Prevented,
    /// A late acknowledgement settled an [`Self::Unknown`] attempt as injected.
    ReconciledInjected,
    /// A late acknowledgement settled an [`Self::Unknown`] attempt as not
    /// delivered.
    ReconciledNotDelivered,
    /// A late acknowledgement settled an [`Self::Unknown`] attempt as a new
    /// unobserved turn.
    ReconciledNewTurn,
}

impl SteerDisposition {
    /// The wire/ledger name of this disposition.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Injected => "injected",
            Self::StartedNewTurn => "started_new_turn",
            Self::NotDelivered => "not_delivered",
            Self::Unknown => "unknown",
            Self::Prevented => "prevented",
            Self::ReconciledInjected => "reconciled_injected",
            Self::ReconciledNotDelivered => "reconciled_not_delivered",
            Self::ReconciledNewTurn => "reconciled_new_turn",
        }
    }

    /// Whether the attempt is still awaiting its resolution.
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Intent)
    }
}

/// One line of [`STEER_ATTEMPTS_FILE`]: everything needed to answer, fall
/// back, or refuse one native steer attempt without the process that started
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SteerAttemptRecord {
    /// `{commandId}#{n}`, `n` = 1 + prior attempts for the command.
    pub attempt_id: String,
    /// The 44220 the attempt answers.
    pub command_id: String,
    /// The execution the input was addressed to.
    pub session_id: String,
    /// The exact generation the command addressed.
    pub generation: u64,
    /// Channel the command arrived on, so receipts go back to the right room.
    pub channel_id: Uuid,
    /// The full target, so a receipt keyed to this attempt names it exactly.
    pub target: CodingSessionTarget,
    /// The command event's `created_at`, which pins the channel watermark
    /// while the attempt is open.
    pub created_at: u64,
    /// The team-wake operation the command takes custody of, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_key: Option<String>,
    /// The verified signer of the command.
    pub operator_pubkey: String,
    /// The seat role the signer holds, when the turn was framed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_role: Option<String>,
    /// The signed original text, kept so a not-delivered attempt can be
    /// rebuilt as a boundary turn from disk.
    pub text: String,
    /// Where the attempt stands.
    pub disposition: SteerDisposition,
    /// Unix seconds this record was written.
    pub at: u64,
    /// The turn the input joined (or was written into), once known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

/// One atomic terminal decision and the exact signed answer it promises.
/// Retained until both the outbox and the refusal ledger contain the decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TerminalDisposition {
    pub semantic_key: String,
    pub event: nostr::Event,
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
    /// Native steer attempts by `attemptId`, last record wins.
    steer_attempts: BTreeMap<String, SteerAttemptRecord>,
    /// Injected one-shot ledger-append failures. Test-only.
    #[cfg(test)]
    fault_plan: FaultPlan,
}

/// One-shot injected failures for the durable ledger appends.
///
/// Test-only, and compiled out of every release build. The provider's
/// ordering guarantees are claims about what survives a *failing* write, so
/// they can only be proven by a test that can make one write fail on demand
/// without breaking the directory for every write after it. Each flag is
/// consumed by the first append it applies to.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct FaultPlan {
    /// Fail the next [`StateStore::record_refusal`].
    pub fail_next_refusal_append: bool,
    /// Fail the next [`StateStore::consume_command`].
    pub fail_next_command_append: bool,
    /// Fail the next [`StateStore::consume_operation`].
    pub fail_next_operation_append: bool,
    /// Fail the next steer-attempt append ([`StateStore::stage_steer_intent`]
    /// or [`StateStore::resolve_steer_attempt`]).
    pub fail_next_steer_append: bool,
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
        for file in [
            STATE_FILE,
            COMMANDS_FILE,
            REFUSALS_FILE,
            OPERATIONS_FILE,
            STEER_ATTEMPTS_FILE,
        ] {
            restrict_file_if_present(&dir.join(file))?;
        }
        let snapshot = load_snapshot(&dir.join(STATE_FILE))?;
        let mut store = Self {
            dir: dir.to_path_buf(),
            snapshot,
            commands: HashSet::new(),
            refusals: HashSet::new(),
            operations: HashMap::new(),
            steer_attempts: BTreeMap::new(),
            #[cfg(test)]
            fault_plan: FaultPlan::default(),
        };
        store.load_commands(command_retention_secs)?;
        Ok(store)
    }

    /// The injected ledger faults this store will honor next. Test-only.
    #[cfg(test)]
    pub(crate) fn fault_plan(&mut self) -> &mut FaultPlan {
        &mut self.fault_plan
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
        #[cfg(test)]
        if std::mem::take(&mut self.fault_plan.fail_next_command_append) {
            return Err(io::Error::other("injected command-ledger append failure"));
        }
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
        #[cfg(test)]
        if std::mem::take(&mut self.fault_plan.fail_next_operation_append) {
            return Err(io::Error::other("injected operation-ledger append failure"));
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
            || self.snapshot.terminal_dispositions.contains_key(command_id)
    }

    /// Whether an answer for this command is staged and not yet projected.
    ///
    /// The one question `AlreadyRefused` has to ask: a command refused on an
    /// earlier pass whose signed answer never reached the outbox still owes
    /// that answer, and the bytes are right here (Astra's third look, R4).
    pub(crate) fn has_staged_disposition(&self, command_id: &str) -> bool {
        self.snapshot.terminal_dispositions.contains_key(command_id)
    }

    /// Fence a terminal decision and its exact answer in one atomic write.
    pub(crate) fn stage_terminal_disposition(
        &mut self,
        command_id: &str,
        disposition: TerminalDisposition,
    ) -> io::Result<()> {
        if self.snapshot.terminal_dispositions.contains_key(command_id)
            || self.refusals.contains(command_id)
        {
            return Ok(());
        }
        let previous = self.snapshot.clone();
        self.snapshot
            .terminal_dispositions
            .insert(command_id.to_owned(), disposition);
        self.persist_or_restore(previous)
    }

    /// Pending terminal answers; callers must project these before retiring them.
    pub(crate) fn terminal_dispositions(&self) -> Vec<(String, TerminalDisposition)> {
        self.snapshot
            .terminal_dispositions
            .iter()
            .map(|(id, disposition)| (id.clone(), disposition.clone()))
            .collect()
    }

    /// Retire an intent only after its answer reached the durable outbox.
    pub(crate) fn finish_terminal_disposition(&mut self, command_id: &str) -> io::Result<()> {
        self.record_refusal(command_id, now_secs())?;
        let previous = self.snapshot.clone();
        self.snapshot.terminal_dispositions.remove(command_id);
        self.persist_or_restore(previous)
    }

    /// Record that a command has a terminal answer and must not be re-admitted.
    ///
    /// CI and mailbox terminal paths first persist the exact signed answer as
    /// a snapshot intent, then project the outbox and this legacy ledger. The
    /// intent remains an admission fence if either projection fails. A consumed
    /// command may also be terminal after recovery: consumption proves a claim,
    /// not whether the adapter actually received its prompt.
    pub fn record_refusal(&mut self, command_id: &str, at: u64) -> io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fault_plan.fail_next_refusal_append) {
            return Err(io::Error::other("injected refusal-ledger append failure"));
        }
        Self::append_ledger_record(
            &self.dir.join(REFUSALS_FILE),
            &mut self.refusals,
            command_id,
            at,
        )
    }

    /// The attempt id the next native steer attempt for `command_id` gets:
    /// `{command_id}#{n}`, `n` = 1 + attempts already recorded for it.
    ///
    /// Deterministic on purpose — no clock, no randomness — so a retry after a
    /// failed write mints the same id it would have minted the first time.
    pub fn next_steer_attempt_id(&self, command_id: &str) -> String {
        let prior = self
            .steer_attempts
            .values()
            .filter(|attempt| attempt.command_id == command_id)
            .count();
        format!("{command_id}#{}", prior + 1)
    }

    /// Durably record that a native steer is about to be handed to a runtime.
    ///
    /// Written **before** the mailbox takes the input, and that order is the
    /// whole point: a crash anywhere after this line finds an open intent at
    /// restart and answers it as unknown rather than replaying a write that
    /// may already have reached the model. `record.disposition` must be
    /// [`SteerDisposition::Intent`] and the attempt id must be new.
    pub fn stage_steer_intent(&mut self, record: SteerAttemptRecord) -> io::Result<()> {
        if !record.disposition.is_open() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a steer attempt is staged as an intent, never as a resolution",
            ));
        }
        if self.steer_attempts.contains_key(&record.attempt_id) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("steer attempt {} already exists", record.attempt_id),
            ));
        }
        self.append_steer_record(record)
    }

    /// Durably move an attempt to a terminal disposition.
    ///
    /// `turn_id`, when given, replaces the recorded one; `None` keeps whatever
    /// the attempt already carried. Refuses to move an attempt back to
    /// [`SteerDisposition::Intent`] or to resolve an attempt it has never seen.
    pub fn resolve_steer_attempt(
        &mut self,
        attempt_id: &str,
        disposition: SteerDisposition,
        turn_id: Option<&str>,
    ) -> io::Result<()> {
        if disposition.is_open() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "an attempt is resolved to a terminal disposition, never back to intent",
            ));
        }
        let Some(current) = self.steer_attempts.get(attempt_id) else {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("steer attempt {attempt_id} is not recorded"),
            ));
        };
        let mut next = current.clone();
        next.disposition = disposition;
        next.at = now_secs();
        if let Some(turn_id) = turn_id {
            next.turn_id = Some(turn_id.to_owned());
        }
        self.append_steer_record(next)
    }

    /// One attempt by id, as last recorded.
    pub fn steer_attempt(&self, attempt_id: &str) -> Option<&SteerAttemptRecord> {
        self.steer_attempts.get(attempt_id)
    }

    /// Every attempt still at [`SteerDisposition::Intent`], oldest first.
    pub fn open_steer_attempts(&self) -> Vec<SteerAttemptRecord> {
        self.steer_attempts_where(|attempt| attempt.disposition.is_open())
    }

    /// Every attempt resolved [`SteerDisposition::Unknown`] and not yet
    /// reconciled, oldest first.
    pub fn unknown_steer_attempts(&self) -> Vec<SteerAttemptRecord> {
        self.steer_attempts_where(|attempt| attempt.disposition == SteerDisposition::Unknown)
    }

    /// The newest attempt recorded for `command_id`, if any.
    pub fn steer_attempt_for_command(&self, command_id: &str) -> Option<&SteerAttemptRecord> {
        self.steer_attempts
            .values()
            .filter(|attempt| attempt.command_id == command_id)
            .max_by_key(|attempt| steer_attempt_ordinal(&attempt.attempt_id))
    }

    /// Every attempt recorded for `command_id`, in attempt order.
    pub fn steer_attempts_for_command(&self, command_id: &str) -> Vec<&SteerAttemptRecord> {
        let mut attempts: Vec<&SteerAttemptRecord> = self
            .steer_attempts
            .values()
            .filter(|attempt| attempt.command_id == command_id)
            .collect();
        attempts.sort_by_key(|attempt| steer_attempt_ordinal(&attempt.attempt_id));
        attempts
    }

    fn steer_attempts_where(
        &self,
        keep: impl Fn(&SteerAttemptRecord) -> bool,
    ) -> Vec<SteerAttemptRecord> {
        let mut attempts: Vec<SteerAttemptRecord> = self
            .steer_attempts
            .values()
            .filter(|attempt| keep(attempt))
            .cloned()
            .collect();
        attempts.sort_by(|left, right| {
            left.at
                .cmp(&right.at)
                .then_with(|| left.attempt_id.cmp(&right.attempt_id))
        });
        attempts
    }

    /// Append one attempt record; the in-memory map only keeps what the disk
    /// took, so a failed write leaves the previous state in force.
    fn append_steer_record(&mut self, record: SteerAttemptRecord) -> io::Result<()> {
        #[cfg(test)]
        if std::mem::take(&mut self.fault_plan.fail_next_steer_append) {
            return Err(io::Error::other("injected steer-attempt append failure"));
        }
        let attempt_id = record.attempt_id.clone();
        let previous = self
            .steer_attempts
            .insert(attempt_id.clone(), record.clone());
        let result = append_json_line(&self.dir.join(STEER_ATTEMPTS_FILE), &record);
        if result.is_err() {
            match previous {
                Some(previous) => {
                    self.steer_attempts.insert(attempt_id, previous);
                }
                None => {
                    self.steer_attempts.remove(&attempt_id);
                }
            }
        }
        result
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
        if sequence > beekeeper_core::coding_session_command::MAX_SAFE_GENERATION {
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
        )?;
        Self::load_steer_attempts(
            &dir.join(STEER_ATTEMPTS_FILE),
            &mut self.steer_attempts,
            retention_secs,
        )
    }

    /// Load [`STEER_ATTEMPTS_FILE`]: last record per attempt wins.
    ///
    /// Resolved attempts older than the freshness horizon are dropped — their
    /// command is ignored on its own merits by then — and the file is
    /// compacted to one line per surviving attempt when anything was dropped.
    /// An open intent is **never** dropped by age: it is owed a restart answer
    /// regardless of how long ago it was staged.
    fn load_steer_attempts(
        path: &Path,
        seen: &mut BTreeMap<String, SteerAttemptRecord>,
        retention_secs: u64,
    ) -> io::Result<()> {
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        let cutoff = now_secs().saturating_sub(retention_secs);
        let mut lines = 0usize;
        let mut unreadable = false;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            lines += 1;
            match serde_json::from_str::<SteerAttemptRecord>(&line) {
                Ok(record) => {
                    seen.insert(record.attempt_id.clone(), record);
                }
                Err(error) => {
                    tracing::warn!(target: "csp::state", "dropping unreadable steer attempt line: {error}");
                    unreadable = true;
                }
            }
        }
        let before = seen.len();
        seen.retain(|_, attempt| attempt.disposition.is_open() || attempt.at >= cutoff);
        if unreadable || seen.len() != before || seen.len() != lines {
            let mut body = String::new();
            for record in seen.values() {
                body.push_str(&serde_json::to_string(record)?);
                body.push('\n');
            }
            atomic_write(path, body.as_bytes())?;
        }
        Ok(())
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

/// The `n` in an attempt id `{commandId}#{n}`; zero for an id without one.
fn steer_attempt_ordinal(attempt_id: &str) -> u64 {
    attempt_id
        .rsplit_once('#')
        .and_then(|(_, ordinal)| ordinal.parse().ok())
        .unwrap_or(0)
}

/// Append one JSON record as a single line, fsynced, to an owner-only file.
///
/// The whole line goes down in one `write` on an O_APPEND handle, as every
/// other ledger here does, so a line can never be torn in half.
fn append_json_line<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    restrict_new_file(&mut options);
    let mut file = options.open(path)?;
    restrict_file(path)?;
    let mut line = serde_json::to_string(record)?;
    line.push('\n');
    file.write_all(line.as_bytes())?;
    file.sync_all()
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
            execution_binding: None,
            execution_boundary: None,
            authority_withdrawn: None,
            project_head_seen_at: None,
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
            model_requested: None,
            model_effective: None,
            routing: None,
            resume_cursor: None,
            title: None,
            created_at_ms: 1_700_000_000_000,
            next_seq: 1,
            next_lease_sequence: 1,
            bootstrap_transport: None,
            open_turn: None,
            closed: false,
            created_by: None,
            handover: ClaimState::NoClaim,
            retired: None,
            pack_ref: None,
            compose_ref: None,
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

    fn steer_attempt(command_id: &str, attempt_id: &str) -> SteerAttemptRecord {
        SteerAttemptRecord {
            attempt_id: attempt_id.to_owned(),
            command_id: command_id.to_owned(),
            session_id: "s1".to_owned(),
            generation: 1,
            channel_id: Uuid::nil(),
            target: CodingSessionTarget {
                instance_id: "instance-1".to_owned(),
                driver: "claude-agent-acp".to_owned(),
                session_id: "s1".to_owned(),
                generation: 1,
            },
            created_at: 1_000,
            operation_key: None,
            operator_pubkey: "ab".repeat(32),
            sender_role: None,
            text: "steer me".to_owned(),
            disposition: SteerDisposition::Intent,
            at: now_secs(),
            turn_id: None,
        }
    }

    /// Attempt ids are `{commandId}#{n}` with `n` counting prior attempts —
    /// no clock, no randomness — and the ledger survives a restart with the
    /// last record per attempt in force.
    #[test]
    fn steer_attempts_round_trip_with_last_record_winning() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            assert_eq!(store.next_steer_attempt_id("cmd"), "cmd#1");
            store
                .stage_steer_intent(steer_attempt("cmd", "cmd#1"))
                .expect("stage");
            assert_eq!(store.next_steer_attempt_id("cmd"), "cmd#2");
            assert_eq!(store.next_steer_attempt_id("other"), "other#1");
            assert!(
                store
                    .stage_steer_intent(steer_attempt("cmd", "cmd#1"))
                    .is_err(),
                "an attempt id is staged once"
            );
            assert!(
                store
                    .stage_steer_intent(SteerAttemptRecord {
                        disposition: SteerDisposition::Injected,
                        ..steer_attempt("cmd", "cmd#9")
                    })
                    .is_err(),
                "only an intent can be staged"
            );
            assert_eq!(store.open_steer_attempts().len(), 1);
            store
                .resolve_steer_attempt("cmd#1", SteerDisposition::NotDelivered, None)
                .expect("resolve");
            store
                .stage_steer_intent(steer_attempt("cmd", "cmd#2"))
                .expect("stage second");
            store
                .resolve_steer_attempt("cmd#2", SteerDisposition::Unknown, Some("turn-9"))
                .expect("resolve second");
            assert!(
                store
                    .resolve_steer_attempt("cmd#2", SteerDisposition::Intent, None)
                    .is_err(),
                "never back to intent"
            );
            assert!(
                store
                    .resolve_steer_attempt("nope#1", SteerDisposition::Prevented, None)
                    .is_err(),
                "never an attempt that was not staged"
            );
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert_eq!(store.next_steer_attempt_id("cmd"), "cmd#3");
        assert_eq!(
            store.steer_attempt("cmd#1").map(|a| a.disposition),
            Some(SteerDisposition::NotDelivered)
        );
        let second = store.steer_attempt("cmd#2").expect("second");
        assert_eq!(second.disposition, SteerDisposition::Unknown);
        assert_eq!(second.turn_id.as_deref(), Some("turn-9"));
        assert!(store.open_steer_attempts().is_empty());
        assert_eq!(store.unknown_steer_attempts().len(), 1);
        assert_eq!(
            store
                .steer_attempt_for_command("cmd")
                .map(|a| a.attempt_id.as_str()),
            Some("cmd#2"),
            "the newest attempt answers for the command"
        );
        assert_eq!(
            store
                .steer_attempts_for_command("cmd")
                .iter()
                .map(|a| a.attempt_id.as_str())
                .collect::<Vec<_>>(),
            vec!["cmd#1", "cmd#2"]
        );
    }

    /// A resolved attempt past the freshness horizon is pruned; an open
    /// intent never is, however old — it is owed a restart answer.
    #[test]
    fn steer_attempt_retention_keeps_every_open_intent() {
        let dir = tempfile::tempdir().expect("tempdir");
        {
            let mut store = StateStore::open(dir.path(), 3600).expect("open");
            let stale = now_secs().saturating_sub(10_000);
            store
                .stage_steer_intent(SteerAttemptRecord {
                    at: stale,
                    ..steer_attempt("old-open", "old-open#1")
                })
                .expect("stage");
            store
                .stage_steer_intent(SteerAttemptRecord {
                    at: stale,
                    ..steer_attempt("old-done", "old-done#1")
                })
                .expect("stage");
            // `resolve_steer_attempt` stamps `now`, so the stale resolved
            // record is written by hand.
            let mut resolved = steer_attempt("old-done", "old-done#1");
            resolved.disposition = SteerDisposition::Injected;
            resolved.at = stale;
            append_json_line(&dir.path().join(STEER_ATTEMPTS_FILE), &resolved).expect("append");
        }
        let store = StateStore::open(dir.path(), 3600).expect("reopen");
        assert!(store.steer_attempt("old-open#1").is_some());
        assert!(store.steer_attempt("old-done#1").is_none());
        assert_eq!(store.open_steer_attempts().len(), 1);
    }

    /// A failed append leaves the in-memory ledger exactly as it was, so a
    /// retry mints the same attempt id and nothing claims a write that did
    /// not happen.
    #[test]
    fn a_failed_steer_append_is_rolled_back() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store.fault_plan().fail_next_steer_append = true;
        assert!(store
            .stage_steer_intent(steer_attempt("cmd", "cmd#1"))
            .is_err());
        assert!(store.steer_attempt("cmd#1").is_none());
        assert_eq!(store.next_steer_attempt_id("cmd"), "cmd#1");
        store
            .stage_steer_intent(steer_attempt("cmd", "cmd#1"))
            .expect("retry");
        store.fault_plan().fail_next_steer_append = true;
        assert!(store
            .resolve_steer_attempt("cmd#1", SteerDisposition::Injected, Some("t"))
            .is_err());
        assert_eq!(
            store.steer_attempt("cmd#1").map(|a| a.disposition),
            Some(SteerDisposition::Intent),
            "the previous record stays in force"
        );
        assert!(
            !dir.path().join(STEER_ATTEMPTS_FILE).exists() || {
                let body = fs::read_to_string(dir.path().join(STEER_ATTEMPTS_FILE)).expect("read");
                body.lines().count() == 1
            }
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_steer_attempt_ledger_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = StateStore::open(dir.path(), 3600).expect("open");
        store
            .stage_steer_intent(steer_attempt("cmd", "cmd#1"))
            .expect("stage");
        assert_eq!(
            fs::metadata(dir.path().join(STEER_ATTEMPTS_FILE))
                .expect("stat")
                .permissions()
                .mode()
                & 0o777,
            0o600
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
            operator_pubkey: None,
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

    /// The incremental fold must agree with the whole-chain one, or the
    /// provider's persisted fence and the canonical rule can disagree.
    ///
    /// Every prefix of a chain is replayed through
    /// [`resume_claim_links`] + [`extend_claim`], one link at a time, and
    /// compared with [`fold_current_claim`] over the whole chain. The chain
    /// deliberately includes the regrant, which is the case a hand-written
    /// incremental rule gets wrong.
    #[test]
    fn folding_link_by_link_agrees_with_folding_the_whole_chain() {
        use beekeeper_core::coding_session_authority_claim::fold_current_claim;

        let claimant = "bb".repeat(32);
        let body = "dd".repeat(32);
        let link = |seq: u32,
                    transition_type: CodingSessionAuthorityTransitionType,
                    grantee: &str,
                    body: Option<&str>| ClaimLink {
            seq,
            accepted_event_id: format!("{seq:02}").repeat(32),
            transition_type,
            grantee_pubkey: grantee.to_owned(),
            body_pubkey: body.map(str::to_owned),
        };
        let chain = [
            link(
                1,
                CodingSessionAuthorityTransitionType::GrantOperator,
                &claimant,
                None,
            ),
            link(
                2,
                CodingSessionAuthorityTransitionType::Takeover,
                &claimant,
                Some(&body),
            ),
            link(
                3,
                CodingSessionAuthorityTransitionType::Revoke,
                &claimant,
                None,
            ),
            // The regrant: standing restored, claim still voided.
            link(
                4,
                CodingSessionAuthorityTransitionType::GrantOperator,
                &claimant,
                None,
            ),
            link(
                5,
                CodingSessionAuthorityTransitionType::Takeover,
                &claimant,
                Some(&"ff".repeat(32)),
            ),
        ];

        let mut incremental = ClaimState::NoClaim;
        for length in 1..=chain.len() {
            incremental = extend_claim(&incremental, chain[length - 1].clone());
            let batch = fold_current_claim(chain[..length].iter().cloned());
            assert_eq!(
                incremental, batch,
                "link-by-link and whole-chain folds disagree after {length} links"
            );
            // And the replayed prefix alone reproduces the state it came from.
            assert_eq!(
                fold_current_claim(resume_claim_links(&incremental).into_iter()),
                incremental
            );
        }
        assert!(matches!(incremental, ClaimState::Active(_)));
    }

    /// `NoClaim` writes no key, so a `state.json` from before the fence
    /// existed decodes unchanged and one written now gains nothing for the
    /// sessions nobody handed over.
    #[test]
    fn an_unclaimed_record_serializes_without_a_handover_or_retired_key() {
        let record = record("session-plain");
        let encoded = serde_json::to_value(&record).expect("encode");
        assert!(encoded.get("handover").is_none(), "{encoded}");
        assert!(encoded.get("retired").is_none(), "{encoded}");

        let mut legacy = encoded.clone();
        legacy.as_object_mut().expect("object").remove("handover");
        let decoded: SessionRecord = serde_json::from_value(legacy).expect("legacy decodes");
        assert_eq!(decoded.handover, ClaimState::NoClaim);
        assert_eq!(decoded.retired, None);
    }
}
