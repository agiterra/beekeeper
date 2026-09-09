//! `bee sessions handover` — continue another participant's work.
//!
//! Four verbs over two mechanisms that already exist. A **checkpoint**
//! (kind 44247) says what the work is, durably enough to be picked up. A
//! **claim** is one more link on the session's accepted 44228 chain, so the
//! relay serializes racing claims and every consumer folds the same answer. A
//! **continuation** (44247 again) records what the claimant then did, in one
//! of two labelled ways: `native-resume` on the original provider, or
//! `reconstructed` as a new execution joining the same umbrella. **Status**
//! prints the fold.
//!
//! # Whole-session scope
//!
//! v1 hands over the *whole session*: the claim is rooted at the genesis, so
//! one claim moves every execution and every assignment under the umbrella and
//! fences the siblings alongside the absent one
//! (`docs/HANDOVER_IMPL.md` §1, §9). Every command here repeats
//! [`handover_render::WHOLE_SESSION_DISCLOSURE`] rather than leaving a reader
//! to discover the scope from behaviour.
//!
//! # What each command prints
//!
//! Every verb ends with a [`VerificationNotes`] block: what this run
//! established, and what it did not. That second list is the point. A
//! reconstruction that could not read the relay's 30618 ref state, a claim
//! whose receipt did not arrive inside the wait, a checkpoint whose push
//! failed — each is a fact the caller needs, and none of them is an error that
//! should stop the rest of the work.
//!
//! # One builder, not two
//!
//! Every 44247 this command publishes goes through
//! `buzz_sdk::coding_session_handover::build_coding_session_handover`, and
//! every 44228 claim through `build_coding_session_takeover`. The SDK builders
//! are what the relay's validators were written against, so the CLI cannot
//! drift into a tag order, a schema string or a bound the relay would refuse —
//! a second hand-rolled envelope here would be exactly that drift waiting to
//! happen.

use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_handover::{
    CodingSessionHandoverBody, CodingSessionHandoverPayload, CODING_SESSION_HANDOVER_SCHEMA,
};
use buzz_core::coding_session_handover_fold::{
    fold_coding_session_handover, HandoverCheckpointEntry, HandoverFold, HandoverFoldContext,
    HandoverStanding,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_HANDOVER, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_TRANSCRIPT, KIND_SYSTEM_MESSAGE,
};
use buzz_sdk::coding_session_handover::build_coding_session_handover;
use nostr::Event;
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{sdk_err, validate_lower_hex64, validate_uuid};
use crate::HandoverCmd;

use super::crew::{build_executions, decode_leases, short_pubkey, CrewExecution, Liveness};
use super::handover_render::{
    artifact_line, preserved_word, VerificationNotes, WHOLE_SESSION_DISCLOSURE,
};
use super::operations_authority::fetch_trusted_relay_self;
use super::operations_reads::{fetch_session_authority, SessionAuthority};

/// The relay receipt type a provider and this CLI both read as proof that a
/// whole-session deletion was applied (`docs/HANDOVER_IMPL.md` §3.2).
const DELETION_ACCEPTANCE_RECEIPT_TYPE: &str = "coding_session_deletion_accepted";

/// What this CLI says when the relay returns no genesis and no deletion
/// receipt explains it.
///
/// The sentence claims exactly one thing — that the read came back empty — and
/// nothing about why. Absence is never deletion authority (§3.2): a relay that
/// is behind, partitioned, or scoping the read away produces this same empty
/// answer, and every one of those is a reason to stop rather than a reason to
/// declare the session gone.
pub(super) const GENESIS_UNAVAILABLE_REASON: &str =
    "the relay returned no genesis for this session; authority cannot be verified";

/// The durable kinds an execution row is folded from.
///
/// The same set `bee sessions status` uses, minus the genesis: this reader
/// already resolved the founder from an exact-id genesis read before it got
/// here.
const EXECUTION_FACT_KINDS: &[u32] = &[
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_TRANSCRIPT,
];

/// Everything the four verbs read before any of them acts.
pub(super) struct HandoverState {
    /// Canonical lowercase channel UUID.
    pub(super) channel: String,
    /// Canonical lowercase umbrella session UUID.
    pub(super) session_ref: String,
    /// The umbrella's immutable genesis event id.
    pub(super) genesis_ref: String,
    /// The genesis signer.
    pub(super) founder: String,
    /// The accepted authority chain, including the claim fold.
    ///
    /// `None` when there is no chain left to project — a retired umbrella, or
    /// one whose genesis the relay would not return. Pretending otherwise
    /// would invent a chain.
    pub(super) authority: Option<SessionAuthority>,
    /// The 44247 fold.
    pub(super) fold: HandoverFold,
    /// Why this umbrella reads as retired, when it does.
    ///
    /// **Only a relay-signed deletion receipt sets this.** An absent genesis
    /// does not: see [`Self::genesis_unavailable`].
    pub(super) retirement: Option<String>,
    /// Why this session's authority could not be read at all, when it could
    /// not.
    ///
    /// Kept strictly apart from [`Self::retirement`], because absence is not
    /// deletion authority. A relay that returns no genesis row may have
    /// applied a deletion, or may be behind, or may be refusing the read, and
    /// a CLI that called all three "deleted" would publish a deletion nobody
    /// performed — the exact failure §3.2 rules out by requiring a signed
    /// receipt. So this is its own state, it refuses `claim` and `continue`
    /// fail-closed, and it says only what it knows.
    pub(super) genesis_unavailable: Option<String>,
}

impl HandoverState {
    /// The claim in force, or `NoClaim`/`Voided` exactly as folded.
    pub(super) fn claim(&self) -> ClaimState {
        self.fold.claim.clone()
    }

    /// Whether `pubkey` may act on this umbrella at all: the founder, or a
    /// live steering grant.
    ///
    /// A seat is deliberately not enough. A seat may **write** a checkpoint —
    /// it is describing its own work — but claiming a session is the act §1
    /// reserves for the founder and live operators.
    pub(super) fn has_standing(&self, pubkey: &str) -> bool {
        if pubkey == self.founder {
            return true;
        }
        self.authority.as_ref().is_some_and(|authority| {
            authority
                .grants
                .iter()
                .any(|(granted, _)| granted == pubkey)
        })
    }

    /// The one sentence `status` prints about this session's existence.
    ///
    /// Three outcomes that a reader must not confuse, so they are produced by
    /// one function rather than assembled at each call site.
    pub(super) fn existence_line(&self) -> String {
        match (&self.retirement, &self.genesis_unavailable) {
            (Some(reason), _) => format!("retired: yes — {reason}"),
            (None, Some(_)) => format!(
                "retired: not proven — {GENESIS_UNAVAILABLE_REASON}. Genesis not readable on the \
                 relay (not proven deleted)."
            ),
            (None, None) => "retired: no".to_owned(),
        }
    }

    /// The newest checkpoint a reconstruction starts from.
    ///
    /// Straight from the fold, which is where the rule lives: a checkpoint a
    /// later one by the same author replaces is folded as
    /// [`HandoverStanding::Superseded`], so `latest_authorized_checkpoint`
    /// already skips it. This lane briefly carried its own copy of that rule
    /// and that was one answer too many — the fold is the single place the
    /// provider, the CLI and the Desktop twin all read it from.
    pub(super) fn latest_authorized_checkpoint(&self) -> Option<&HandoverCheckpointEntry> {
        let id = self.fold.latest_authorized_checkpoint.as_deref()?;
        self.fold
            .checkpoints
            .iter()
            .find(|entry| entry.event_id == id)
    }

    /// The newest still-standing checkpoint **by one author** — what that
    /// author's next checkpoint names as its `prevCheckpointRef`.
    ///
    /// A different question from the one above, which is why it is asked here:
    /// the fold answers "what does this umbrella's newest statement say", and
    /// a writer needs "what did I last say". Supersession is not recomputed —
    /// an entry the fold marked `Superseded` is skipped because the fold
    /// marked it.
    pub(super) fn latest_checkpoint_by(&self, author: &str) -> Option<&HandoverCheckpointEntry> {
        self.fold
            .checkpoints
            .iter()
            .rfind(|entry| entry.author == author && entry.standing == HandoverStanding::Authorized)
    }

    /// Every checkpoint a later one by the same author replaced, with its
    /// replacement.
    pub(super) fn superseded_checkpoints(&self) -> Vec<&HandoverCheckpointEntry> {
        self.fold
            .checkpoints
            .iter()
            .filter(|entry| entry.standing == HandoverStanding::Superseded)
            .collect()
    }
}

/// What the relay said about a write whose event id **this process computed**.
///
/// A nostr event id is the hash of its own content, so "the relay already has
/// this id" means the relay already has these exact bytes — ours. That is a
/// success, and treating it as a failure is what made a repeat checkpoint of
/// an unchanged tree within one second drop its artifact and publish
/// `preserved: "none"` while the identical patch sat on the relay
/// (composition run 5, finding 2).
///
/// Scoped to ids the caller computed for content-addressed kinds. It is **not**
/// a general "duplicates are fine" rule: kind 44226's duplicate answer names a
/// *rival* genesis that won, which is a different event entirely, and this
/// helper is never used there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum OwnWriteOutcome {
    /// The relay stored these bytes on this call.
    Published,
    /// The relay already held them, under the id this process computed.
    AlreadyPresent,
}

/// Classify the relay's answer to a write of our own content-addressed event.
///
/// # Errors
/// Every refusal that is not a duplicate, in the relay's own words.
pub(super) fn classify_own_write(raw: &str) -> Result<OwnWriteOutcome, CliError> {
    let response: Value = serde_json::from_str(raw)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error} ({raw})")))?;
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // The relay answers a stored duplicate `{accepted: true, message:
    // "duplicate:"}`; some paths answer `accepted: false` with the same
    // prefix. Both mean the same thing about an id we computed, so the prefix
    // decides and the flag does not.
    if message == "duplicate" || message.starts_with("duplicate:") {
        return Ok(OwnWriteOutcome::AlreadyPresent);
    }
    if !response
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(CliError::Other(format!("relay rejected event: {message}")));
    }
    Ok(OwnWriteOutcome::Published)
}

/// Read the genesis, the accepted chain and every 44247 record for one
/// umbrella, and fold them.
///
/// Retirement is decided first and beats everything: a deleted umbrella yields
/// no claim, no authorized checkpoint and no continuation, because nothing is
/// reconstructed to make a deleted session resumable (§3.2). Retirement means
/// a **relay-signed deletion receipt** and nothing else — an empty genesis
/// read is reported as [`GENESIS_UNAVAILABLE_REASON`], which is a different
/// state with a different exit code.
pub(super) async fn load_handover_state(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
) -> Result<HandoverState, CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    let channel = channel.to_ascii_lowercase();
    let session_ref = session_ref.to_ascii_lowercase();
    if let Some(genesis) = genesis {
        validate_lower_hex64("--genesis", genesis)?;
    }
    let genesis_ref = resolve_genesis(client, &channel, &session_ref, genesis).await?;

    let genesis_rows = client
        .query_all(json!({
            "ids": [genesis_ref],
            "kinds": [KIND_CODING_SESSION_GENESIS],
            "#h": [channel],
        }))
        .await?;
    let genesis_event: Option<Event> = genesis_rows
        .first()
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok());
    // Retirement is proven by a relay-signed receipt and by nothing else. An
    // absent genesis is recorded separately, unexplained, and never promoted
    // to a deletion.
    let retirement = deletion_receipt_reason(client, &channel, &genesis_ref).await?;
    let genesis_unavailable = match (&genesis_event, &retirement) {
        (Some(_), _) => None,
        // The receipt already explains the absence; there is nothing unknown.
        (None, Some(_)) => None,
        (None, None) => Some(GENESIS_UNAVAILABLE_REASON.to_owned()),
    };
    let founder = genesis_event
        .as_ref()
        .map(|event| event.pubkey.to_hex())
        .unwrap_or_default();

    let authority = if retirement.is_some() || genesis_event.is_none() {
        None
    } else {
        Some(fetch_session_authority(client, &channel, &session_ref, &genesis_ref).await?)
    };

    let events = fetch_handover_events(client, &channel, &session_ref, &genesis_ref).await?;
    let context = HandoverFoldContext {
        channel_ref: channel.clone(),
        session_ref: session_ref.clone(),
        genesis_ref: genesis_ref.clone(),
        founder_pubkey: founder.clone(),
        grants: authority
            .as_ref()
            .map(|authority| authority.grants.clone())
            .unwrap_or_default(),
        seats: authority
            .as_ref()
            .map(|authority| authority.seats.clone())
            .unwrap_or_default(),
        claim: authority
            .as_ref()
            .map_or(ClaimState::NoClaim, |authority| authority.claim.clone()),
        claim_since: authority
            .as_ref()
            .and_then(|authority| authority.claim_since),
        retired: retirement.is_some(),
    };
    let fold = fold_coding_session_handover(&events, &context).map_err(CliError::Other)?;

    Ok(HandoverState {
        channel,
        session_ref,
        genesis_ref,
        founder,
        authority,
        fold,
        retirement,
        genesis_unavailable,
    })
}

/// Resolve `--genesis`, or find the umbrella's genesis on the relay.
async fn resolve_genesis(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
) -> Result<String, CliError> {
    match genesis {
        Some(genesis) => Ok(genesis.to_ascii_lowercase()),
        None => {
            let events =
                super::fetch_channel_events(client, channel, &[KIND_CODING_SESSION_GENESIS])
                    .await?;
            super::crew::resolve_umbrella_genesis(&events, session_ref)
        }
    }
}

/// Every verified 44247 record for this umbrella.
async fn fetch_handover_events(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis_ref: &str,
) -> Result<Vec<Event>, CliError> {
    let values = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_HANDOVER],
            "#h": [channel],
            "#d": [session_ref],
            "#csh-genesis": [genesis_ref],
        }))
        .await?;
    values
        .into_iter()
        .map(|value| {
            serde_json::from_value(value).map_err(|error| {
                CliError::Other(format!(
                    "relay returned a malformed handover record: {error}"
                ))
            })
        })
        .collect()
}

/// A relay-signed deletion receipt naming this genesis, rendered as a reason.
///
/// Absent is the ordinary answer — the receipt is new (§3.2) and older
/// deletions exist only as a kind 5 plus an absent genesis — so a relay that
/// serves none is not an error.
async fn deletion_receipt_reason(
    client: &BuzzClient,
    channel: &str,
    genesis_ref: &str,
) -> Result<Option<String>, CliError> {
    let relay_self = match fetch_trusted_relay_self(client).await {
        Ok(relay_self) => relay_self,
        // The relay's own key is what makes a receipt trustworthy; without it
        // no receipt can be believed, and the genesis-absence test below still
        // answers. This is disclosed by the caller, never swallowed.
        Err(_) => return Ok(None),
    };
    let events = client
        .query_all(json!({
            "kinds": [KIND_SYSTEM_MESSAGE],
            "#h": [channel],
            "authors": [relay_self],
        }))
        .await
        .unwrap_or_default();
    for value in events {
        let Ok(event) = serde_json::from_value::<Event>(value) else {
            continue;
        };
        let Ok(content) = serde_json::from_str::<Value>(&event.content) else {
            continue;
        };
        if content.get("type").and_then(Value::as_str) != Some(DELETION_ACCEPTANCE_RECEIPT_TYPE) {
            continue;
        }
        if content.get("genesisRef").and_then(Value::as_str) != Some(genesis_ref) {
            continue;
        }
        if buzz_core::verify_event(&event).is_err() || event.pubkey.to_hex() != relay_self {
            continue;
        }
        return Ok(Some(format!(
            "the relay published a signed deletion receipt ({}) for this session",
            event.id.to_hex()
        )));
    }
    Ok(None)
}

/// Build one signed kind 44247 record.
///
/// The envelope comes from `buzz-sdk`, which is also what the relay's
/// validator was written against, so the CLI cannot drift into a tag order or
/// a schema string the relay would refuse.
///
/// # Errors
/// The payload's own validation message when the record is out of bounds, so
/// a caller learns which field is wrong before anything is published.
pub(super) fn build_handover_event(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis_ref: &str,
    body: CodingSessionHandoverBody,
) -> Result<Event, CliError> {
    let payload = CodingSessionHandoverPayload {
        schema: CODING_SESSION_HANDOVER_SCHEMA.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis_ref: genesis_ref.to_owned(),
        handover_type: body.handover_type(),
        body,
    };
    let builder = build_coding_session_handover(channel, payload).map_err(sdk_err)?;
    client.sign_event_unchecked(builder)
}

/// Every execution of this umbrella, with its liveness and its published
/// fence, if the provider published one.
pub(super) struct ExecutionRow {
    /// The crew row.
    pub(super) execution: CrewExecution,
    /// The `handover` object the provider's newest 44223 metadata carried, or
    /// `None` when it carried none.
    ///
    /// `None` is honest and common: the field is additive and a provider that
    /// predates it publishes nothing, which is not the same as "not fenced".
    pub(super) published_fence: Option<Value>,
}

/// Read this umbrella's executions and each one's published fence.
pub(super) async fn fetch_executions(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
) -> Result<Vec<ExecutionRow>, CliError> {
    let events = super::fetch_channel_events(client, channel, EXECUTION_FACT_KINDS).await?;
    let lease_events = client
        .query_all(json!({ "kinds": [KIND_CODING_SESSION_LEASE], "#h": [channel] }))
        .await
        .unwrap_or_default();
    let leases = decode_leases(&lease_events);
    let (metadata, _) = super::decode_metadata(&events);
    let (receipts, _) = super::decode_receipts(&events);
    let (transcripts, _) = super::decode_transcripts(&events);
    let now = chrono::Utc::now().timestamp();
    let executions = build_executions(&metadata, &receipts, &transcripts, &leases, now);

    let mut rows = Vec::new();
    for execution in executions {
        if execution.session_ref.as_deref() != Some(session_ref) {
            continue;
        }
        let published_fence = metadata
            .iter()
            .filter(|record| record.target_key == execution.target_key)
            .max_by_key(|record| record.created_at)
            .and_then(|record| {
                record
                    .raw
                    .get("content")
                    .and_then(Value::as_str)
                    .and_then(|content| serde_json::from_str::<Value>(content).ok())
            })
            .and_then(|content| content.get("handover").cloned())
            .filter(|value| !value.is_null());
        rows.push(ExecutionRow {
            execution,
            published_fence,
        });
    }
    Ok(rows)
}

/// Whether a candidate execution is reachable: a live kind-24223 lease answers
/// for its **current generation**.
///
/// Nothing weaker counts. A `quiet` execution is one whose provider stopped
/// renewing its lease, which is exactly the absent participant this feature
/// exists for, and treating it as reachable would send a native turn into a
/// machine that cannot answer.
pub(super) fn is_reachable(row: &ExecutionRow) -> bool {
    row.execution.liveness == Liveness::Live
}

/// `bee sessions handover status` — print the fold.
pub(super) async fn cmd_status(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: Option<&str>,
    as_json: bool,
) -> Result<(), CliError> {
    let state = load_handover_state(client, channel, session_ref, genesis).await?;
    let executions = fetch_executions(client, &state.channel, &state.session_ref).await?;

    let mut notes = VerificationNotes::default();
    notes.verified(format!(
        "the accepted 44228 chain for genesis {} was projected from relay-signed acceptance \
         receipts, and the claim was folded from it",
        state.genesis_ref
    ));
    notes.verified(format!(
        "{} handover record(s) were signature-checked and envelope-checked before folding",
        state.fold.checkpoints.len() + state.fold.continuations.len()
    ));
    match (&state.retirement, &state.genesis_unavailable) {
        (Some(_), _) => notes.not_verified(
            "the authority chain was not projected: a signed deletion receipt retires this \
             umbrella, so there is no live chain to project"
                .to_owned(),
        ),
        (None, Some(reason)) => notes.not_verified(format!(
            "{reason}. Nothing below rests on a verified chain, and this is NOT a statement \
             that the session was deleted — no signed deletion receipt names it"
        )),
        (None, None) => {}
    }
    notes.not_verified(
        "no provider was contacted: the fence each execution reports below is what its provider \
         last published in kind 44223, not an answer to a request made now"
            .to_owned(),
    );

    let execution_json: Vec<Value> = executions
        .iter()
        .map(|row| {
            json!({
                "target": row.execution.target_key,
                "seat": row.execution.seat_label(),
                "status": row.execution.status,
                "liveness": row.execution.liveness.render(),
                "reachable": is_reachable(row),
                "publishedFence": row.published_fence.clone().unwrap_or(Value::Null),
            })
        })
        .collect();

    if as_json {
        println!(
            "{}",
            json!({
                "sessionRef": state.session_ref,
                "genesisRef": state.genesis_ref,
                "founder": state.founder,
                "retired": state.fold.retired,
                "retirement": state.retirement.clone().map_or(Value::Null, Value::from),
                // Never folded into `retired`: an unreadable genesis is not a
                // deletion, and a consumer that could not tell them apart
                // would render one as the other (§3.2).
                "genesisUnavailable": state
                    .genesis_unavailable
                    .clone()
                    .map_or(Value::Null, Value::from),
                "fold": state.fold,
                // The fold's own `latestAuthorizedCheckpoint` orders by
                // (created_at, id); this is the one nothing replaces.
                "latestCheckpoint": state
                    .latest_authorized_checkpoint()
                    .map_or(Value::Null, |entry| Value::from(entry.event_id.clone())),
                "supersededCheckpoints": state
                    .superseded_checkpoints()
                    .iter()
                    .map(|entry| json!({
                        "eventId": entry.event_id,
                        "author": entry.author,
                        "supersededBy": entry.superseded_by,
                    }))
                    .collect::<Vec<_>>(),
                "executions": execution_json,
                "scope": WHOLE_SESSION_DISCLOSURE,
                "notes": notes.to_json(),
            })
        );
        return Ok(());
    }

    println!(
        "session {} (genesis {})",
        state.session_ref, state.genesis_ref
    );
    println!("scope: {WHOLE_SESSION_DISCLOSURE}");
    println!("{}", state.existence_line());
    print_claim(&state);
    print_latest_checkpoint(&state);
    print_continuations(&state);
    print_executions(&executions);
    if !state.fold.excluded.is_empty() {
        println!("excluded records:");
        for exclusion in &state.fold.excluded {
            println!("  - {}: {}", exclusion.event_id, exclusion.reason);
        }
    }
    print!("{}", notes.render());
    Ok(())
}

/// Print the claim, keeping `NoClaim` and `Voided` apart.
fn print_claim(state: &HandoverState) {
    match state.claim() {
        ClaimState::NoClaim => println!(
            "claim: none — no handover has ever happened on this session, so the ordinary rules \
             apply"
        ),
        ClaimState::Active(claim) => {
            let since = state
                .fold
                .claim_since
                .map_or_else(|| "unknown".to_owned(), |at| at.to_string());
            println!(
                "claim: active — {} holds this whole session on body {} (link {}, seq {}, since \
                 unix {since})",
                short_pubkey(&claim.claimant),
                short_pubkey(&claim.body_pubkey),
                claim.accepted_event_id,
                claim.seq,
            );
        }
        ClaimState::Voided {
            last, voided_by, ..
        } => println!(
            "claim: voided — {} held this session on body {} and lost standing (voided by {}). \
             The fence stays up for everyone until a fresh takeover is accepted; a regrant does \
             not restore it.",
            short_pubkey(&last.claimant),
            short_pubkey(&last.body_pubkey),
            voided_by,
        ),
    }
}

/// Print the newest authorized checkpoint in full.
fn print_latest_checkpoint(state: &HandoverState) {
    match state.latest_authorized_checkpoint() {
        None => println!(
            "checkpoint: none authorized — nothing here may be reconstructed from ({} record(s) \
             listed, see excluded)",
            state.fold.checkpoints.len()
        ),
        Some(entry) => {
            println!(
                "checkpoint {}: by {} at unix {}",
                entry.event_id,
                short_pubkey(&entry.author),
                entry.created_at
            );
            let revision = &entry.body.revision;
            println!(
                "  revision: repo {} branch {} head {} base {} dirty {} preserved {}",
                revision.repo_ref.as_deref().unwrap_or("null"),
                revision.branch.as_deref().unwrap_or("null"),
                revision.head_sha.as_deref().unwrap_or("null"),
                revision.base_sha.as_deref().unwrap_or("null"),
                revision.dirty,
                preserved_word(revision.preserved),
            );
            for artifact in &entry.body.artifacts {
                println!("  artifact: {}", artifact_line(artifact));
            }
            for line in &entry.body.missing {
                println!("  missing: {line}");
            }
        }
    }
    print_superseded_checkpoints(state);
}

/// List the checkpoints a later one by the same author replaced.
///
/// History, not noise: they were written and signed, a continuation may name
/// one, and a reader who cannot see them cannot tell "there was only ever this
/// statement" from "this statement replaced three others".
fn print_superseded_checkpoints(state: &HandoverState) {
    let superseded = state.superseded_checkpoints();
    if superseded.is_empty() {
        return;
    }
    println!(
        "superseded checkpoints ({}): history, replaced by a later checkpoint from the same \
         author and never used for reconstruction",
        superseded.len()
    );
    for entry in superseded {
        println!(
            "  - {} by {} at unix {}, replaced by {}",
            entry.event_id,
            short_pubkey(&entry.author),
            entry.created_at,
            entry.superseded_by.as_deref().unwrap_or("(unknown)"),
        );
    }
}

/// Print every continuation, labelled by standing.
fn print_continuations(state: &HandoverState) {
    if state.fold.continuations.is_empty() {
        println!("continuations: none");
        return;
    }
    for entry in &state.fold.continuations {
        let standing = match entry.standing {
            HandoverStanding::Authorized => "current",
            HandoverStanding::Superseded => "historical",
            HandoverStanding::Unauthorized => "unauthorized",
        };
        println!(
            "continuation {} ({standing}): {} by {} on claim {}",
            entry.event_id,
            entry.mode.as_str(),
            short_pubkey(&entry.author),
            entry.claim_ref
        );
        for line in &entry.body.recovered {
            println!("  recovered: {line}");
        }
        for line in &entry.body.missing {
            println!("  missing: {line}");
        }
    }
}

/// Print each execution's liveness and the fence its provider published.
fn print_executions(rows: &[ExecutionRow]) {
    if rows.is_empty() {
        println!("executions: none recorded for this umbrella");
        return;
    }
    for row in rows {
        let fence = match &row.published_fence {
            Some(value) => format!("fenced per its provider's metadata: {value}"),
            None => "its provider's metadata carries no handover field (this build of the \
                     provider may predate the field; absence is not proof it is unfenced)"
                .to_owned(),
        };
        println!(
            "execution {} [{}] status {} liveness {} — {fence}",
            row.execution.target_key,
            row.execution.seat_label(),
            row.execution.status,
            row.execution.liveness.render(),
        );
    }
}

/// Route one `handover` subcommand.
pub async fn dispatch(cmd: HandoverCmd, client: &BuzzClient) -> Result<(), CliError> {
    match cmd {
        HandoverCmd::Checkpoint(args) => {
            super::handover_checkpoint::cmd_checkpoint(client, args).await
        }
        HandoverCmd::Claim(args) => super::handover_claim::cmd_claim(client, args).await,
        HandoverCmd::Continue(args) => super::handover_continue::cmd_continue(client, args).await,
        HandoverCmd::Status(args) => {
            cmd_status(
                client,
                &args.channel,
                &args.session_ref,
                args.genesis.as_deref(),
                args.json,
            )
            .await
        }
    }
}

#[cfg(test)]
#[path = "handover_tests.rs"]
mod tests;

// Split by topic, not by size alone: arguments and refusals above, everything
// that is signed or read back off the wire here.
#[cfg(test)]
#[path = "handover_wire_tests.rs"]
mod wire_tests;

// Split by subject again: the authority chain above, the 44247 records this
// command writes here.
#[cfg(test)]
#[path = "handover_record_tests.rs"]
mod record_tests;
