//! Crew addressing for `bee sessions` — who is seated where, what is owed to
//! whom, and which generation a name resolves to.
//!
//! Everything in this module is a pure function over decoded events so the
//! rules that decide *who a message reaches* can be tested without a relay.
//! Three of those rules are load-bearing and are stated once, here:
//!
//! 1. **A name is resolved, never guessed.** `--to` is tried as an exact
//!    `cs-target` key, then a provider session id, then a role slug. When more
//!    than one execution answers to the name, the caller gets an error listing
//!    every candidate — a crew whose messages silently reach the wrong seat is
//!    worse than a crew that cannot send.
//! 2. **A role never resolves across umbrellas.** A role slug is only unique
//!    inside one umbrella session, so role lookup requires an umbrella: either
//!    `--session-ref`, or the one the caller's own seat sits in. Without a
//!    scope the lookup is refused, not widened.
//! 3. **Liveness is read from a signed lease, or admitted as unknown.** Kind
//!    24223 is ephemeral, and the relay serves it from a Redis snapshot rather
//!    than from stored events, so it is a statement about *now* with no
//!    history. When no lease answers for a generation, this module says how
//!    long ago the execution last signed something and calls that `quiet` —
//!    never `live`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;

use buzz_core::coding_session_command::{
    coding_session_target_key, CodingSessionAction, CodingSessionCommandPayload,
    CodingSessionDelivery, CodingSessionTarget,
};
use buzz_core::coding_session_genesis::decode_coding_session_genesis;
use buzz_core::coding_session_identity::{ProviderInstanceAlias, RuntimeWord};
use buzz_core::coding_session_lease::{decode_coding_session_lease, CodingSessionLeaseState};
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, MAX_LIFECYCLE_HIRE_BRIEF_BYTES,
};
use buzz_core::coding_session_payload::{
    LifecycleReceipt, ReceiptStatus, SessionStatus, NO_LIVE_EXECUTION, STALE_GENERATION,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
};

use super::{
    content_of, event_created_at, event_str, newest_metadata, resolve_sessions, DecodeStats,
    MetadataRecord, ReceiptRecord, TranscriptRecord,
};
use crate::error::CliError;

// ── Executions ───────────────────────────────────────────────────────────────

/// Whether a generation currently has an actor behind it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Liveness {
    /// A `live` kind-24223 lease answers for this exact generation.
    Live,
    /// No lease, and the execution last signed something `age_secs` ago.
    Quiet {
        /// Seconds since the newest 44225 this generation signed.
        age_secs: i64,
    },
    /// The provider released the lease, or the execution is durably stopped.
    Released,
    /// No lease, and this generation has never signed a transcript item.
    Unknown,
}

impl Liveness {
    /// The one-word form `bee sessions status` prints.
    pub fn word(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Quiet { .. } => "quiet",
            Self::Released => "released",
            Self::Unknown => "unknown",
        }
    }

    /// The rendered form, e.g. `live`, `quiet 3m`, `released`, `unknown`.
    pub fn render(self) -> String {
        match self {
            Self::Quiet { age_secs } => format!("quiet {}", format_age(age_secs)),
            other => other.word().to_owned(),
        }
    }
}

/// Render a duration the way an operator scans it: one unit, no decimals.
pub fn format_age(seconds: i64) -> String {
    let seconds = seconds.max(0);
    match seconds {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86_400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86_400),
    }
}

/// One provider execution as the crew surface sees it: an addressable seat.
#[derive(Debug, Clone, PartialEq)]
pub struct CrewExecution {
    /// Structured `cs-target` key — the unambiguous name for this generation.
    pub target_key: String,
    /// The generation itself.
    pub target: CodingSessionTarget,
    /// Provider pubkey that signs this execution's metadata and transcript.
    pub signer: String,
    /// Agent seat pubkey (`agentRef`), or `None` for a human-driven execution.
    pub actor: Option<String>,
    /// Role slug held within the umbrella; only ever set beside `actor`.
    pub role: Option<String>,
    /// Umbrella session reference, or `None` when the create claimed none.
    pub session_ref: Option<String>,
    /// Newest metadata status, or one inferred from the transcript.
    pub status: String,
    /// Effective model, when metadata named one.
    pub model: Option<String>,
    /// Runtime **word** behind the driver (`claude`, `codex`), when metadata
    /// named one.
    ///
    /// Typed since batch 2 lane B2 so it can never be compared with
    /// `target.driver`, which is a driver slug (`claude-agent-acp`).
    pub runtime: Option<RuntimeWord>,
    /// `event_seq` of the newest transcript item this generation signed.
    pub last_signed_seq: Option<u64>,
    /// `created_at` of that item, in Unix seconds.
    pub last_signed_at: Option<i64>,
    /// Derived liveness — see [`Liveness`].
    pub liveness: Liveness,
    /// The umbrella's turn budget as this execution's newest metadata last
    /// echoed it (plan D9 / contract B), or `None` when no metadata ever
    /// carried the key — either this build predates the budget, or the
    /// umbrella has none configured.
    pub turn_budget: Option<TurnBudget>,
}

impl CrewExecution {
    /// The identity a generation belongs to: everything but the generation.
    ///
    /// Two rows sharing this triple are the same execution at different
    /// points in time, which is exactly what "re-address to the current
    /// generation" needs to know.
    pub fn identity(&self) -> (String, String, String) {
        (
            self.target.driver.clone(),
            self.target.instance_id.clone(),
            self.target.session_id.clone(),
        )
    }

    /// How a human reads this seat: `actor·role` when seated, else
    /// `runtime·model`, else the driver.
    pub fn seat_label(&self) -> String {
        match (self.actor.as_deref(), self.role.as_deref()) {
            (Some(actor), Some(role)) => format!("{}·{role}", short_pubkey(actor)),
            (Some(actor), None) => short_pubkey(actor),
            _ => match (
                self.runtime.as_ref().map(RuntimeWord::as_str),
                self.model.as_deref(),
            ) {
                (Some(runtime), Some(model)) => format!("{runtime}·{model}"),
                (Some(runtime), None) => runtime.to_owned(),
                (None, Some(model)) => model.to_owned(),
                (None, None) => self.target.driver.clone(),
            },
        }
    }
}

/// First eight hex characters of a pubkey, the form every Buzz surface uses.
pub fn short_pubkey(pubkey: &str) -> String {
    pubkey.chars().take(8).collect()
}

/// Statuses that mean this generation will never run another turn.
fn is_terminal_status(status: &str) -> bool {
    matches!(status, "stopped" | "completed" | "failed")
}

/// An umbrella's turn budget as of one execution's newest metadata (plan D9 /
/// contract B): how many agent-originated turns have been counted against it,
/// and the ceiling that refuses the next one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnBudget {
    /// Turns counted against the umbrella so far.
    pub used: u64,
    /// The ceiling; the next agent-originated turn past this is refused
    /// `turn_refused` / `BUDGET_EXHAUSTED`.
    pub limit: u64,
}

impl TurnBudget {
    /// Whether the umbrella has no budget left for another agent turn.
    pub fn exhausted(self) -> bool {
        self.used >= self.limit
    }
}

/// Adopt the wire shape `buzz-core` decoded, so this command reports exactly
/// the numbers the provider signed.
impl From<buzz_core::coding_session_payload::TurnBudget> for TurnBudget {
    fn from(budget: buzz_core::coding_session_payload::TurnBudget) -> Self {
        Self {
            used: budget.used,
            limit: budget.limit,
        }
    }
}

/// Decode every kind-24223 lease snapshot record into `(signer, targetKey)`.
///
/// The relay serves these from Redis, not from stored events, so there is at
/// most one record per generation and it describes the present moment.
pub fn decode_leases(events: &[Value]) -> HashMap<(String, String), CodingSessionLeaseState> {
    let mut leases = HashMap::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64) != Some(u64::from(KIND_CODING_SESSION_LEASE)) {
            continue;
        }
        let (Some(content), Some(signer)) = (content_of(event), event_str(event, "pubkey")) else {
            continue;
        };
        let Ok(lease) = decode_coding_session_lease(content) else {
            continue;
        };
        leases.insert(
            (signer, coding_session_target_key(&lease.target)),
            lease.state,
        );
    }
    leases
}

/// Fold the three durable streams plus the live lease snapshot into one row
/// per generation.
///
/// The row set is exactly [`resolve_sessions`]'s, so `bee sessions status` and
/// `bee sessions list` can never disagree about which generations exist; this
/// adds the seat, the umbrella, and the liveness on top.
pub fn build_executions(
    metadata: &[MetadataRecord],
    receipts: &[ReceiptRecord],
    transcripts: &[TranscriptRecord],
    leases: &HashMap<(String, String), CodingSessionLeaseState>,
    now: i64,
) -> Vec<CrewExecution> {
    let mut rows: Vec<CrewExecution> = resolve_sessions(metadata, receipts, transcripts)
        .into_iter()
        .map(|row| {
            let own: Vec<&MetadataRecord> = metadata
                .iter()
                .filter(|record| record.signer == row.signer && record.target_key == row.target_key)
                .collect();
            let (newest, _) = newest_metadata(&own);
            let newest_item = transcripts
                .iter()
                .filter(|record| record.signer == row.signer && record.target_key == row.target_key)
                .max_by_key(|record| (record.seq, record.created_at));

            let last_signed_seq = newest_item.map(|record| record.seq);
            let last_signed_at = newest_item.map(|record| record.created_at);
            let liveness = derive_liveness(
                leases
                    .get(&(row.signer.clone(), row.target_key.clone()))
                    .copied(),
                &row.status,
                last_signed_at,
                now,
            );

            CrewExecution {
                target_key: row.target_key,
                target: row.target,
                signer: row.signer,
                actor: newest.and_then(|record| record.metadata.agent_ref.clone()),
                role: newest.and_then(|record| record.metadata.role.clone()),
                session_ref: newest.and_then(|record| record.metadata.session_ref.clone()),
                status: row.status,
                model: newest.and_then(|record| record.metadata.model.clone()),
                runtime: newest.and_then(|record| record.metadata.runtime.clone()),
                last_signed_seq,
                last_signed_at,
                liveness,
                turn_budget: newest
                    .and_then(|record| record.metadata.turn_budget)
                    .map(TurnBudget::from),
            }
        })
        .collect();
    apply_umbrella_turn_budget(&mut rows);
    rows
}

/// Raise every seat's reported budget to the umbrella's, so no row advertises
/// room the crew session no longer has.
///
/// The provider publishes `turnBudget` on the metadata of whichever execution
/// is *acting*, so a sibling that has been idle keeps echoing the count it
/// last saw. The count itself is one number per umbrella, and the highest
/// `used` any seat has published is the closest any reader can get to it —
/// counts only ever rise, so the maximum is the newest fact, and lending it to
/// the umbrella's other seats can only ever make them more accurate. An
/// execution that claimed no `sessionRef` is not part of any umbrella and
/// keeps exactly what its own metadata said.
fn apply_umbrella_turn_budget(rows: &mut [CrewExecution]) {
    let mut newest: HashMap<&str, TurnBudget> = HashMap::new();
    for row in rows.iter() {
        let (Some(session_ref), Some(budget)) = (row.session_ref.as_deref(), row.turn_budget)
        else {
            continue;
        };
        newest
            .entry(session_ref)
            .and_modify(|held| {
                if (budget.used, budget.limit) > (held.used, held.limit) {
                    *held = budget;
                }
            })
            .or_insert(budget);
    }
    let newest: HashMap<String, TurnBudget> = newest
        .into_iter()
        .map(|(session_ref, budget)| (session_ref.to_owned(), budget))
        .collect();
    for row in rows.iter_mut() {
        let Some(session_ref) = row.session_ref.as_deref() else {
            continue;
        };
        if let Some(budget) = newest.get(session_ref) {
            row.turn_budget = Some(*budget);
        }
    }
}

/// Decide liveness from the lease first, the durable status second, and the
/// transcript clock last.
///
/// The order matters: a `released` lease and a `stopped` status are both
/// positive claims that nothing is running, while silence is only ever
/// reported as silence.
pub fn derive_liveness(
    lease: Option<CodingSessionLeaseState>,
    status: &str,
    last_signed_at: Option<i64>,
    now: i64,
) -> Liveness {
    match lease {
        Some(CodingSessionLeaseState::Live) => return Liveness::Live,
        Some(CodingSessionLeaseState::Released) => return Liveness::Released,
        None => {}
    }
    if is_terminal_status(status) {
        return Liveness::Released;
    }
    match last_signed_at {
        Some(at) => Liveness::Quiet {
            age_secs: (now - at).max(0),
        },
        None => Liveness::Unknown,
    }
}

/// The wire string [`SessionStatus::Stopped`] serializes as, asked of the
/// enum rather than written out, so a rename cannot silently pass this by.
fn stopped_status_word() -> String {
    serde_json::to_value(SessionStatus::Stopped)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "stopped".to_owned())
}

// ── Turn commands (kind 44220) ───────────────────────────────────────────────

/// One decoded 44220 command, with the event facts a mailbox needs.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnCommand {
    /// Event id — the inbox cursor.
    pub event_id: String,
    /// Who signed it. The relay checked their channel membership at ingest.
    pub signer: String,
    /// Event `created_at`, Unix seconds.
    pub created_at: i64,
    /// Payload `commandId` — the key every receipt answers on.
    pub command_id: String,
    /// The addressed generation.
    pub target: CodingSessionTarget,
    /// `cs-target` key for the addressed generation.
    pub target_key: String,
    /// The requested action, kept typed so nothing is lost in translation.
    pub action: CodingSessionAction,
}

impl TurnCommand {
    /// Turn text, or `None` for `thread.turn.interrupt` or
    /// `thread.turn.continue_on_ci`, neither of which carries turn text of
    /// its own. A CI-continuation registration's `continuation` is not this
    /// command's turn text — the provider materializes the eventual turn's
    /// text from the verified result later, under this same commandId — so
    /// crew's pointer matching must not see it here.
    pub fn text(&self) -> Option<&str> {
        match &self.action {
            CodingSessionAction::ThreadTurnStart { text, .. } => Some(text),
            CodingSessionAction::ThreadTurnInterrupt
            | CodingSessionAction::ThreadTurnContinueOnCi { .. } => None,
        }
    }

    /// Requested delivery class; `boundary` for an interrupt's own command
    /// or a CI-continuation registration, neither of which is a delivery
    /// class request of its own on the wire.
    pub fn deliver(&self) -> CodingSessionDelivery {
        match &self.action {
            CodingSessionAction::ThreadTurnStart { deliver, .. } => *deliver,
            CodingSessionAction::ThreadTurnInterrupt => CodingSessionDelivery::Interrupt,
            CodingSessionAction::ThreadTurnContinueOnCi { .. } => CodingSessionDelivery::Boundary,
        }
    }
}

/// Decode every 44220 in `events`, dropping and counting the unreadable.
pub fn decode_turn_commands(events: &[Value]) -> (Vec<TurnCommand>, DecodeStats) {
    let mut records = Vec::new();
    let mut stats = DecodeStats::default();
    for event in events {
        if event.get("kind").and_then(Value::as_u64) != Some(u64::from(KIND_CODING_SESSION_COMMAND))
        {
            continue;
        }
        let (Some(content), Some(signer), Some(created_at), Some(event_id)) = (
            content_of(event),
            event_str(event, "pubkey"),
            event_created_at(event),
            event_str(event, "id"),
        ) else {
            stats.malformed += 1;
            continue;
        };
        let Ok(payload) = serde_json::from_str::<CodingSessionCommandPayload>(content) else {
            stats.malformed += 1;
            continue;
        };
        records.push(TurnCommand {
            event_id,
            signer,
            created_at,
            command_id: payload.command_id,
            target_key: coding_session_target_key(&payload.target),
            target: payload.target,
            action: payload.action,
        });
    }
    (records, stats)
}

/// Who last resumed an execution, read from the signed 44221 that asked.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumeRecord {
    /// Pubkey that signed the resume — the operator, not the provider.
    pub signer: String,
    /// Event `created_at`, Unix seconds.
    pub created_at: i64,
    /// The generation the resume was asked *of*; the provider mints N+1.
    pub previous: CodingSessionTarget,
}

/// Decode every `session.resume` among a channel's 44221 events.
pub fn decode_resumes(events: &[Value]) -> Vec<ResumeRecord> {
    let mut records = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_COMMAND))
        {
            continue;
        }
        let (Some(content), Some(signer), Some(created_at)) = (
            content_of(event),
            event_str(event, "pubkey"),
            event_created_at(event),
        ) else {
            continue;
        };
        let Ok(payload) =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                content,
            )
        else {
            continue;
        };
        if let CodingSessionLifecycleAction::SessionResume { session, .. } = payload.action {
            records.push(ResumeRecord {
                signer,
                created_at,
                previous: session,
            });
        }
    }
    records
}

// ── Founders ─────────────────────────────────────────────────────────────────

/// Who stands behind one execution: the human who asked for it, and the
/// founder of the umbrella that request pointed at.
///
/// Both fields are `Option` and both print `null` when unknown. Nothing here
/// ever falls back to the *provider's* signer: a provider signs every
/// execution in the channel, so a founder read off it would make every
/// session look like it belonged to the same person — which is exactly the
/// mistake that sent a probe aimed at a quiet session into someone else's
/// (SESSION_STATE item 73).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Founding {
    /// Pubkey that signed the 44221 `session.create` a 44224 receipt joined to
    /// this execution, or `None` when this channel holds no such join.
    pub create_signer: Option<String>,
    /// Pubkey that signed the 44226 genesis that create named, or `None` when
    /// the create named none, the genesis is not in this channel, or it founds
    /// a different umbrella than the create claimed.
    pub founder: Option<String>,
}

/// A 44226 genesis, reduced to the two facts a founder lookup needs.
struct GenesisRecord {
    /// The founder: whoever signed the genesis.
    signer: String,
    /// The umbrella it founds.
    session_ref: String,
}

/// A 44221 `session.create`, reduced to the facts the join needs.
struct CreateRecord {
    event_id: String,
    signer: String,
    created_at: i64,
    command_id: String,
    session_ref: Option<String>,
    genesis_ref: Option<String>,
    /// The provider the create addressed — the only signer whose receipt may
    /// answer it.
    provider_authority_pubkey: String,
}

/// Founding facts for every execution one channel's record can join.
///
/// Keyed by the execution *identity* `(driver, instanceId, sessionId)` rather
/// than by the exact generation. A `session.create` mints generation 1; every
/// later generation of the same execution comes from a `session.resume`, which
/// founds nothing. So the founding create's signer is the answer for every
/// generation of that execution, and a resume signer is never one — see
/// [`decode_resumes`], whose records this fold deliberately ignores.
#[derive(Debug, Clone, Default)]
pub struct FounderIndex {
    by_identity: HashMap<(String, String, String), Founding>,
}

impl FounderIndex {
    /// What this channel says about the generation's founding.
    ///
    /// [`Founding::default`] — both fields `None` — when the channel holds no
    /// joined create for it. That is a statement about the record in front of
    /// this caller, not about the session: an execution created in a channel
    /// whose 44221 has aged out reads as unknown, and unknown prints `null`.
    pub fn of(&self, target: &CodingSessionTarget) -> Founding {
        self.by_identity
            .get(&(
                target.driver.clone(),
                target.instance_id.clone(),
                target.session_id.clone(),
            ))
            .cloned()
            .unwrap_or_default()
    }

    /// Every distinct founder the channel names, sorted.
    pub fn founders(&self) -> Vec<String> {
        let distinct: BTreeSet<&str> = self
            .by_identity
            .values()
            .filter_map(|founding| founding.founder.as_deref())
            .collect();
        distinct.into_iter().map(str::to_owned).collect()
    }
}

/// Decode every 44226 genesis, keyed by its **event id**.
///
/// Canonical identity is the event id, never the `csg-session` tag: NIP-CSG is
/// explicit that a `#csg-session` lookup is not a founder lookup, because a
/// consumer that selects by label has already accepted that two rows might
/// answer and that it may pick one.
fn decode_geneses(events: &[Value]) -> HashMap<String, GenesisRecord> {
    let mut geneses = HashMap::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64) != Some(u64::from(KIND_CODING_SESSION_GENESIS))
        {
            continue;
        }
        let (Some(content), Some(signer), Some(event_id)) = (
            content_of(event),
            event_str(event, "pubkey"),
            event_str(event, "id"),
        ) else {
            continue;
        };
        let Ok(payload) = decode_coding_session_genesis(content) else {
            continue;
        };
        geneses.insert(
            event_id,
            GenesisRecord {
                signer,
                session_ref: payload.session_ref,
            },
        );
    }
    geneses
}

/// Decode every `session.create` among a channel's 44221 events.
fn decode_creates(events: &[Value]) -> Vec<CreateRecord> {
    let mut records = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_COMMAND))
        {
            continue;
        }
        let (Some(content), Some(signer), Some(created_at), Some(event_id)) = (
            content_of(event),
            event_str(event, "pubkey"),
            event_created_at(event),
            event_str(event, "id"),
        ) else {
            continue;
        };
        let Ok(payload) =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                content,
            )
        else {
            continue;
        };
        if let CodingSessionLifecycleAction::SessionCreate {
            session_ref,
            genesis_ref,
            provider_authority_pubkey,
            ..
        } = payload.action
        {
            records.push(CreateRecord {
                event_id,
                signer,
                created_at,
                command_id: payload.command_id,
                session_ref,
                genesis_ref,
                provider_authority_pubkey,
            });
        }
    }
    records
}

/// The one target a create's own provider says it minted, or `None`.
///
/// The self-fence: only receipts signed by the very pubkey the create *named*
/// are read as its answer, so a stranger's receipt never joins even in a
/// channel that happily stores it. Receipts from that provider naming
/// different targets for one `commandId` are the provider contradicting
/// itself, and a disputed claim resolves to nothing rather than to a side.
fn joined_target<'a>(
    receipts: &'a [ReceiptRecord],
    command_id: &str,
    provider_authority_pubkey: &str,
) -> Option<&'a CodingSessionTarget> {
    let mut answer: Option<&CodingSessionTarget> = None;
    for record in receipts {
        // A turn receipt names the generation its 44220 addressed but never
        // creates one, so it can never be a create's answer (NIP-CSL fork
        // amendment 7).
        if record.is_turn_status || record.signer != provider_authority_pubkey {
            continue;
        }
        let Some(target) = record.target.as_ref() else {
            continue;
        };
        let Some(content) = content_of(&record.raw) else {
            continue;
        };
        let Ok(receipt) = serde_json::from_str::<LifecycleReceipt>(content) else {
            continue;
        };
        if receipt.command_id != command_id {
            continue;
        }
        match answer {
            Some(held) if coding_session_target_key(held) != coding_session_target_key(target) => {
                return None
            }
            _ => answer = Some(target),
        }
    }
    answer
}

/// Fold a channel's geneses, creates, and lifecycle receipts into founding
/// facts per execution.
///
/// Three rules, each of which refuses rather than guesses:
///
/// 1. **Only receipt-joined creates are observed.** A create nobody's provider
///    ever acted on mints no execution, so it is not evidence of founding one
///    — and requiring the join is what stops a member from backdating a create
///    bearing someone else's `sessionRef`.
/// 2. **A disputed `commandId` founds nothing.** Two creates sharing one id
///    but disagreeing about signer, umbrella, genesis, or addressed provider
///    are a contradiction; so are two joined creates claiming one execution
///    for different signers.
/// 3. **A genesis becomes founder only through an explicit event id**, and
///    only when it founds the umbrella the create claimed.
///
/// The same three rules the desktop applies
/// (`desktop/src/features/coding-sessions/lib/codingSessionCreateObservations.ts`),
/// so `bee` and the app never name different founders for one session.
pub fn build_founder_index(events: &[Value], receipts: &[ReceiptRecord]) -> FounderIndex {
    let geneses = decode_geneses(events);

    let mut by_command: BTreeMap<String, Vec<CreateRecord>> = BTreeMap::new();
    for create in decode_creates(events) {
        by_command
            .entry(create.command_id.clone())
            .or_default()
            .push(create);
    }

    let mut index = FounderIndex::default();
    // `None` marks an identity two joined creates disagree about; it stays
    // absent from the index rather than taking either side.
    let mut resolved: HashMap<(String, String, String), Option<Founding>> = HashMap::new();

    for (command_id, mut group) in by_command {
        group.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.event_id.cmp(&right.event_id))
        });
        let disputed = group.iter().any(|create| {
            create.signer != group[0].signer
                || create.session_ref != group[0].session_ref
                || create.genesis_ref != group[0].genesis_ref
                || create.provider_authority_pubkey != group[0].provider_authority_pubkey
        });
        if disputed {
            continue;
        }
        let create = &group[0];
        let Some(target) = joined_target(receipts, &command_id, &create.provider_authority_pubkey)
        else {
            continue;
        };
        let founder = create
            .genesis_ref
            .as_deref()
            .and_then(|genesis_ref| geneses.get(genesis_ref))
            .filter(|genesis| Some(genesis.session_ref.as_str()) == create.session_ref.as_deref())
            .map(|genesis| genesis.signer.clone());
        let founding = Founding {
            create_signer: Some(create.signer.clone()),
            founder,
        };
        let identity = (
            target.driver.clone(),
            target.instance_id.clone(),
            target.session_id.clone(),
        );
        match resolved.get(&identity) {
            Some(Some(held)) if *held != founding => {
                resolved.insert(identity, None);
            }
            Some(_) => {}
            None => {
                resolved.insert(identity, Some(founding));
            }
        }
    }

    for (identity, founding) in resolved {
        if let Some(founding) = founding {
            index.by_identity.insert(identity, founding);
        }
    }
    index
}

// ── Receipt stages ───────────────────────────────────────────────────────────

/// The newest stage one 44220 has been answered with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnStage {
    /// The stage itself.
    pub status: ReceiptStatus,
    /// `error.code`, when the stage carried one.
    pub error_code: Option<String>,
    /// `error.message`, when the stage carried one.
    pub error_message: Option<String>,
    /// The provider's `turnId`, present exactly for `turn_started`.
    pub turn_id: Option<String>,
    /// Receipt `created_at`, Unix seconds.
    pub at: i64,
}

/// Order two stages that share a second.
///
/// Second-granularity `created_at` cannot separate `turn_queued` from the
/// `turn_started` that follows it in the same second, and a reader that picked
/// either arbitrarily would report a running turn as merely queued half the
/// time. The rank is the contract's own progression.
fn stage_rank(status: ReceiptStatus) -> u8 {
    match status {
        ReceiptStatus::TurnQueued => 1,
        ReceiptStatus::TurnDegraded => 2,
        ReceiptStatus::TurnStarted => 3,
        ReceiptStatus::TurnDropped | ReceiptStatus::TurnRefused => 4,
        ReceiptStatus::InterruptDelivered => 5,
        _ => 0,
    }
}

/// Fold every turn receipt into the newest stage per `commandId`.
pub fn newest_turn_stages(receipts: &[ReceiptRecord]) -> HashMap<String, TurnStage> {
    let mut newest: HashMap<String, (i64, u8, String, TurnStage)> = HashMap::new();
    for record in receipts {
        let Some(content) = content_of(&record.raw) else {
            continue;
        };
        let Ok(receipt) = serde_json::from_str::<LifecycleReceipt>(content) else {
            continue;
        };
        if !receipt.status.is_turn_stage() {
            continue;
        }
        let event_id = event_str(&record.raw, "id").unwrap_or_default();
        let rank = stage_rank(receipt.status);
        let key = (record.created_at, rank, event_id);
        let stage = TurnStage {
            status: receipt.status,
            error_code: receipt.error.as_ref().map(|error| error.code.clone()),
            error_message: receipt.error.as_ref().map(|error| error.message.clone()),
            turn_id: receipt.turn_id.clone(),
            at: record.created_at,
        };
        match newest.get(&receipt.command_id) {
            Some((at, existing_rank, id, _))
                if (*at, *existing_rank, id.as_str()) >= (key.0, key.1, key.2.as_str()) => {}
            _ => {
                newest.insert(receipt.command_id.clone(), (key.0, key.1, key.2, stage));
            }
        }
    }
    newest
        .into_iter()
        .map(|(command_id, (_, _, _, stage))| (command_id, stage))
        .collect()
}

// ── Delivery, as the provider answered it ────────────────────────────────────

/// How long `bee sessions send` waits for the first receipt for its command.
///
/// The relay's `accepted:true` is a statement about storage, not about
/// delivery; the provider answers separately and usually within a second.
/// Ten seconds is long enough to cover a busy provider's first poll and short
/// enough that a scripted sender is not left hanging — after it, the command
/// reports that nothing answered rather than guessing that something did.
pub const DELIVERY_WAIT_SECONDS: u64 = 10;

/// What became of a turn the relay accepted.
///
/// Ledger 80 (c): `--deliver steer` printed `accepted:true` while the provider
/// was quietly degrading the steer to a boundary delivery, so the sender
/// believed a mid-turn injection had happened that had not. `accepted` is kept
/// as what it always was — the relay stored the command — and this carries the
/// separate fact of what the execution did with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryReport {
    /// `Some(true)` when a receipt says the turn reached the execution in some
    /// form, `Some(false)` when a receipt says it will never run, and `None`
    /// when no receipt arrived inside [`DELIVERY_WAIT_SECONDS`] — unknown is
    /// its own answer and is never rendered as either of the other two.
    pub delivered: Option<bool>,
    /// The receipt's own status word, or `unconfirmed` when none arrived.
    pub status: &'static str,
    /// One sentence naming what happened, in the sender's terms.
    pub detail: String,
}

/// The status word used when no receipt answered inside the wait.
pub const DELIVERY_UNCONFIRMED: &str = "unconfirmed";

/// Fold the requested delivery class and the receipt that answered it into the
/// report `bee sessions send` prints.
///
/// `waited` says whether the sender actually gave the provider a chance to
/// answer, so the two ways of having no receipt — nobody answered, and nobody
/// was asked — are never printed as the same sentence.
///
/// Pure so the wording of an unpleasant answer is testable without a relay:
/// the whole point of this function is that a degraded steer reads as a
/// degraded steer.
pub fn fold_delivery(
    requested: CodingSessionDelivery,
    stage: Option<&TurnStage>,
    waited: bool,
) -> DeliveryReport {
    let asked = requested.as_str();
    let Some(stage) = stage else {
        return DeliveryReport {
            delivered: None,
            status: DELIVERY_UNCONFIRMED,
            detail: if waited {
                format!(
                    "the relay stored the command, but no provider receipt arrived within \
                     {DELIVERY_WAIT_SECONDS}s — whether the turn reached the execution is unknown"
                )
            } else {
                "the relay stored the command; --no-wait skipped the receipt read, so whether \
                 the turn reached the execution is unknown"
                    .to_owned()
            },
        };
    };
    let error = match (stage.error_code.as_deref(), stage.error_message.as_deref()) {
        (Some(code), Some(message)) => format!("{code} — {message}"),
        (Some(code), None) => code.to_owned(),
        (None, Some(message)) => message.to_owned(),
        (None, None) => "no reason given".to_owned(),
    };
    let (delivered, detail) = match stage.status {
        // The degradation this report exists for: the runtime advertised no
        // native steering, so the words are queued for the next boundary.
        ReceiptStatus::TurnDegraded => (
            Some(true),
            format!("{asked} requested, provider degraded to boundary"),
        ),
        ReceiptStatus::TurnQueued => (
            Some(true),
            format!("{asked} accepted; the turn is queued and has not started yet"),
        ),
        ReceiptStatus::TurnStarted => (
            Some(true),
            match stage.turn_id.as_deref() {
                Some(turn_id) => format!("{asked} accepted; the turn is running as {turn_id}"),
                None => format!("{asked} accepted; the turn is running"),
            },
        ),
        ReceiptStatus::InterruptDelivered => (
            Some(true),
            format!("{asked} delivered; the running turn was cancelled"),
        ),
        ReceiptStatus::TurnDropped => (
            Some(false),
            format!("not delivered: the turn was dropped ({error})"),
        ),
        ReceiptStatus::TurnRefused => (
            Some(false),
            format!("not delivered: the turn was refused ({error})"),
        ),
        // `newest_turn_stages` only ever yields `is_turn_stage` statuses, so
        // this arm is unreachable through the CLI. It still refuses to guess:
        // a lifecycle word over a turn command is not evidence of delivery.
        other => (
            None,
            format!(
                "the provider answered `{}`, which does not report a turn stage",
                other.as_str()
            ),
        ),
    };
    DeliveryReport {
        delivered,
        status: stage.status.as_str(),
        detail,
    }
}

// ── Addressing ───────────────────────────────────────────────────────────────

/// Collapse candidate generations of one execution to its newest.
///
/// Several generations of the *same* execution are not an ambiguity — the
/// newest is the one a turn can reach. Several *distinct* executions are.
fn current_generation<'a>(
    candidates: &[&'a CrewExecution],
    what: &str,
) -> Result<&'a CrewExecution, CliError> {
    let mut by_identity: BTreeMap<(String, String, String), &'a CrewExecution> = BTreeMap::new();
    for candidate in candidates {
        by_identity
            .entry(candidate.identity())
            .and_modify(|current| {
                if candidate.target.generation > current.target.generation {
                    *current = candidate;
                }
            })
            .or_insert(candidate);
    }
    let mut newest: Vec<&&CrewExecution> = by_identity.values().collect();
    newest.sort_by(|left, right| left.target_key.cmp(&right.target_key));
    match newest.as_slice() {
        [] => Err(CliError::NotFound(format!("nothing matches {what}"))),
        [one] => Ok(**one),
        many => Err(CliError::Usage(format!(
            "{what} matches {} executions — pass --to with one of: {}",
            many.len(),
            many.iter()
                .map(|execution| execution.target_key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Resolve `--to` to exactly one generation.
///
/// `umbrella` scopes a role lookup and is required for one: see rule 2 in the
/// module docs. A name that answers as *both* a session id and a role is an
/// ambiguity, not a precedence question — the caller is told both readings.
pub fn resolve_send_target<'a>(
    executions: &'a [CrewExecution],
    to: &str,
    umbrella: Option<&str>,
) -> Result<&'a CrewExecution, CliError> {
    let by_key: Vec<&CrewExecution> = executions
        .iter()
        .filter(|execution| execution.target_key == to)
        .collect();
    if !by_key.is_empty() {
        return match by_key.as_slice() {
            [one] => Ok(one),
            many => Err(CliError::Usage(format!(
                "cs-target '{to}' is published by {} different signers: {}",
                many.len(),
                many.iter()
                    .map(|execution| execution.signer.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        };
    }

    let by_session: Vec<&CrewExecution> = executions
        .iter()
        .filter(|execution| execution.target.session_id == to)
        .collect();
    let role_matches: Vec<&CrewExecution> = executions
        .iter()
        .filter(|execution| execution.role.as_deref() == Some(to))
        .collect();

    if !by_session.is_empty() && !role_matches.is_empty() {
        return Err(CliError::Usage(format!(
            "'{to}' is both a session id ({}) and a role slug ({}) in this channel — \
             pass the exact cs-target key instead",
            by_session
                .iter()
                .map(|execution| execution.target_key.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            role_matches
                .iter()
                .map(|execution| execution.target_key.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }

    if !by_session.is_empty() {
        return current_generation(&by_session, &format!("session id '{to}'"));
    }

    if role_matches.is_empty() {
        return Err(CliError::NotFound(format!(
            "no execution in this channel answers to '{to}' as a cs-target key, \
             a session id, or a role slug"
        )));
    }

    let Some(umbrella) = umbrella else {
        let umbrellas: BTreeSet<&str> = role_matches
            .iter()
            .filter_map(|execution| execution.session_ref.as_deref())
            .collect();
        return Err(CliError::Usage(format!(
            "role '{to}' is only unique inside one umbrella session, and none was given — \
             pass --session-ref (seen here: {}), or address the seat by its cs-target key",
            if umbrellas.is_empty() {
                "none; the matching seats claim no umbrella".to_owned()
            } else {
                umbrellas.into_iter().collect::<Vec<_>>().join(", ")
            }
        )));
    };

    let scoped: Vec<&CrewExecution> = role_matches
        .iter()
        .copied()
        .filter(|execution| execution.session_ref.as_deref() == Some(umbrella))
        .collect();
    if scoped.is_empty() {
        let umbrellas: BTreeSet<&str> = role_matches
            .iter()
            .filter_map(|execution| execution.session_ref.as_deref())
            .collect();
        return Err(CliError::NotFound(format!(
            "no seat holds role '{to}' in umbrella {umbrella}; the role exists in: {}",
            if umbrellas.is_empty() {
                "no umbrella (those seats claim none)".to_owned()
            } else {
                umbrellas.into_iter().collect::<Vec<_>>().join(", ")
            }
        )));
    }
    current_generation(&scoped, &format!("role '{to}' in umbrella {umbrella}"))
}

/// The umbrella the caller's own seat sits in, when there is exactly one.
///
/// `Ok(None)` means the caller holds no seat in this channel — role lookup
/// then needs an explicit `--session-ref` rather than a guess.
pub fn caller_umbrella(
    executions: &[CrewExecution],
    caller_pubkey: &str,
) -> Result<Option<String>, CliError> {
    let umbrellas: BTreeSet<&str> = executions
        .iter()
        .filter(|execution| execution.actor.as_deref() == Some(caller_pubkey))
        .filter_map(|execution| execution.session_ref.as_deref())
        .collect();
    match umbrellas.len() {
        0 => Ok(None),
        1 => Ok(umbrellas.into_iter().next().map(str::to_owned)),
        _ => Err(CliError::Usage(format!(
            "this identity is seated in {} umbrella sessions ({}) — pass --session-ref to say which",
            umbrellas.len(),
            umbrellas.into_iter().collect::<Vec<_>>().join(", ")
        ))),
    }
}

// ── Re-addressing an owed turn (plan ruling R1) ──────────────────────────────

/// A resolved `--readdress`: the original words, and where they go now.
#[derive(Debug, Clone, PartialEq)]
pub struct ReaddressPlan {
    /// `commandId` of the 44220 that was answered but never ran.
    pub source_command_id: String,
    /// The original turn text, re-sent byte-for-byte.
    pub text: String,
    /// The original delivery class.
    pub deliver: CodingSessionDelivery,
    /// The generation the original turn addressed.
    pub refused_target: CodingSessionTarget,
    /// The stage that answered it, e.g. `turn_dropped`.
    pub refused_stage: ReceiptStatus,
    /// The code that stage carried, e.g. `NO_LIVE_EXECUTION`.
    pub refused_code: String,
    /// The generation the re-send addresses.
    pub target: CodingSessionTarget,
    /// `cs-target` key of that generation.
    pub target_key: String,
    /// Pubkey that signed the newest `session.resume` for this execution, or
    /// `None` when no resume is on the record.
    pub resumed_by: Option<String>,
}

/// Codes a turn can be answered with that leave the sender owed a re-send.
///
/// This is the single ruling on what `--readdress` recovers, and every surface
/// that *offers* it must ask here first: `doctor` printing "re-address it" over
/// an answer [`plan_readdress`] then refuses is a command telling its reader to
/// run something it will not run.
pub(super) fn readdressable_reason(stage: &TurnStage) -> Option<String> {
    let code = stage.error_code.as_deref()?;
    match (stage.status, code) {
        (ReceiptStatus::TurnDropped, NO_LIVE_EXECUTION)
        | (ReceiptStatus::TurnRefused, STALE_GENERATION) => Some(code.to_owned()),
        _ => None,
    }
}

/// Resolve `--readdress <commandId>` into a fresh send against the current
/// generation of the same execution.
///
/// Answers the three questions ruling R1 left open, and refuses rather than
/// guesses on each:
///
/// - *Which generation?* The highest generation of the same
///   `(driver, instanceId, sessionId)`. When that is still the generation that
///   refused the turn, the re-send is allowed only if a `live` lease says an
///   actor came back — otherwise the caller is told to resume it first.
/// - *Who resumed it?* The signer of the newest `session.resume` naming an
///   earlier generation of this execution, reported as `resumedBy`.
/// - *And if the session is closed?* A durably stopped execution is refused,
///   because a stopped session accepts no turns at all.
pub fn plan_readdress(
    commands: &[TurnCommand],
    stages: &HashMap<String, TurnStage>,
    executions: &[CrewExecution],
    resumes: &[ResumeRecord],
    command_id: &str,
) -> Result<ReaddressPlan, CliError> {
    let source = commands
        .iter()
        .filter(|command| command.command_id == command_id)
        .max_by_key(|command| (command.created_at, command.event_id.clone()))
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "no coding-session command with commandId '{command_id}' in this channel"
            ))
        })?;
    let (text, deliver) = match &source.action {
        CodingSessionAction::ThreadTurnStart { text, deliver, .. } => (text.clone(), *deliver),
        CodingSessionAction::ThreadTurnInterrupt => {
            return Err(CliError::Usage(format!(
                "command '{command_id}' is a thread.turn.interrupt — \
                 it carries no text to re-address"
            )))
        }
        CodingSessionAction::ThreadTurnContinueOnCi { .. } => {
            return Err(CliError::Usage(format!(
                "command '{command_id}' is a thread.turn.continue_on_ci — it registers a turn \
                 rather than carrying one to re-address"
            )))
        }
    };

    let stage = stages.get(command_id).ok_or_else(|| {
        CliError::Usage(format!(
            "command '{command_id}' has no turn receipt yet — nothing says it did not run"
        ))
    })?;
    let refused_code = readdressable_reason(stage).ok_or_else(|| {
        CliError::Usage(format!(
            "command '{command_id}' was answered {}{} — --readdress is only for a \
             turn_dropped/{NO_LIVE_EXECUTION} or turn_refused/{STALE_GENERATION}",
            stage.status.as_str(),
            stage
                .error_code
                .as_deref()
                .map(|code| format!("/{code}"))
                .unwrap_or_default()
        ))
    })?;

    let candidates: Vec<&CrewExecution> = executions
        .iter()
        .filter(|execution| {
            execution.target.driver == source.target.driver
                && execution.target.instance_id == source.target.instance_id
                && execution.target.session_id == source.target.session_id
        })
        .collect();
    let current = current_generation(
        &candidates,
        &format!("execution '{}'", source.target.session_id),
    )
    .map_err(|error| match error {
        CliError::NotFound(_) => CliError::NotFound(format!(
            "execution '{}' has no generation on the record in this channel — \
             nothing to re-address to",
            source.target.session_id
        )),
        other => other,
    })?;

    if current.status == stopped_status_word() {
        return Err(CliError::Usage(format!(
            "execution '{}' was durably stopped (generation {}, status {}) — \
             a stopped session accepts no turns; create a new one",
            source.target.session_id, current.target.generation, current.status
        )));
    }
    if current.target.generation == source.target.generation && current.liveness != Liveness::Live {
        return Err(CliError::Usage(format!(
            "execution '{}' is still on generation {} and reads {} — resume it \
             (desktop, or a 44221 session.resume) and re-address then; re-sending \
             into a generation with no live actor only earns another {refused_code}",
            source.target.session_id,
            current.target.generation,
            current.liveness.render()
        )));
    }

    let resumed_by = resumes
        .iter()
        .filter(|resume| {
            resume.previous.driver == source.target.driver
                && resume.previous.instance_id == source.target.instance_id
                && resume.previous.session_id == source.target.session_id
                && resume.previous.generation < current.target.generation
        })
        .max_by_key(|resume| (resume.previous.generation, resume.created_at))
        .map(|resume| resume.signer.clone());

    Ok(ReaddressPlan {
        source_command_id: command_id.to_owned(),
        text,
        deliver,
        refused_target: source.target.clone(),
        refused_stage: stage.status,
        refused_code,
        target: current.target.clone(),
        target_key: current.target_key.clone(),
        resumed_by,
    })
}

// ── Inbox ────────────────────────────────────────────────────────────────────

/// One turn addressed to a seat this identity holds.
#[derive(Debug, Clone, PartialEq)]
pub struct InboxRow {
    /// Event id — the value `--since` takes.
    pub event_id: String,
    /// Event `created_at`, Unix seconds.
    pub created_at: i64,
    /// Who sent it.
    pub from: String,
    /// Payload `commandId`.
    pub command_id: String,
    /// `cs-target` key of the addressed seat.
    pub target_key: String,
    /// Turn text, or `None` for an interrupt.
    pub text: Option<String>,
    /// Requested delivery class.
    pub deliver: CodingSessionDelivery,
    /// Newest receipt stage, or `None` when nothing has answered yet.
    pub stage: Option<TurnStage>,
}

/// Every 44220 addressed to one of `mine`, oldest first, with its stage.
///
/// Only commands *addressed to* the caller's own seats are returned: an inbox
/// is a mailbox, not a channel-wide feed, so a sibling's traffic never appears
/// here even though the relay would happily serve it.
pub fn build_inbox(
    commands: &[TurnCommand],
    stages: &HashMap<String, TurnStage>,
    mine: &HashSet<String>,
    since: Option<&str>,
) -> Result<Vec<InboxRow>, CliError> {
    let mut rows: Vec<InboxRow> = commands
        .iter()
        .filter(|command| mine.contains(&command.target_key))
        .map(|command| InboxRow {
            event_id: command.event_id.clone(),
            created_at: command.created_at,
            from: command.signer.clone(),
            command_id: command.command_id.clone(),
            target_key: command.target_key.clone(),
            text: command.text().map(str::to_owned),
            deliver: command.deliver(),
            stage: stages.get(&command.command_id).cloned(),
        })
        .collect();
    rows.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.event_id.cmp(&right.event_id))
    });

    let Some(since) = since else {
        return Ok(rows);
    };
    let position = rows.iter().position(|row| row.event_id == since);
    match position {
        Some(index) => Ok(rows.split_off(index.saturating_add(1))),
        None => Err(CliError::NotFound(format!(
            "--since {since} names no turn addressed to this identity in this channel"
        ))),
    }
}

// ── Status ───────────────────────────────────────────────────────────────────

/// What one execution owes and is running, folded from its receipts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TurnLoad {
    /// `commandId` of a turn that started and has not produced a terminal
    /// transcript item.
    pub open_command_id: Option<String>,
    /// The provider `turnId` of that turn.
    pub open_turn_id: Option<String>,
    /// Unix seconds the open turn started at.
    pub open_since: Option<i64>,
    /// Turns accepted into the mailbox that have not started.
    pub queued: usize,
}

/// Transcript item kinds that end a turn.
const TERMINAL_ITEM_KINDS: &[&str] = &["result", "interrupted"];

/// Fold one execution's commands, stages, and transcript into its open turn
/// and queue depth.
///
/// A `turn_started` receipt alone does not mean a turn is *still* running —
/// the provider publishes no receipt when it ends. The transcript does: an
/// open turn is one whose `turnId` has no terminal item behind it.
pub fn turn_load(
    execution: &CrewExecution,
    commands: &[TurnCommand],
    stages: &HashMap<String, TurnStage>,
    transcripts: &[TranscriptRecord],
) -> TurnLoad {
    let settled: HashSet<&str> = transcripts
        .iter()
        .filter(|record| record.target_key == execution.target_key)
        .filter(|record| {
            record
                .envelope
                .item
                .get("kind")
                .and_then(Value::as_str)
                .is_some_and(|kind| TERMINAL_ITEM_KINDS.contains(&kind))
        })
        .filter_map(|record| record.envelope.turn_id.as_deref())
        .collect();

    let mut load = TurnLoad::default();
    let mut mine: Vec<&TurnCommand> = commands
        .iter()
        .filter(|command| command.target_key == execution.target_key)
        .collect();
    mine.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.event_id.cmp(&right.event_id))
    });

    for command in mine {
        let Some(stage) = stages.get(&command.command_id) else {
            continue;
        };
        match stage.status {
            ReceiptStatus::TurnQueued | ReceiptStatus::TurnDegraded => load.queued += 1,
            ReceiptStatus::TurnStarted => {
                let open = match stage.turn_id.as_deref() {
                    Some(turn_id) => !settled.contains(turn_id),
                    // A started receipt with no turnId cannot be matched
                    // against the transcript; the execution's own status is
                    // the only remaining witness.
                    None => execution.status == "running",
                };
                if open {
                    load.open_command_id = Some(command.command_id.clone());
                    load.open_turn_id = stage.turn_id.clone();
                    load.open_since = Some(stage.at);
                }
            }
            _ => {}
        }
    }
    load
}

// ── Hire, as the host answered it (plan D14) ─────────────────────────────────

/// How long `bee sessions hire` waits for the founder's host to answer.
///
/// A hire is not a write the relay can settle: the host has to read the
/// request, apply its standing policy, pick an identity, cut a worktree, stage
/// custody, and only then publish a seated create the provider answers, and
/// the provider only enqueues its `created` receipt once actor startup
/// finishes. This is the same window Desktop's own receipt wait has always
/// used — `CODING_SESSION_CREW_RECEIPT_TIMEOUT_MS = 120_000` in
/// `desktop/src/features/coding-sessions/lib/codingSessionCrewReceipt.ts` —
/// and the two disagreeing is what broke on cleantest, 2026-09-01: the
/// builder's `created` receipt was signed at 19:11:28, three seconds after
/// the 19:11:25 hire, and the sixty-second window still closed without seeing
/// it. The CLI reported an unseated hire for a seat that existed and was
/// running, and no grant was ever written for it.
pub const HIRE_WAIT_SECONDS: u64 = 120;

/// The exact `bee sessions seat-repair` invocation that recovers one seat's
/// role authority, with this hire's own values substituted.
///
/// Printed rather than described: a lead reading an ungranted-seat outcome
/// needs the command, not the name of a command, and re-hiring — the obvious
/// wrong answer — cannot recover the seat and is forbidden.
pub fn seat_repair_command(channel: &str, session_ref: &str, actor: &str) -> String {
    format!(
        "bee sessions seat-repair --channel {channel} --session-ref {session_ref} --actor {actor}"
    )
}

/// The remediation sentence every ungranted-seat outcome ends with.
pub fn seat_repair_remedy(channel: &str, session_ref: &str, actor: &str) -> String {
    format!(
        "If the seat runs anyway, repair its authority with: {}. Never hire again.",
        seat_repair_command(channel, session_ref, actor)
    )
}

/// The seated create a host published in answer to a hire.
///
/// This is the whole of the hire's identity on the wire: the request itself
/// carries no id the answer echoes, so the answer is recognized by *what it
/// is* — a seated create, for this role, in this umbrella, published after the
/// request went out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiredSeat {
    /// Event id of the seated create.
    pub event_id: String,
    /// Pubkey that signed the seated create.
    pub create_signer: String,
    /// Its `commandId` — the key the provider's receipt answers on, and the
    /// id this command reports back as the hire's own receipt reference.
    pub command_id: String,
    /// Pubkey the host chose to seat. Never a key, always a name.
    pub actor: String,
    /// Role slug it seated.
    pub role: String,
    /// Umbrella it joined.
    pub session_ref: String,
    /// Exact genesis the create joined.
    pub genesis_ref: String,
    /// Provider authority whose receipt may prove the execution exists.
    pub provider_authority_pubkey: String,
    /// Provider instance **alias** the host ran it on — the request's, or the
    /// policy's.
    ///
    /// Typed since batch 2 lane B2. Comparing it with a receipt's
    /// `cs-target.instanceId` is ledger item 102, and is now a compile error.
    pub provider_instance_ref: ProviderInstanceAlias,
    /// Model the host chose, when the create named one.
    pub model: Option<String>,
    /// Event `created_at`, Unix seconds.
    pub at: i64,
    /// Signed create retained for trust verification before granting a seat.
    pub raw: Value,
}

/// What a provider answered a seated create with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatReceipt {
    /// Event id of the provider receipt.
    pub event_id: String,
    /// Pubkey that signed the receipt.
    pub signer: String,
    /// The receipt's own status word.
    pub status: ReceiptStatus,
    /// `cs-target` key of the execution it minted, absent for a failure.
    pub target_key: Option<String>,
    /// `error.code`, when the receipt carried one.
    pub error_code: Option<String>,
    /// `error.message`, when the receipt carried one.
    pub error_message: Option<String>,
    /// Receipt `created_at`, Unix seconds.
    pub at: i64,
    /// Signed receipt retained for trust verification before granting a seat.
    pub raw: Value,
}

/// A host's refusal of a hire, as it reached the requesting seat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HireRefusal {
    /// Event id of the 44220 turn carrying it.
    pub event_id: String,
    /// Turn `created_at`, Unix seconds.
    pub at: i64,
    /// The machine-readable code, one of
    /// [`buzz_core::coding_session_lifecycle_command::HIRE_REFUSAL_CODES`].
    pub code: String,
    /// The host's own sentence explaining it.
    pub reason: String,
}

/// What became of a hire request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HireOutcome {
    /// The host seated the role and the provider confirmed the execution.
    Created {
        /// The seated create.
        seat: HiredSeat,
        /// Its provider receipt.
        receipt: SeatReceipt,
        /// Whether the accepted authority chain names this exact actor-role
        /// pair with a live `grant-seat`.
        ///
        /// What an ungranted seat can and cannot do, exactly: the typed team
        /// fold **includes** its report — inclusion is assignee equality, and
        /// an assignment names a target rather than an authorship claim
        /// (`buzz_core::coding_session_team_transaction_fold`) — and discloses
        /// it under `unseatedReports` as carrying no seat authority. What the
        /// seat cannot do is hold `verifier` standing, so it can never refute;
        /// and it is absent from `activeSeats`, so nothing else in the product
        /// notices. Carried separately from the receipt because the provider's
        /// `created` says nothing about authority — on 2026-08-28 a hired
        /// builder had a `created` receipt and no grant and worked for 1,009 s
        /// (item 83); on 2026-09-01 the same thing happened again because the
        /// receipt landed after the CLI's window closed. The remedy is
        /// [`seat_repair_command`], never a second hire.
        granted: bool,
    },
    /// The host seated the role and the provider refused it.
    Failed {
        /// The seated create.
        seat: HiredSeat,
        /// Its provider receipt, carrying the code.
        receipt: SeatReceipt,
    },
    /// The host published a seated create and no provider receipt answered it
    /// inside the wait. Something exists; whether it runs is not yet known.
    Seating {
        /// The seated create.
        seat: HiredSeat,
    },
    /// More than one seated create for this role verifies against its own
    /// provider receipt, so which seat this hire got is genuinely ambiguous.
    ///
    /// Refused rather than resolved: the two tie-breakers that look obvious —
    /// earliest wins, newest wins — both read the author's own `created_at`,
    /// which is the defect T2.5 removed. Nothing is written and nothing is
    /// granted; the operator settles it with `seat-repair`.
    Ambiguous {
        /// The refusal sentence, from [`ambiguous_hire_refusal`].
        message: String,
    },
    /// The host refused the hire itself.
    Refused(HireRefusal),
    /// Nothing answered: no seated create, no refusal. Never rendered as
    /// either of the other two.
    Unconfirmed,
}

/// Every seated create that could answer a hire for `role` in `umbrella`.
///
/// **Not a choice (T2.5).** This used to be `find_hired_seat`, which kept the
/// minimum `(created_at, event_id)` and handed that one create to the evidence
/// check — so the answer was picked by the author's own clock and only then
/// verified. `created_at` is a claim nothing signs into agreement with the
/// wire: an earlier create no provider ever answered shadowed the seat that
/// was actually running, and anyone able to publish a 44221 for the role could
/// park the hire on a create that will never verify.
///
/// So no time is consulted beyond the caller's own `since` cutoff, which is
/// taken before the hire request is published and exists only to keep a
/// *previous* hire's seat out of the candidate set. The caller assesses every
/// candidate and lets the signed provider evidence decide — exactly what
/// [`founder_seated_creates_for_actor`] already does for `seat-repair`, and
/// for the same reasons written out there.
///
/// Returned in event-id order, which is a content hash and therefore neither
/// author-asserted nor dependent on the order the relay handed the events
/// over. One logical command is one candidate: a host that retries a submit
/// republishes the same `commandId` under a new event id, and that is our own
/// retry rather than two seats to choose between. The role is part of the
/// dedupe key because the same `commandId` republished with a *different* role
/// is a genuine contradiction about what was written.
pub fn hired_seat_candidates(
    events: &[Value],
    umbrella: &str,
    role: &str,
    since: i64,
) -> Vec<HiredSeat> {
    let mut found: Vec<HiredSeat> = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_COMMAND))
        {
            continue;
        }
        let (Some(content), Some(created_at), Some(event_id), Some(create_signer)) = (
            content_of(event),
            event_created_at(event),
            event_str(event, "id"),
            event_str(event, "pubkey"),
        ) else {
            continue;
        };
        if created_at < since {
            continue;
        }
        let Ok(payload) =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                content,
            )
        else {
            continue;
        };
        let CodingSessionLifecycleAction::SessionCreate {
            session_ref: Some(session_ref),
            provider_instance_ref,
            model,
            actor: Some(actor),
            role: Some(seat_role),
            genesis_ref: Some(genesis_ref),
            provider_authority_pubkey,
            ..
        } = payload.action
        else {
            continue;
        };
        if session_ref != umbrella || seat_role != role {
            continue;
        }
        if found.iter().any(|held| held.event_id == event_id) {
            continue;
        }
        found.push(HiredSeat {
            event_id,
            create_signer,
            command_id: payload.command_id,
            actor,
            role: seat_role,
            session_ref,
            genesis_ref,
            provider_authority_pubkey,
            provider_instance_ref,
            model,
            // Retained for display only. Nothing in the selection reads it.
            at: created_at,
            raw: event.clone(),
        });
    }
    // Grouped so the dedupe sees its duplicates adjacent, then re-sorted into
    // event-id order for the caller — deduped AFTER the event-id sort, so
    // which copy survives is deterministic and not time-derived.
    found.sort_by(|left, right| {
        left.command_id
            .cmp(&right.command_id)
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    found.dedup_by(|left, right| left.command_id == right.command_id && left.role == right.role);
    found.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    found
}

/// The refusal a hire prints when more than one seated create for its role
/// verifies against its own receipt.
///
/// Two verified creates are two signed facts that disagree about which seat
/// this hire got, and the CLI has nothing honest to break the tie with —
/// choosing by time is the very defect this replaced. So it refuses, names the
/// creates, and hands over the one command that can settle it deliberately.
///
/// Bounded: at most [`MAX_LISTED_HIRE_CANDIDATES`] create ids are named.
pub fn ambiguous_hire_refusal(
    channel: &str,
    session_ref: &str,
    candidates: &[&HiredSeat],
) -> String {
    let listed: Vec<&str> = candidates
        .iter()
        .take(MAX_LISTED_HIRE_CANDIDATES)
        .map(|seat| seat.event_id.as_str())
        .collect();
    let ids = if candidates.len() > MAX_LISTED_HIRE_CANDIDATES {
        format!(
            "{}, +{} more not listed",
            listed.join(", "),
            candidates.len() - MAX_LISTED_HIRE_CANDIDATES
        )
    } else {
        listed.join(", ")
    };
    let count = match candidates.len() {
        2 => "two".to_owned(),
        other => other.to_string(),
    };
    // One actor, or the honest plural. `seat-repair --actor` takes exactly one
    // pubkey, so a set of candidates naming different actors cannot be reduced
    // to a single remedy without picking for the operator — which is what this
    // refusal exists to avoid.
    let mut actors: Vec<&str> = candidates.iter().map(|seat| seat.actor.as_str()).collect();
    actors.sort_unstable();
    actors.dedup();
    let remedy = match actors.as_slice() {
        [only] => seat_repair_command(channel, session_ref, only),
        many => format!(
            "{} — these creates name different actors ({}), so repair the one you meant",
            seat_repair_command(channel, session_ref, "<actor>"),
            many.join(", ")
        ),
    };
    format!(
        "{count} seated creates for this role verify against their own receipts ({ids}); the \
         CLI will not choose between them — repair the seat you meant with {remedy}"
    )
}

/// The most seated creates one refusal sentence names before it truncates.
const MAX_LISTED_HIRE_CANDIDATES: usize = 8;

/// Every founder-signed seated create naming `actor` in `umbrella`.
///
/// The candidate set for a repair, and deliberately not a *choice* among them.
/// [`find_hired_seat`] can pick one because a hire knows the role it asked for
/// and the second it asked; a repair knows neither, and the two tie-breakers
/// that look obvious are both wrong:
///
/// - **Earliest `created_at` wins** — which this function used to do — lets a
///   benign earlier hire that no provider ever answered shadow the seat that is
///   actually running, and lets anyone who can publish a 44221 park the only
///   recovery path permanently. `created_at` is author-asserted; nothing signs
///   it into agreement with the wire.
/// - **Newest wins** has the mirror-image failure.
///
/// So no time is consulted at all. The caller gathers *every* candidate and
/// lets the signed provider evidence decide which one runs; when more than one
/// verifies, that is a real ambiguity and the caller refuses rather than
/// guessing. Only the founder's own creates are candidates, because
/// `verify_hire_evidence` will reject any other signer anyway
/// (`create.pubkey != genesis.pubkey`) and a stranger's create must not be able
/// to occupy a slot in the candidate set.
///
/// Returned in event-id order, which is a content hash and therefore neither
/// author-asserted nor dependent on the order the relay handed the events over.
pub fn founder_seated_creates_for_actor(
    events: &[Value],
    umbrella: &str,
    actor: &str,
    founder: &str,
) -> Vec<HiredSeat> {
    let mut found: Vec<HiredSeat> = Vec::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64)
            != Some(u64::from(KIND_CODING_SESSION_LIFECYCLE_COMMAND))
        {
            continue;
        }
        let (Some(content), Some(created_at), Some(event_id), Some(create_signer)) = (
            content_of(event),
            event_created_at(event),
            event_str(event, "id"),
            event_str(event, "pubkey"),
        ) else {
            continue;
        };
        if create_signer != founder {
            continue;
        }
        let Ok(payload) =
            buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
                content,
            )
        else {
            continue;
        };
        let CodingSessionLifecycleAction::SessionCreate {
            session_ref: Some(session_ref),
            provider_instance_ref,
            model,
            actor: Some(create_actor),
            role: Some(seat_role),
            genesis_ref: Some(genesis_ref),
            provider_authority_pubkey,
            ..
        } = payload.action
        else {
            continue;
        };
        if session_ref != umbrella || create_actor != actor {
            continue;
        }
        if found.iter().any(|held| held.event_id == event_id) {
            continue;
        }
        found.push(HiredSeat {
            event_id,
            create_signer,
            command_id: payload.command_id,
            actor: create_actor,
            role: seat_role,
            session_ref,
            genesis_ref,
            provider_authority_pubkey,
            provider_instance_ref,
            model,
            // Retained for display only. Nothing in the repair's selection
            // reads it, and nothing may start.
            at: created_at,
            raw: event.clone(),
        });
    }
    // Grouped first so the dedupe below sees its duplicates adjacent, then
    // re-sorted into event-id order for the caller.
    found.sort_by(|left, right| {
        left.command_id
            .cmp(&right.command_id)
            .then_with(|| left.role.cmp(&right.role))
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    // One logical command is one candidate. A founder that retries a submit
    // publishes the same commandId under a new event id and a new
    // `created_at`; showing the operator that commandId twice, or treating it
    // as two seats to choose between, is a bug about our own retry, not a fact
    // about the umbrella. Deduped AFTER the event-id sort, so which copy
    // survives is deterministic and not time-derived.
    //
    // The role is part of the key on purpose: the same commandId republished
    // with a DIFFERENT role is a genuine contradiction about what to write, and
    // collapsing it here would silently pick one of the two roles.
    found.dedup_by(|left, right| left.command_id == right.command_id && left.role == right.role);
    found.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    found
}

/// Every lifecycle (non-turn) receipt whose payload answers `command_id`.
///
/// Every lifecycle (non-turn) receipt answering `command_id`, in event-id
/// order.
///
/// Deliberately *not* "the newest by `created_at`". Selecting on an author's
/// own clock and checking the binding afterwards let anyone who could publish
/// a 44224 carrying the commandId deny both the repair and the hire; this
/// returns them all and leaves the choice to
/// [`super::hire_evidence::create_receipt_binding`] — signature first, time
/// never. Turn stages are excluded for the same reason [`newest_turn_stages`]
/// excludes lifecycle outcomes: a turn receipt names a generation but never
/// creates one, so it can never say whether a seat exists.
pub fn create_receipts_for_command(
    receipts: &[ReceiptRecord],
    command_id: &str,
) -> Vec<SeatReceipt> {
    let mut found: Vec<SeatReceipt> = Vec::new();
    for record in receipts {
        if record.is_turn_status {
            continue;
        }
        let Some(content) = content_of(&record.raw) else {
            continue;
        };
        let Ok(receipt) = serde_json::from_str::<LifecycleReceipt>(content) else {
            continue;
        };
        if receipt.command_id != command_id || receipt.status.is_turn_stage() {
            continue;
        }
        let Some(event_id) = event_str(&record.raw, "id") else {
            continue;
        };
        if found.iter().any(|held| held.event_id == event_id) {
            continue;
        }
        found.push(SeatReceipt {
            event_id,
            signer: record.signer.clone(),
            status: receipt.status,
            target_key: record.target_key.clone(),
            error_code: receipt.error.as_ref().map(|error| error.code.clone()),
            error_message: receipt.error.as_ref().map(|error| error.message.clone()),
            at: record.created_at,
            raw: record.raw.clone(),
        });
    }
    found.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    found
}

/// What a lead does next about one refusal code — the CLI's list of known
/// codes.
///
/// A refusal reaches a lead as a code and the host's own sentence about *this*
/// host. The sentence says what happened; this says what to do, and it is the
/// same answer every time, so it belongs to the CLI rather than to each host's
/// prose. `None` for a code this build has never heard of: a guessed remedy
/// for an unknown refusal is worse than none, and the host's own reason is
/// still printed either way.
///
/// Every code in
/// [`buzz_core::coding_session_lifecycle_command::HIRE_REFUSAL_CODES`] has an
/// entry here, pinned by a test — a code that ships in `buzz-core` alone would
/// reach a lead as a bare token with no way forward.
pub fn hire_refusal_remedy(code: &str) -> Option<&'static str> {
    Some(match code {
        "HIRE_OFF" => {
            "ask the operator to turn hiring on in Settings → Sessions → Hiring; do not retry \
             unchanged"
        }
        "HIRE_ROLE_NOT_ALLOWED" => {
            "hire one of the roles the reason lists, or ask the operator to allow this one"
        }
        "HIRE_LIMIT" => "end a seat in this session, or ask the operator to raise the ceiling",
        "HIRE_NO_IDENTITY" => {
            "ask the operator to install team roles on the Agents screen: this computer holds no \
             identity for that role"
        }
        // Not an install and not a retry: the seat the lead wanted is already
        // sitting in this umbrella, and the reason names it.
        "HIRE_ROLE_BUSY" => {
            "send your brief to the seat the reason names: bee sessions send --to <role>"
        }
        "HIRE_PROVIDER_NOT_ALLOWED" => {
            "name a provider instance the reason lists, or drop --provider-instance and take the \
             host's default"
        }
        "HIRE_MODEL_NOT_OFFERED" => {
            "re-run with a model id the reason lists, or drop --model and take the identity's own"
        }
        "HIRE_STALE" => "hire again: this request sat unanswered past the host's window",
        // Not a retry and not a downgrade. Nothing on offer clears the gate,
        // so the way forward is a different requirement or a stated override —
        // "the next model down" is exactly the answer the ruling forbids.
        "HIRE_NO_ROUTE" => {
            "nothing offered clears that class at that risk tier: hire a different class, \
             re-assess the risk, or override deliberately with --override-model and --because"
        }
        // The remedy names the failing key, because the whole point of this
        // code is that the host stops dropping a hire it cannot parse. Before
        // 2026-08-30 this refusal did not exist and the hire simply vanished
        // (ledger draft 97); the sentence the host writes carries the key,
        // and this is what the lead does about it.
        "HIRE_MALFORMED" => {
            "the hire's routing did not parse: <key> — run `bee sessions route` and hire again \
             with the request shape (`bee sessions hire --help`)"
        }
        _ => return None,
    })
}

/// Split a host's refusal turn into its code and reason.
///
/// The shape is fixed — `hire refused: <CODE> — <reason>` — so a refusal is
/// recognized structurally rather than by reading prose. The code must be
/// `[A-Z0-9_]+` and a reason must follow, so an agent merely *talking* about a
/// refused hire is not mistaken for one. Both an em dash and a hyphen separate
/// the two, because a host that normalizes punctuation should not break this.
pub fn parse_hire_refusal(text: &str) -> Option<(String, String)> {
    let rest =
        text.strip_prefix(buzz_core::coding_session_lifecycle_command::HIRE_REFUSAL_PREFIX)?;
    let (code, reason) = rest.split_once(" — ").or_else(|| rest.split_once(" - "))?;
    let code = code.trim();
    let reason = reason.trim();
    if code.is_empty()
        || reason.is_empty()
        || !code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    {
        return None;
    }
    Some((code.to_owned(), reason.to_owned()))
}

/// Find the refusal turn answering a hire into `umbrella`.
///
/// Scoped two ways, because a channel can carry several umbrellas at once: the
/// turn must address an execution this channel says belongs to `umbrella`, and
/// it must have been published after the request went out. Earliest qualifying
/// turn wins.
pub fn find_hire_refusal(
    commands: &[TurnCommand],
    executions: &[CrewExecution],
    umbrella: &str,
    since: i64,
) -> Option<HireRefusal> {
    let in_umbrella: HashSet<&str> = executions
        .iter()
        .filter(|execution| execution.session_ref.as_deref() == Some(umbrella))
        .map(|execution| execution.target_key.as_str())
        .collect();
    let mut best: Option<HireRefusal> = None;
    for command in commands {
        if command.created_at < since || !in_umbrella.contains(command.target_key.as_str()) {
            continue;
        }
        let Some((code, reason)) = command.text().and_then(parse_hire_refusal) else {
            continue;
        };
        let candidate = HireRefusal {
            event_id: command.event_id.clone(),
            at: command.created_at,
            code,
            reason,
        };
        match &best {
            Some(held)
                if (held.at, held.event_id.as_str())
                    <= (candidate.at, candidate.event_id.as_str()) => {}
            _ => best = Some(candidate),
        }
    }
    best
}

/// Fold what the channel said into one outcome.
///
/// A published seat outranks a refusal: if both are somehow present, something
/// was actually created and reporting "refused" would be false. Pure so the
/// wording of an unpleasant answer is testable without a relay.
///
/// `granted` is whether the seat's exact actor-role pair holds a live,
/// receipt-backed `grant-seat` on the umbrella's authority chain. It does not
/// change what happened — the seat is created either way — only whether the
/// hire is usable by the typed team workflow.
pub fn fold_hire(
    seat: Option<HiredSeat>,
    receipt: Option<SeatReceipt>,
    refusal: Option<HireRefusal>,
    granted: bool,
) -> HireOutcome {
    match (seat, refusal) {
        (Some(seat), _) => match receipt {
            Some(receipt)
                if matches!(
                    receipt.status,
                    ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
                ) =>
            {
                HireOutcome::Created {
                    seat,
                    receipt,
                    granted,
                }
            }
            Some(receipt) => HireOutcome::Failed { seat, receipt },
            None => HireOutcome::Seating { seat },
        },
        (None, Some(refusal)) => HireOutcome::Refused(refusal),
        (None, None) => HireOutcome::Unconfirmed,
    }
}

/// One hire outcome, in the two forms a caller reads: a status word for a
/// script and a sentence for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HireReport {
    /// `created`, `failed`, `seating`, `refused`, or `unconfirmed`.
    pub status: &'static str,
    /// One sentence naming what happened, in the requester's terms.
    pub detail: String,
}

/// Render a hire outcome.
///
/// `waited` distinguishes the two ways of having no answer — nobody answered,
/// and nobody was asked — so `--no-wait` never reads as a silent host.
///
/// `channel` is carried only so the two outcomes that leave a seat without
/// accepted role authority can print the whole `seat-repair` command rather
/// than name it.
pub fn hire_report(outcome: &HireOutcome, waited: bool, channel: &str) -> HireReport {
    match outcome {
        HireOutcome::Created {
            seat,
            receipt,
            granted,
        } => HireReport {
            status: "created",
            // Seated and granted are two facts, and the second is the one that
            // decides whether an answer is ever coming back. Said in the same
            // sentence so `created` is never read as "and it can report".
            detail: format!(
                "the host seated {} as {} on {}{}. {}",
                short_pubkey(&seat.actor),
                seat.role,
                seat.provider_instance_ref,
                match receipt.target_key.as_deref() {
                    Some(target) => format!(" — {target}"),
                    None => String::new(),
                },
                if *granted {
                    "Its accepted role-seat authority is active, so it can report back to you"
                        .to_owned()
                } else {
                    // Exactly what is true: the report is canonical and the
                    // seat is not. Saying the fold drops the report — which
                    // this sentence used to say — sent leads looking for a
                    // missing event that was there all along.
                    format!(
                        "It is seated, but its exact actor-role grant is not accepted. The typed \
                         team fold still INCLUDES its report, by assignee identity, and \
                         discloses it under `unseatedReports` as carrying no seat authority; an \
                         ungranted seat cannot hold verifier authority, so it can never refute, \
                         and it is absent from `activeSeats`. {}",
                        seat_repair_remedy(channel, &seat.session_ref, &seat.actor)
                    )
                }
            ),
        },
        HireOutcome::Failed { seat, receipt } => HireReport {
            status: "failed",
            detail: format!(
                "the host published a seat for {} and the provider refused it: {}{}",
                seat.role,
                receipt
                    .error_code
                    .as_deref()
                    .unwrap_or(receipt.status.as_str()),
                match receipt.error_message.as_deref() {
                    Some(message) => format!(" — {message}"),
                    None => String::new(),
                }
            ),
        },
        HireOutcome::Seating { seat } => HireReport {
            status: "seating",
            detail: format!(
                "the host published a seat for {} ({}), but no provider receipt answered it \
                 within {HIRE_WAIT_SECONDS}s — read `bee sessions status` for the umbrella. \
                 The repair below needs a provider receipt, so it answers `no_receipt_yet` \
                 until one lands. {}",
                seat.role,
                seat.command_id,
                seat_repair_remedy(channel, &seat.session_ref, &seat.actor)
            ),
        },
        HireOutcome::Ambiguous { message } => HireReport {
            status: "ambiguous",
            detail: message.clone(),
        },
        HireOutcome::Refused(refusal) => HireReport {
            status: "refused",
            // The host's own words first, then what to do about them. An
            // unknown code prints the host's sentence alone rather than a
            // remedy this build invented for it.
            detail: format!(
                "hire refused: {} — {}{}",
                refusal.code,
                refusal.reason,
                match hire_refusal_remedy(&refusal.code) {
                    Some(remedy) => format!(" Next: {remedy}."),
                    None => String::new(),
                }
            ),
        },
        HireOutcome::Unconfirmed if waited => HireReport {
            status: DELIVERY_UNCONFIRMED,
            detail: format!(
                "the relay stored the hire and nothing answered it within \
                 {HIRE_WAIT_SECONDS}s — no seat was created and no refusal was published; \
                 the founder's host may be offline"
            ),
        },
        HireOutcome::Unconfirmed => HireReport {
            status: DELIVERY_UNCONFIRMED,
            detail: "the relay stored the hire; --no-wait means nothing was asked what became \
                     of it"
                .to_owned(),
        },
    }
}

/// The process exit code one hire outcome earns.
///
/// `0` created with accepted role-seat authority, `1` refused, failed,
/// ambiguous, or created without authority, `5` unconfirmed — including a seat
/// published but never answered, which is not a success.
pub fn hire_exit_code(outcome: &HireOutcome) -> i32 {
    match outcome {
        HireOutcome::Created { granted: true, .. } => 0,
        HireOutcome::Created { granted: false, .. } => 1,
        HireOutcome::Failed { .. } | HireOutcome::Refused(_) | HireOutcome::Ambiguous { .. } => 1,
        HireOutcome::Seating { .. } | HireOutcome::Unconfirmed => 5,
    }
}

/// What one `bee sessions seat-repair` run found, and whether it wrote.
///
/// Four words, three of them terminal facts about evidence rather than about
/// the write: a repair that does not grant has to say *why* it did not, and
/// "nothing happened" is three different situations with three different next
/// steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeatRepairOutcome {
    /// A new accepted `grant-seat` now names this exact actor-role pair.
    Granted,
    /// More than one founder-signed create for this actor carries
    /// provider-signed proof of a running execution, so which seat the
    /// operator means is genuinely unknown. Nothing was written: a tie-breaker
    /// here would grant authority to an execution nobody named.
    Ambiguous,
    /// The pair already held an accepted seat. Nothing was written; running
    /// this command twice is deliberately a no-op.
    AlreadyGranted,
    /// The seated create exists and no provider lifecycle receipt answers it.
    /// Nothing was written: there is no proof yet that anything runs.
    NoReceiptYet,
    /// The receipt refused the create, the signed evidence did not verify, or
    /// the actor holds a different role. Nothing was written.
    Refused,
}

impl SeatRepairOutcome {
    /// The machine-readable word printed as `outcome`.
    pub fn as_str(self) -> &'static str {
        match self {
            SeatRepairOutcome::Granted => "granted",
            SeatRepairOutcome::Ambiguous => "ambiguous",
            SeatRepairOutcome::AlreadyGranted => "already_granted",
            SeatRepairOutcome::NoReceiptYet => "no_receipt_yet",
            SeatRepairOutcome::Refused => "refused",
        }
    }
}

/// The process exit code one `seat-repair` outcome earns.
///
/// `0` for both granted words — the point of the command is that the seat
/// holds its role afterwards, and it held it either way. `1` refused, in line
/// with every other CLI refusal. `5` for `no_receipt_yet`, the same code
/// `hire` gives an unanswered create: the repair is unfinished rather than
/// wrong, and a script should come back rather than escalate.
pub fn seat_repair_exit_code(outcome: SeatRepairOutcome) -> i32 {
    match outcome {
        SeatRepairOutcome::Granted | SeatRepairOutcome::AlreadyGranted => 0,
        SeatRepairOutcome::Refused | SeatRepairOutcome::Ambiguous => 1,
        SeatRepairOutcome::NoReceiptYet => 5,
    }
}

/// Build the exact `session.hire` payload `bee sessions hire` signs.
///
/// Separate from the command so the shape is asserted byte-for-byte in a unit
/// test: the relay validates 44221 with `deny_unknown_fields`, so a key more or
/// a key fewer is not a lint, it is a rejected request.
///
/// `routing` is the one additive key (Brian's ruling of 2026-08-30), and it is
/// a [`HireRoutingRequest`](buzz_core::coding_session_routing::HireRoutingRequest)
/// — the *question* — not the routing record. The host routes, because only
/// the host can see its own live kind:44222 catalog. Emitting the record here
/// is exactly the 2026-08-30 09:52 failure: the desktop host's parser accepted
/// only the request, so a hire the relay had accepted was classified malformed
/// and dropped with no answer at all (ledger draft 97).
///
/// It is omitted entirely — never written as an explicit `null` — when nothing
/// routed, so an unrouted hire is byte-identical to the seven-key form that
/// shipped before the router existed.
#[allow(clippy::too_many_arguments)]
pub fn hire_payload(
    command_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    role: &str,
    provider_instance_ref: Option<ProviderInstanceAlias>,
    model: Option<&str>,
    brief: &str,
    requested_by: Option<&str>,
    routing: Option<buzz_core::coding_session_routing::HireRoutingRequest>,
) -> buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleCommandPayload {
    buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleCommandPayload {
        schema:
            buzz_core::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA
                .to_owned(),
        command_id: command_id.to_owned(),
        action: CodingSessionLifecycleAction::SessionHire {
            session_ref: session_ref.to_owned(),
            genesis_ref: genesis_ref.to_owned(),
            role: role.to_owned(),
            provider_instance_ref,
            model: model.map(str::to_owned),
            brief: brief.to_owned(),
            requested_by: requested_by.map(str::to_owned),
            routing,
        },
    }
}

/// Recognize a relay that predates `session.hire`, and say so in those words.
///
/// The relay validates 44221 with `deny_unknown_fields` and a closed action
/// vocabulary, so a relay built before this action refuses a hire as a *shape*
/// error. Passing that through unchanged would tell a lead its request was
/// malformed when the request is fine and the relay is old. The relay's own
/// sentence is kept on the end rather than replaced — an operator upgrading a
/// relay needs it.
pub fn hire_unsupported_by_relay(message: &str) -> Option<String> {
    const SHAPE_ERRORS: &[&str] = &[
        "action type is unsupported",
        "action has missing or unsupported fields",
        "malformed coding-session lifecycle command payload",
    ];
    if !SHAPE_ERRORS.iter().any(|needle| message.contains(needle)) {
        return None;
    }
    Some(format!(
        "this relay does not accept hire requests yet — it validates kind 44221 against a \
         closed action list that has no `session.hire` in it, so the request never reached \
         the founder's host. Upgrade the relay, or seat the role from the desktop. The relay \
         said: {message}"
    ))
}

/// Resolve the genesis event id founding `umbrella` from a channel's events.
///
/// A lead names an umbrella by its UUID; the wire needs the genesis *event
/// id*, and the channel already carries it. Two geneses claiming one label is
/// a contradiction NIP-CSG explicitly allows the relay to store, so it is
/// reported with both ids rather than resolved by picking one.
pub fn resolve_umbrella_genesis(events: &[Value], umbrella: &str) -> Result<String, CliError> {
    let mut found: BTreeSet<String> = BTreeSet::new();
    for event in events {
        if event.get("kind").and_then(Value::as_u64) != Some(u64::from(KIND_CODING_SESSION_GENESIS))
        {
            continue;
        }
        let (Some(content), Some(event_id)) = (content_of(event), event_str(event, "id")) else {
            continue;
        };
        let Ok(payload) = decode_coding_session_genesis(content) else {
            continue;
        };
        if payload.session_ref == umbrella {
            found.insert(event_id);
        }
    }
    let mut found = found.into_iter();
    let Some(first) = found.next() else {
        return Err(CliError::NotFound(format!(
            "no coding-session genesis in this channel founds umbrella {umbrella} — a hire is \
             authorized against the genesis, so pass --genesis with the founding event id, or \
             check the channel"
        )));
    };
    let rest: Vec<String> = found.collect();
    if rest.is_empty() {
        return Ok(first);
    }
    let mut all = vec![first];
    all.extend(rest);
    Err(CliError::Usage(format!(
        "{} geneses in this channel claim umbrella {umbrella}: {}. Pass --genesis to name the \
         one this hire is authorized against.",
        all.len(),
        all.join(", ")
    )))
}

/// What one hire wait is holding, and why the last signed answer it saw did
/// not verify.
///
/// The second half is the point. Before this existed, a `Created` answer whose
/// evidence failed to bind was dropped by a bare `Err(_) => {}` arm and the
/// wait ran to its deadline still holding `Seating` — so on 2026-08-31 and
/// 2026-09-01 an alias defect in the receipt comparison was read twice as "the
/// host was slow", and the receipt it rejected had in fact arrived two seconds
/// after the create (`docs/SESSION_STATE.md` item 103, finding 2). A binding
/// defect must never again be indistinguishable from a slow provider, so the
/// last verification error is carried out of the wait and printed.
#[derive(Debug, Default)]
pub struct HireWait {
    held: Option<HireOutcome>,
    last_evidence_error: Option<String>,
    unbound_receipts: usize,
}

/// Everything one hire wait ended holding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HireWaitOutcome {
    /// The answer, or the best partial fact the wait is holding.
    pub outcome: HireOutcome,
    /// The last verification failure the wait saw, if any.
    pub last_evidence_error: Option<String>,
    /// The most receipts the wait ever saw naming the create's commandId
    /// without being bound to it. Reported, never hidden.
    pub unbound_receipts: usize,
}

impl HireWait {
    /// A wait holding nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one poll's answer into the wait.
    ///
    /// `verification` carries the evidence check for a `Created` answer and is
    /// `None` for every other outcome — a `Created` passed with `None` was
    /// never checked, so it is held rather than returned, and says so. Returns
    /// `Some` with the outcome the wait should return immediately, or `None`
    /// to keep polling.
    pub fn observe(
        &mut self,
        outcome: HireOutcome,
        verification: Option<Result<(), CliError>>,
    ) -> Option<HireOutcome> {
        match outcome {
            // A seat with no receipt yet is progress, not an answer: hold it
            // and keep waiting for the provider to speak.
            HireOutcome::Seating { .. } => {
                self.held = Some(outcome);
                None
            }
            HireOutcome::Unconfirmed => None,
            HireOutcome::Created { .. } => match verification {
                Some(Ok(())) => Some(outcome),
                // A signed create and receipt can precede the provider's
                // metadata by a moment. Preserve the live seat and keep
                // polling; if metadata never arrives, the caller reports the
                // explicit partial outcome — now with the reason attached.
                Some(Err(error @ CliError::Unconfirmed(_))) => {
                    self.last_evidence_error = Some(error.to_string());
                    self.held = Some(outcome);
                    None
                }
                // Malformed, forged, or cross-context facts are not a hire
                // answer and never reach the authority writer. They are still
                // recorded: this is the arm that hid the alias defect.
                Some(Err(error)) => {
                    self.last_evidence_error = Some(error.to_string());
                    None
                }
                None => {
                    self.last_evidence_error =
                        Some("hire evidence was not checked for this answer".to_owned());
                    self.held = Some(outcome);
                    None
                }
            },
            // An ambiguity is an answer — the wrong kind, and one no later
            // poll can improve: another verifying create only makes it worse.
            HireOutcome::Failed { .. }
            | HireOutcome::Refused(_)
            | HireOutcome::Ambiguous { .. } => Some(outcome),
        }
    }

    /// The last verification failure seen, if any.
    pub fn last_evidence_error(&self) -> Option<&str> {
        self.last_evidence_error.as_deref()
    }

    /// The outcome the wait ends on when its deadline passes.
    pub fn held(&self) -> HireOutcome {
        self.held.clone().unwrap_or(HireOutcome::Unconfirmed)
    }

    /// Record what one poll's evidence selection discarded.
    ///
    /// `rejection` overwrites: the *last* refusal is the one that explains the
    /// state the wait ends in. `unbound` is kept at its high-water mark — a
    /// later poll that happened to see fewer must not erase the fact.
    pub fn note(&mut self, rejection: Option<String>, unbound: usize) {
        if let Some(rejection) = rejection {
            self.last_evidence_error = Some(rejection);
        }
        self.unbound_receipts = self.unbound_receipts.max(unbound);
    }

    /// Close the wait on `outcome`, carrying everything it learned.
    pub fn finish(&self, outcome: HireOutcome) -> HireWaitOutcome {
        HireWaitOutcome {
            outcome,
            last_evidence_error: self.last_evidence_error().map(str::to_owned),
            unbound_receipts: self.unbound_receipts,
        }
    }
}

/// The sentence a hire report adds about evidence it refused on the way.
///
/// `answered` is whether the hire ended on the host's own verified answer. It
/// changes the tense and nothing else: a refusal that a later poll overtook is
/// history, and printing it in the present tense on a `created` hire reads as
/// a failure that did not happen (REVIEW-A1 F7). On a hire that ended holding
/// something, the same fact is the reason it is holding — the sentence whose
/// absence let an alias defect read as a slow host for two nights
/// (`docs/SESSION_STATE.md` item 103, finding 2).
///
/// Returns an empty string when the wait refused nothing and saw no unbound
/// receipt: there is no fact, so there is no sentence.
pub fn evidence_disclosure(
    answered: bool,
    last_evidence_error: Option<&str>,
    unbound_receipts: usize,
) -> String {
    let mut sentences = Vec::new();
    if let Some(error) = last_evidence_error {
        // "Earlier" was a time word, and after T2.5 the selection order is
        // event id rather than time; the error also now names its own create,
        // so the sentence does not have to imply one (REVIEW-L1 F3).
        sentences.push(if answered {
            format!("A candidate receipt was refused before this answer verified: {error}")
        } else {
            format!("The last hire evidence check refused the host's answer: {error}")
        });
    }
    if unbound_receipts > 0 {
        // Not "this create's": the count spans every seated create assessed
        // for this role, which after T2.5 can be more than one (REVIEW-L1 F3).
        sentences.push(format!(
            "{unbound_receipts} receipt(s) named a seated create's commandId for this role \
             without being bound to it and were ignored."
        ));
    }
    sentences.join(" ")
}

/// Everything `bee sessions hire --check` was asked to validate.
///
/// Borrowed rather than owned so the caller can build it from the exact values
/// it would have signed: a check that re-derived its own inputs would not be
/// checking the hire.
pub struct HireCheckRequest<'a> {
    /// Channel UUID the umbrella lives in.
    pub channel: &'a str,
    /// Umbrella session reference.
    pub session_ref: &'a str,
    /// Genesis the hire joins, after resolution.
    pub genesis_ref: &'a str,
    /// Role slug the hire would seat.
    pub role: &'a str,
    /// The brief exactly as it would be signed.
    pub brief: &'a str,
    /// Provider instance the hire names, or `None` for the host's default.
    pub provider_instance: Option<&'a str>,
    /// Model the hire names, or `None`.
    pub model: Option<&'a str>,
    /// The routing request the hire would carry, or `None` when unrouted.
    pub routing: Option<&'a buzz_core::coding_session_routing::HireRoutingRequest>,
    /// Why the local router could not answer, when it could not.
    pub proposal_unavailable: Option<&'a str>,
}

/// The document `bee sessions hire --check` prints.
///
/// Every number is measured, never estimated: `briefBytes` is the UTF-8 length
/// of the exact string that would be signed and `briefCapBytes` is the relay's
/// own ceiling for a *brief* ([`MAX_LIFECYCLE_HIRE_BRIEF_BYTES`]) — the
/// initial-turn ceiling minus the host's `"[From the lead] "` prefix, which is
/// what the decoder refuses against, not the wider initial-turn ceiling.
/// `published` is present
/// and `false` so a reader — or a grep over a seat's transcript — can tell a
/// check from a hire without knowing which flags were passed. An acceptance
/// test that publishes a live 44221 is not an acceptance test; one ran on
/// 2026-09-01 (`docs/SESSION_STATE.md` item 103, finding 10).
pub fn hire_check_report(request: &HireCheckRequest<'_>) -> Value {
    let brief_bytes = request.brief.len();
    serde_json::json!({
        "check": true,
        "published": false,
        "channel": request.channel,
        "sessionRef": request.session_ref,
        "genesisRef": request.genesis_ref,
        "role": request.role,
        "briefBytes": brief_bytes,
        "briefCapBytes": MAX_LIFECYCLE_HIRE_BRIEF_BYTES,
        "briefWithinCap": brief_bytes <= MAX_LIFECYCLE_HIRE_BRIEF_BYTES,
        "providerInstanceRef": request.provider_instance,
        "model": request.model,
        "routing": request
            .routing
            .and_then(|routing| serde_json::to_value(routing).ok()),
        "routed": request.routing.is_some(),
        "proposedUnavailable": request.proposal_unavailable,
    })
}
