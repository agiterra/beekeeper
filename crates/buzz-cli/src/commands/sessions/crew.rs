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
use buzz_core::coding_session_lease::{decode_coding_session_lease, CodingSessionLeaseState};
use buzz_core::coding_session_lifecycle_command::CodingSessionLifecycleAction;
use buzz_core::coding_session_payload::{
    LifecycleReceipt, ReceiptStatus, SessionStatus, NO_LIVE_EXECUTION, STALE_GENERATION,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_COMMAND, KIND_CODING_SESSION_LEASE, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
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
    /// Runtime slug behind the driver, when metadata named one.
    pub runtime: Option<String>,
    /// `event_seq` of the newest transcript item this generation signed.
    pub last_signed_seq: Option<u64>,
    /// `created_at` of that item, in Unix seconds.
    pub last_signed_at: Option<i64>,
    /// Derived liveness — see [`Liveness`].
    pub liveness: Liveness,
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
            _ => match (self.runtime.as_deref(), self.model.as_deref()) {
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
    resolve_sessions(metadata, receipts, transcripts)
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
            }
        })
        .collect()
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
    /// Turn text, or `None` for `thread.turn.interrupt`, which carries none.
    pub fn text(&self) -> Option<&str> {
        match &self.action {
            CodingSessionAction::ThreadTurnStart { text, .. } => Some(text),
            CodingSessionAction::ThreadTurnInterrupt => None,
        }
    }

    /// Requested delivery class; `boundary` for an interrupt's own command,
    /// which is a class of its own on the wire.
    pub fn deliver(&self) -> CodingSessionDelivery {
        match &self.action {
            CodingSessionAction::ThreadTurnStart { deliver, .. } => *deliver,
            CodingSessionAction::ThreadTurnInterrupt => CodingSessionDelivery::Interrupt,
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
fn readdressable_reason(stage: &TurnStage) -> Option<String> {
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
        CodingSessionAction::ThreadTurnStart { text, deliver } => (text.clone(), *deliver),
        CodingSessionAction::ThreadTurnInterrupt => {
            return Err(CliError::Usage(format!(
                "command '{command_id}' is a thread.turn.interrupt — \
                 it carries no text to re-address"
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
