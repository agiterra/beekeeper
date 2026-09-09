//! `bee sessions handover claim` — take the whole session over, at the relay.
//!
//! A claim is one more link on the accepted kind-44228 chain, so the relay is
//! the thing that decides a race: both claimants extend the same head, exactly
//! one link is accepted, and the loser is refused **by name** rather than
//! discovering later that its work was fenced. That is why this command reads
//! the head, publishes at it, and — on a stale-head refusal — re-reads the
//! chain **once** and reports the claimant who actually won, instead of
//! retrying blindly into a race it has already lost.
//!
//! Two things about that refusal are easy to get wrong, and both were:
//! it arrives as a **2xx** whose body says `{"accepted": false, …}`, not as a
//! transport error; and its text is the relay's own sentence ("invalid:
//! prevAccepted does not match the chain's current head (expected …)"), not
//! the name of the refusal variant behind it. Matching the variant names on
//! the error arm alone meant the path never fired (REVIEW B2).
//!
//! # `--body` names a machine, not a person
//!
//! `bodyPubkey` is the **provider authority pubkey** of the execution body the
//! claimant will use. The fence compares it against the provider that receives
//! a turn, so a claim naming the wrong body fences the claimant out of their
//! own session. `--body-self` resolves it from the wire — the provider
//! authority this caller's own `session.create`s have named in this channel —
//! and refuses when that is ambiguous rather than picking one.

use std::time::Duration;

use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use buzz_core::kind::{KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_SYSTEM_MESSAGE};
use buzz_sdk::coding_session_handover::build_coding_session_takeover;
use nostr::Event;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{sdk_err, validate_lower_hex64};
use crate::HandoverClaimArgs;

use super::crew::short_pubkey;
use super::handover::{
    body_liveness_of, fetch_executions, live_target_on, load_handover_state, next_action_line,
    BodyLiveness, HandoverState, CLAIM_MOVES_THE_FENCE,
};
use super::handover_render::{VerificationNotes, WHOLE_SESSION_DISCLOSURE};
use super::operations_authority::fetch_trusted_relay_self;

/// How often the receipt wait re-asks the relay. The same cadence as
/// `DELIVERY_POLL` in `crew_cmds`.
const RECEIPT_POLL: Duration = Duration::from_millis(500);

/// Longest wait this command will accept, so a caller cannot turn a bounded
/// wait into an unbounded one by argument.
pub const MAX_CLAIM_WAIT_SECONDS: u64 = 300;

/// The relay refusal substrings that mean "somebody else moved the head".
///
/// **These are the relay's own sentences, not its refusal enum's names.** The
/// first version of this list matched `"StaleHead"` and `"SeqMismatch"` — the
/// `AuthorityTransitionRefusal` variants — and those strings never reach a
/// client: `crates/buzz-relay/src/handlers/ingest.rs` renders them as
/// "invalid: prevAccepted does not match the chain's current head (expected
/// …)" and "invalid: seq does not extend the chain (expected …)". So the whole
/// lost-race path was dead, and a claimant who lost a race got exit 4 and no
/// winner (REVIEW B2). Substrings, not equality, because the relay appends the
/// value it expected.
const RACE_REFUSALS: [&str; 3] = [
    "prevAccepted does not match",
    "prevAccepted must be null",
    "seq does not extend",
];

/// Whether a relay refusal message says this link lost a race at the head.
///
/// Pure and crate-visible so the sentences the relay actually emits can be
/// asserted against it directly, rather than only through a live relay.
pub(super) fn is_race_refusal(message: &str) -> bool {
    RACE_REFUSALS.iter().any(|phrase| message.contains(phrase))
}

/// The relay's refusal message from a write response, when it refused.
///
/// `submit_event` returns `Ok` for a stored-but-refused write: the HTTP call
/// succeeded and the body says `{"accepted": false, "message": …}`. Reading
/// that body **before** `parse_write_response` is the whole of the fix for
/// REVIEW B2 — that helper turns every refusal into `CliError::Other`, which
/// exits 4 and carries no structure a race check could read.
pub(super) fn write_refusal_message(raw: &str) -> Option<String> {
    let response: Value = serde_json::from_str(raw).ok()?;
    let accepted = response
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if accepted {
        return None;
    }
    Some(
        response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    )
}

/// What publishing a claim produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaimOutcome {
    /// Event id of the accepted `takeover`.
    pub(super) accepted_event_id: String,
    /// Its chain sequence number.
    pub(super) seq: u32,
    /// The claimant — this caller.
    pub(super) claimant: String,
    /// The body the claim fences to.
    pub(super) body_pubkey: String,
    /// The relay's 40099 acceptance receipt, when one arrived inside the wait.
    ///
    /// `None` means the wait elapsed, which is a fact about the wait and not
    /// about the claim: the relay stored the link either way, and the caller
    /// is told exactly that.
    pub(super) receipt_event_id: Option<String>,
}

/// `bee sessions handover claim`.
pub(super) async fn cmd_claim(
    client: &BuzzClient,
    args: HandoverClaimArgs,
) -> Result<(), CliError> {
    let wait_secs = bounded_wait(args.wait_secs)?;
    let state = load_handover_state(
        client,
        &args.channel,
        &args.session_ref,
        args.genesis.as_deref(),
    )
    .await?;
    refuse_retired(&state)?;
    refuse_unverifiable_genesis(&state)?;
    let caller = client.keys().public_key().to_hex();
    if !state.has_standing(&caller) {
        return Err(CliError::Usage(format!(
            "{} is neither the founder of this session nor a live operator on it, and §1 \
             reserves a takeover for those two. Ask the founder for `grant-operator` first.",
            short_pubkey(&caller)
        )));
    }
    let body_pubkey = resolve_body(client, &state, args.body.as_deref(), args.body_self).await?;

    let mut notes = VerificationNotes::default();
    let outcome = claim_session(client, &state, &body_pubkey, wait_secs, &mut notes).await?;

    // The claim moved the fence. Whether anything is *running* on the body it
    // moved to is a separate fact, read separately, and printed as its own
    // line — the composition run had a successful takeback followed by
    // `turn_dropped/NO_LIVE_EXECUTION`, because the two had been conflated.
    let (liveness, live_target) =
        read_body_liveness(client, &state.channel, &state.session_ref, &body_pubkey).await;
    notes.verified(CLAIM_MOVES_THE_FENCE.to_owned());
    match liveness {
        BodyLiveness::Unknown => notes.not_verified(format!(
            "the execution liveness of body {} could not be read, so this run does not know \
             whether anything is running there",
            short_pubkey(&body_pubkey)
        )),
        BodyLiveness::Live | BodyLiveness::NotLive => notes.verified(format!(
            "body {} reads {} from the relay's kind-24223 lease snapshot",
            short_pubkey(&body_pubkey),
            liveness.word()
        )),
    }
    notes.not_verified(
        "this command sent no turn and no resume: claiming a session steers nothing".to_owned(),
    );

    let next_action = next_action_line(
        liveness,
        &body_pubkey,
        &state.channel,
        &state.session_ref,
        live_target.as_deref(),
    );
    report_claim(&outcome, &next_action, &notes, args.json);
    Ok(())
}

/// Read the claimed body's execution liveness, the way `continue` reads it.
///
/// A failed read answers [`BodyLiveness::Unknown`] rather than `NotLive`: "the
/// relay did not answer" and "nothing is running there" send a person to two
/// different places, and only one of them is true at a time.
async fn read_body_liveness(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    body_pubkey: &str,
) -> (BodyLiveness, Option<String>) {
    match fetch_executions(client, channel, session_ref).await {
        Ok(rows) => (
            body_liveness_of(&rows, body_pubkey),
            live_target_on(&rows, body_pubkey),
        ),
        Err(_) => (BodyLiveness::Unknown, None),
    }
}

/// Refuse the wait ceiling rather than silently clamping it.
pub(super) fn bounded_wait(seconds: u64) -> Result<u64, CliError> {
    if seconds == 0 || seconds > MAX_CLAIM_WAIT_SECONDS {
        return Err(CliError::Usage(format!(
            "--wait-secs must be between 1 and {MAX_CLAIM_WAIT_SECONDS}"
        )));
    }
    Ok(seconds)
}

/// Refuse every verb on a retired umbrella, in the words §3.2 uses.
///
/// Retirement here means a **relay-signed deletion receipt**. A session whose
/// genesis simply did not come back is a different state entirely
/// ([`refuse_unverifiable_genesis`]) and must not be reported as deleted.
pub(super) fn refuse_retired(state: &HandoverState) -> Result<(), CliError> {
    match &state.retirement {
        None => Ok(()),
        Some(reason) => Err(CliError::Usage(format!(
            "this session has been deleted, so nothing here is claimed, reconstructed or \
             resumed: {reason}"
        ))),
    }
}

/// Refuse a claim or a continuation whose session's authority could not be
/// read at all.
///
/// Fail-closed and **not** the retired path. A relay that returns no genesis
/// may have applied a deletion, may be behind, or may be scoping the read
/// away; treating those three alike would let a partitioned relay retire
/// somebody's session. So this says only what it knows, exits 2 — the
/// network/relay code, because a relay read is what failed — and leaves the
/// session exactly as it was.
pub(super) fn refuse_unverifiable_genesis(state: &HandoverState) -> Result<(), CliError> {
    match &state.genesis_unavailable {
        None => Ok(()),
        Some(reason) => Err(CliError::Unverifiable(format!(
            "{reason}. Nothing was claimed or continued: this is not proof the session was \
             deleted, only that its genesis could not be read. Retry against a healthy relay, \
             or pass --genesis if the umbrella's genesis is known."
        ))),
    }
}

/// Resolve `bodyPubkey` from `--body`, or from the wire with `--body-self`.
pub(super) async fn resolve_body(
    client: &BuzzClient,
    state: &HandoverState,
    body: Option<&str>,
    body_self: bool,
) -> Result<String, CliError> {
    match (body, body_self) {
        (Some(_), true) => Err(CliError::Usage(
            "--body and --body-self both name the execution body; give exactly one".into(),
        )),
        (Some(body), false) => {
            let body = body.to_ascii_lowercase();
            validate_lower_hex64("--body", &body)?;
            Ok(body)
        }
        (None, true) => resolve_self_body(client, state).await,
        (None, false) => Err(CliError::Usage(
            "a claim must name the execution body it fences to: pass --body \
             <providerAuthorityPubkey>, or --body-self to use the provider authority this \
             caller's own session creates have named in this channel"
                .into(),
        )),
    }
}

/// The provider authority this caller's own creates have named in this channel.
///
/// Derived from the wire rather than from any local file: the CLI holds no
/// provider identity of its own, and a guess here would fence the wrong
/// machine. More than one distinct answer is refused by name rather than
/// resolved by recency — "the newest one" is not a fact about which provider
/// the caller means to run on.
async fn resolve_self_body(client: &BuzzClient, state: &HandoverState) -> Result<String, CliError> {
    let caller = client.keys().public_key().to_hex();
    let events = super::fetch_channel_events(
        client,
        &state.channel,
        &[KIND_CODING_SESSION_LIFECYCLE_COMMAND],
    )
    .await?;
    let mut found: Vec<String> = Vec::new();
    for value in &events {
        if value.get("pubkey").and_then(Value::as_str) != Some(caller.as_str()) {
            continue;
        }
        let Some(content) = value.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(payload) = decode_coding_session_lifecycle_command(content) else {
            continue;
        };
        if let CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        } = payload.action
        {
            let authority = provider_authority_pubkey.to_ascii_lowercase();
            if !found.contains(&authority) {
                found.push(authority);
            }
        }
    }
    match found.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err(CliError::Usage(
            "--body-self found no session create signed by this caller in this channel, so there \
             is no provider authority to read. Pass --body <providerAuthorityPubkey> — the same \
             value `bee sessions create --provider-authority` takes."
                .into(),
        )),
        many => Err(CliError::Usage(format!(
            "--body-self is ambiguous: this caller's session creates in this channel name {} \
             different provider authorities ({}). Pass --body to say which one this claim \
             fences to.",
            many.len(),
            many.iter()
                .map(|key| short_pubkey(key))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// Publish one `takeover` at the current head and wait for its receipt.
///
/// Shared by `handover claim` and by `handover continue` step 3, so both
/// produce the same link, the same race behaviour and the same exit code.
pub(super) async fn claim_session(
    client: &BuzzClient,
    state: &HandoverState,
    body_pubkey: &str,
    wait_secs: u64,
    notes: &mut VerificationNotes,
) -> Result<ClaimOutcome, CliError> {
    let authority = state.authority.as_ref().ok_or_else(|| {
        CliError::Other(
            "no accepted authority chain was projected for this session, so a claim cannot name \
             a head to extend"
                .to_owned(),
        )
    })?;
    let caller = client.keys().public_key().to_hex();
    let seq = authority
        .head_seq
        .checked_add(1)
        .ok_or_else(|| CliError::Other("authority chain seq overflow".into()))?;
    let event = sign_takeover(
        client,
        &state.channel,
        &state.genesis_ref,
        authority.head_event_id.clone(),
        seq,
        &caller,
        body_pubkey,
    )?;
    let accepted_event_id = event.id.to_hex();
    // Taken before the write so a receipt published in the same second cannot
    // fall outside the wait's window.
    let since = chrono::Utc::now().timestamp() - 1;

    match client.submit_event(event).await {
        Ok(raw) => {
            // A relay refusal is a 2xx with `accepted: false`, so it arrives
            // here and not on the `Err` arm. Read it before
            // `parse_write_response` flattens it to `CliError::Other`.
            if let Some(message) = write_refusal_message(&raw) {
                if is_race_refusal(&message) {
                    return Err(lost_race(client, state, &message).await);
                }
            }
            crate::commands::parse_write_response(&raw, "takeover already accepted")?;
        }
        Err(error) => {
            // And a relay that answers a refusal with a non-2xx status puts
            // the same sentence in the body, so the check runs on both arms
            // rather than on whichever one this relay build happens to use.
            let message = error.to_string();
            if is_race_refusal(&message) {
                return Err(lost_race(client, state, &message).await);
            }
            return Err(error);
        }
    }
    notes.verified(format!(
        "the relay accepted takeover {accepted_event_id} at seq {seq} extending head {}",
        authority
            .head_event_id
            .as_deref()
            .unwrap_or("(none — this is the chain's first link)")
    ));

    let receipt_event_id =
        await_claim_receipt(client, &state.channel, &accepted_event_id, since, wait_secs).await;
    match &receipt_event_id {
        Some(id) => notes.verified(format!(
            "the relay's signed acceptance receipt {id} names this takeover, so every consumer \
             folding the chain will see it"
        )),
        None => notes.not_verified(format!(
            "no signed acceptance receipt arrived within {wait_secs}s. The relay stored the link \
             — that is what `accepted` above means — but consumers that fold from receipts have \
             not been observed to see it yet; re-run `handover status` to check"
        )),
    }

    Ok(ClaimOutcome {
        accepted_event_id,
        seq,
        claimant: caller,
        body_pubkey: body_pubkey.to_owned(),
        receipt_event_id,
    })
}

/// Re-read the chain once and name the claimant who actually won.
async fn lost_race(client: &BuzzClient, state: &HandoverState, message: &str) -> CliError {
    let reread = load_handover_state(
        client,
        &state.channel,
        &state.session_ref,
        Some(&state.genesis_ref),
    )
    .await;
    let winner = match reread {
        Ok(state) => match state.claim() {
            ClaimState::Active(claim) => format!(
                "{} now holds this session on body {} (link {})",
                short_pubkey(&claim.claimant),
                short_pubkey(&claim.body_pubkey),
                claim.accepted_event_id
            ),
            ClaimState::Voided { last, .. } => format!(
                "the chain moved and the session is now voided; {} held it last",
                short_pubkey(&last.claimant)
            ),
            ClaimState::NoClaim => {
                "the chain moved but still carries no claim — somebody else extended it with a \
                 grant or a seat, so this takeover can be retried at the new head"
                    .to_owned()
            }
        },
        Err(error) => format!("the chain could not be re-read to name the winner: {error}"),
    };
    CliError::Conflict(format!(
        "this takeover lost the race at the relay ({message}). {winner}. Nothing was retried: a \
         blind retry at a moved head would claim a session somebody else already holds."
    ))
}

/// Sign one 44228 `takeover` through the SDK builder.
///
/// The builder is what the relay's acceptance rules were written against, so
/// the envelope, the schema and the claimant/self check cannot drift between
/// what this command mints and what the relay will take.
fn sign_takeover(
    client: &BuzzClient,
    channel: &str,
    genesis: &str,
    prev_accepted: Option<String>,
    seq: u32,
    claimant: &str,
    body_pubkey: &str,
) -> Result<Event, CliError> {
    let channel = Uuid::parse_str(channel)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let builder =
        build_coding_session_takeover(channel, genesis, prev_accepted, seq, claimant, body_pubkey)
            .map_err(sdk_err)?;
    client.sign_event_unchecked(builder)
}

/// Wait, bounded, for the relay-signed receipt naming `accepted_event_id`.
async fn await_claim_receipt(
    client: &BuzzClient,
    channel: &str,
    accepted_event_id: &str,
    since: i64,
    wait_secs: u64,
) -> Option<String> {
    let relay_self = fetch_trusted_relay_self(client).await.ok()?;
    let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
    let filter = json!({
        "kinds": [KIND_SYSTEM_MESSAGE],
        "#h": [channel],
        "authors": [relay_self],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            if let Some(id) = find_claim_receipt(&events, accepted_event_id, &relay_self) {
                return Some(id);
            }
        }
        if std::time::Instant::now() + RECEIPT_POLL >= deadline {
            return None;
        }
        tokio::time::sleep(RECEIPT_POLL).await;
    }
}

/// The relay-signed acceptance receipt naming `accepted_event_id`, if present.
///
/// Verified before it is believed: an unsigned or wrongly-signed row is not a
/// receipt, and returning its id would report an acceptance nobody made.
pub(super) fn find_claim_receipt(
    events: &[Value],
    accepted_event_id: &str,
    relay_self: &str,
) -> Option<String> {
    for value in events {
        let Ok(event) = serde_json::from_value::<Event>(value.clone()) else {
            continue;
        };
        let Ok(content) = serde_json::from_str::<Value>(&event.content) else {
            continue;
        };
        if content.get("type").and_then(Value::as_str)
            != Some(super::operations_authority::AUTHORITY_ACCEPTANCE_RECEIPT_TYPE)
        {
            continue;
        }
        if content.get("acceptedEventId").and_then(Value::as_str) != Some(accepted_event_id) {
            continue;
        }
        if event.pubkey.to_hex() != relay_self || buzz_core::verify_event(&event).is_err() {
            continue;
        }
        return Some(event.id.to_hex());
    }
    None
}

/// Print what the claim did.
fn report_claim(
    outcome: &ClaimOutcome,
    next_action: &str,
    notes: &VerificationNotes,
    as_json: bool,
) {
    if as_json {
        println!(
            "{}",
            json!({
                "acceptedEventId": outcome.accepted_event_id,
                "seq": outcome.seq,
                "claimant": outcome.claimant,
                "bodyPubkey": outcome.body_pubkey,
                "receiptEventId": outcome
                    .receipt_event_id
                    .clone()
                    .map_or(Value::Null, Value::from),
                "scope": WHOLE_SESSION_DISCLOSURE,
                "fence": CLAIM_MOVES_THE_FENCE,
                "nextAction": next_action,
                "notes": notes.to_json(),
            })
        );
        return;
    }
    println!(
        "claimed: {} holds this session on body {}",
        short_pubkey(&outcome.claimant),
        short_pubkey(&outcome.body_pubkey)
    );
    println!("scope: {WHOLE_SESSION_DISCLOSURE}");
    println!("link {} at seq {}", outcome.accepted_event_id, outcome.seq);
    print!("{}", notes.render());
}
