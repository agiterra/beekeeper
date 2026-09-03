//! `bee sessions policy set|get|clear` — the writer and reader for NIP-CSP
//! session policy records (kind 44245).
//!
//! One signed record saying how a mission is meant to be run. Before it,
//! budgets lived in a launch dialog, "red first" lived in `AGENTS.md`, and
//! "ask before you push" lived in a person's memory of having said it once
//! (`docs/design/portable-team-loop/POLICY.md`).
//!
//! # What this command may and may not claim
//!
//! Two fields are enforced anywhere in this repository: `budget.turns`, at the
//! provider's turn gate, and `gates.verifierRequired`, at the 44244 fold's
//! completion check. Everything else is **read and shown, not enforced**, and
//! `get` says exactly that in its own output rather than leaving a reader to
//! assume a budget bar is being counted. Printing an unenforced ceiling as
//! though something were counting it is the same defect as a status that reads
//! Idle over a disconnected provider — and so is the reverse, which is what
//! REVIEW-L7 F1 found: telling a founder nothing counts a field the next
//! command is about to refuse them for.
//!
//! # Standing — one rule, and this command does not invent a second
//!
//! The relay stores a structurally valid 44245 from **any** channel member, by
//! design: NIP-CSP's validation boundary says the relay checks shape and the
//! consumer adjudicates. The rule every consumer applies is the **provable**
//! one — the umbrella's founder, or a seat holding an operator grant the relay
//! had already accepted when the record was published. A `lead` role slug is
//! not enough and is not an input: a provider can prove a grant from the
//! accepted NIP-CSAT chain and cannot prove a role slug it did not mint.
//!
//! `set` and `clear` refuse before signing when this key does not hold it, and
//! `get` folds it — both through
//! [`fold_coding_session_policies`](buzz_core::coding_session_policy::fold_coding_session_policies),
//! the same function the session provider's context projection calls. Two
//! surfaces that answered "which record is the policy" differently is exactly
//! what REVIEW-B2 F1 found: this command printed a stranger's `turns: 9999` as
//! "the newest accepted policy" while the provider correctly ignored it.

use buzz_core::coding_session_policy::{
    fold_coding_session_policies, signer_may_steer_at, CodingSessionAttention,
    CodingSessionContextTier, CodingSessionIrreversibleAct, CodingSessionPolicyBench,
    CodingSessionPolicyBudget, CodingSessionPolicyFold, CodingSessionPolicyGates,
    CodingSessionPolicyGrant, CodingSessionPolicyPayload, CodingSessionPolicyStop,
    CodingSessionPosture,
};
use buzz_core::kind::KIND_CODING_SESSION_POLICY;
use buzz_sdk::coding_session_policy::build_coding_session_policy;
use nostr::Event;
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};
use crate::{SessionPolicyCmd, SessionPolicySetArgs};

/// The one sentence every surface that renders a policy owes its reader.
///
/// Repeated verbatim by `get` and by `set`'s answer. It names **every** field
/// something actually counts and says plainly that nothing counts the rest;
/// a surface that softens it, or that lists a field this sentence does not,
/// is lying to the person who set the policy
/// (`docs/design/portable-team-loop/POLICY.md` §4).
///
/// `gates.verifierRequired` joined it on 2026-09-02 (batch 3, item G): the
/// 44244 fold now excludes a `mission.completed` that settled on a report no
/// active verifier ruled on, and `bee sessions complete` refuses to sign one.
/// Before that landing this sentence said only `budget.turns` was enforced,
/// which would have told a founder who set `--verifier-required true` that
/// nothing counted it, immediately before refusing their completion.
///
/// Byte-identical copies live in the Tauri adapter and in POLICY.md §4.2;
/// `crates/buzz-cli/tests/policy_enforcement_sentence.rs` holds all three
/// together.
pub const POLICY_ENFORCEMENT_DISCLOSURE: &str =
    "Enforced: budget.turns at the provider's turn gate, gates.verifierRequired at the fold's \
     completion check and at the relay's verdict-gated push, and gates.requiredGates at that \
     push. Every other field is read and shown, never counted.";

/// Dispatch `bee sessions policy`.
pub async fn cmd_policy(client: &BuzzClient, cmd: SessionPolicyCmd) -> Result<(), CliError> {
    match cmd {
        SessionPolicyCmd::Set(args) => set(client, args).await,
        SessionPolicyCmd::Get {
            channel,
            session_ref,
            genesis,
        } => get(client, &channel, &session_ref, &genesis).await,
        SessionPolicyCmd::Clear {
            channel,
            session_ref,
            genesis,
        } => clear(client, &channel, &session_ref, &genesis).await,
    }
}

fn validate_coordinates(channel: &str, session_ref: &str, genesis: &str) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    validate_lower_hex64("--genesis", genesis)
}

/// Build the payload for `set` from its flags.
///
/// A sub-object is emitted only when at least one of its own flags was passed:
/// NIP-CSP refuses an empty `budget`/`gates`/`bench`/`stop`, because "no
/// budget" is said by omitting the key, not by an empty object
/// (`POLICY.md` §2.3).
pub(super) fn payload_from_args(
    args: &SessionPolicySetArgs,
) -> Result<CodingSessionPolicyPayload, CliError> {
    let posture: Option<CodingSessionPosture> = closed_word(
        "--posture",
        args.posture.as_deref(),
        &["spike", "ship", "investigate", "overnight"],
    )?;
    let attention: Option<CodingSessionAttention> = closed_word(
        "--attention",
        args.attention.as_deref(),
        &["decisions", "decisions-and-milestones", "everything"],
    )?;
    let context_tier: Option<CodingSessionContextTier> = closed_word(
        "--context-tier",
        args.context_tier.as_deref(),
        &["standard", "long"],
    )?;
    let mut irreversible: Vec<CodingSessionIrreversibleAct> = Vec::new();
    for act in &args.irreversible {
        let parsed = closed_word(
            "--irreversible",
            Some(act.as_str()),
            &["push", "deploy", "delete", "external-message"],
        )?;
        if let Some(parsed) = parsed {
            irreversible.push(parsed);
        }
    }

    let budget = CodingSessionPolicyBudget {
        turns: args.budget_turns,
        tokens_per_seat: args.tokens_per_seat,
        tokens_per_session: args.tokens_per_session,
        cost_usd_per_session: args.cost_usd,
        context_tier,
    };
    let gates = CodingSessionPolicyGates {
        red_first: args.red_first,
        review_every_lane: args.review_every_lane,
        required_gates: (!args.required_gate.is_empty()).then(|| args.required_gate.clone()),
        verifier_required: args.verifier_required,
    };
    let bench = CodingSessionPolicyBench {
        identities: (!args.bench_identity.is_empty()).then(|| args.bench_identity.clone()),
        providers: (!args.bench_provider.is_empty()).then(|| args.bench_provider.clone()),
        challenger_sample_rate: args.challenger_sample_rate,
    };
    let stop = CodingSessionPolicyStop {
        time_box_secs: args.time_box_secs,
        on_milestone: args.on_milestone.clone(),
    };

    for identity in &args.bench_identity {
        validate_lower_hex64("--bench-identity", identity)?;
    }

    let payload = CodingSessionPolicyPayload {
        posture,
        budget: budget_is_set(&budget).then_some(budget),
        attention,
        gates: gates_are_set(&gates).then_some(gates),
        bench: bench_is_set(&bench).then_some(bench),
        irreversible: (!irreversible.is_empty()).then_some(irreversible),
        stop: stop_is_set(&stop).then_some(stop),
        ..CodingSessionPolicyPayload::empty(args.session_ref.clone(), args.genesis.clone())
    };
    if !payload.sets_any_policy() {
        return Err(CliError::Usage(
            "`policy set` with no policy flags would publish the withdrawal record. That is a \
             real decision, so it has its own verb: run `bee sessions policy clear`."
                .into(),
        ));
    }
    payload.validate().map_err(CliError::Usage)?;
    Ok(payload)
}

/// Parse one word of a closed vocabulary through the record's own serde
/// definition, so the CLI can never accept a word the wire refuses.
///
/// The refusal lists the vocabulary. `clap`'s `ValueEnum` would do this too,
/// but only by putting a `clap` derive on a `buzz-core` wire type, which would
/// make the protocol crate depend on this CLI's argument parser.
fn closed_word<T: serde::de::DeserializeOwned>(
    flag: &str,
    value: Option<&str>,
    vocabulary: &[&str],
) -> Result<Option<T>, CliError> {
    let Some(value) = value else {
        return Ok(None);
    };
    serde_json::from_value::<T>(Value::String(value.to_owned()))
        .map(Some)
        .map_err(|_| CliError::Usage(format!("{flag} must be one of: {}", vocabulary.join(", "))))
}

fn budget_is_set(budget: &CodingSessionPolicyBudget) -> bool {
    budget.turns.is_some()
        || budget.tokens_per_seat.is_some()
        || budget.tokens_per_session.is_some()
        || budget.cost_usd_per_session.is_some()
        || budget.context_tier.is_some()
}

fn gates_are_set(gates: &CodingSessionPolicyGates) -> bool {
    gates.red_first.is_some()
        || gates.review_every_lane.is_some()
        || gates.required_gates.is_some()
        || gates.verifier_required.is_some()
}

fn bench_is_set(bench: &CodingSessionPolicyBench) -> bool {
    bench.identities.is_some()
        || bench.providers.is_some()
        || bench.challenger_sample_rate.is_some()
}

fn stop_is_set(stop: &CodingSessionPolicyStop) -> bool {
    stop.time_box_secs.is_some() || stop.on_milestone.is_some()
}

async fn set(client: &BuzzClient, args: SessionPolicySetArgs) -> Result<(), CliError> {
    validate_coordinates(&args.channel, &args.session_ref, &args.genesis)?;
    let payload = payload_from_args(&args)?;
    publish(client, &args.channel, payload, "set").await
}

async fn clear(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<(), CliError> {
    validate_coordinates(channel, session_ref, genesis)?;
    // The withdrawal record: `{schema, sessionRef, genesisRef}` and nothing
    // else. Under a newest-wins fold it is the only way to take a policy back,
    // so it is a legal record rather than an error, and a reader must render it
    // as "no policy" — never as "policy unknown" (POLICY.md §2.3).
    let payload = CodingSessionPolicyPayload::empty(session_ref, genesis);
    publish(client, channel, payload, "clear").await
}

/// Refuse before signing when this key could not steer the umbrella.
///
/// The **provable** rule, and the only one: the umbrella's founder, or a seat
/// holding an operator grant the relay has accepted. It is evaluated through
/// [`signer_may_steer_at`] — the same function the session provider's fold
/// calls — at *now*, because now is when this record is about to be signed.
///
/// An earlier draft of this command signed anyway for an active `lead` seat
/// holding no grant and disclosed `willNotBind` in its answer. That was wrong
/// twice over: it wrote a permanent, immutable record onto a public relay that
/// **no consumer in this repository will act on**, and the disclosure appeared
/// once, at write time, in one operator's terminal — it does not travel with
/// the record (REVIEW-B2 F2). Refusing costs nothing and is the honest answer.
async fn require_policy_standing(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<(), CliError> {
    let authority =
        super::operations_reads::fetch_session_authority(client, channel, session_ref, genesis)
            .await?;
    let author = client.keys().public_key().to_hex();
    let now = u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(u64::MAX);
    refuse_without_policy_standing(
        &author,
        now,
        &authority.context.founder_pubkey,
        &authority.policy_grants,
    )
}

/// The pure half of [`require_policy_standing`]: the rule, no I/O.
pub(super) fn refuse_without_policy_standing(
    author: &str,
    at: u64,
    founder: &str,
    grants: &[CodingSessionPolicyGrant],
) -> Result<(), CliError> {
    if signer_may_steer_at(author, at, founder, grants) {
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "refusing to publish a session policy: {author} is neither this umbrella's founder \
         ({founder}) nor a seat holding an operator grant, and every consumer folds a policy \
         from anyone else as unauthorized — the record would sit on the relay binding nothing. \
         Ask the founder to publish it, or to grant this seat operator standing \
         (`bee sessions grant`)."
    )))
}

async fn publish(
    client: &BuzzClient,
    channel: &str,
    payload: CodingSessionPolicyPayload,
    verb: &str,
) -> Result<(), CliError> {
    require_policy_standing(client, channel, &payload.session_ref, &payload.genesis_ref).await?;
    let sets_any_policy = payload.sets_any_policy();
    let builder = build_coding_session_policy(channel, payload)
        .map_err(|error| CliError::Usage(error.to_string()))?;
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    client.submit_event(event).await?;
    println!(
        "{}",
        json!({
            "eventId": event_id,
            "accepted": true,
            "verb": verb,
            "setsAnyPolicy": sets_any_policy,
            "enforcement": POLICY_ENFORCEMENT_DISCLOSURE,
        })
    );
    Ok(())
}

async fn get(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
) -> Result<(), CliError> {
    validate_coordinates(channel, session_ref, genesis)?;
    // Authority first: a policy is not "the policy" until somebody with
    // standing signed it, and this command used to skip that entirely.
    let authority =
        super::operations_reads::fetch_session_authority(client, channel, session_ref, genesis)
            .await?;
    let values = client
        .query_all(json!({
            "kinds": [KIND_CODING_SESSION_POLICY],
            "#h": [channel],
            "#d": [session_ref],
            "#csp-genesis": [genesis],
        }))
        .await?;
    let events: Vec<Event> = values
        .into_iter()
        .filter_map(|value| serde_json::from_value::<Event>(value).ok())
        .collect();
    println!(
        "{}",
        policy_json(&fold_policies(
            &events,
            session_ref,
            genesis,
            &authority.context.founder_pubkey,
            &authority.policy_grants,
        ))
    );
    Ok(())
}

/// Fold this umbrella's published 44245s exactly as the session provider does.
///
/// One call into `buzz-core`, with the provider's own standing rule
/// ([`signer_may_steer_at`]) supplied as the predicate. There is deliberately
/// no second implementation here: REVIEW-B2 F1 is what happens when the two
/// surfaces disagree.
pub(super) fn fold_policies(
    events: &[Event],
    session_ref: &str,
    genesis: &str,
    founder: &str,
    grants: &[CodingSessionPolicyGrant],
) -> CodingSessionPolicyFold {
    fold_coding_session_policies(
        events,
        session_ref,
        genesis,
        founder,
        &|author, created_at| signer_may_steer_at(author, created_at, founder, grants),
    )
}

/// Render the fold: the record in force, and every record that is not it.
///
/// `policy` is `null` — never an empty object — when nobody with standing set
/// one, so "nobody set one" and "the policy sets nothing" stay different
/// answers: a withdrawal is a decision somebody made and renders as a real
/// record with `setsAnyPolicy: false`.
///
/// `excluded` lists every refused record with its author, its code and one
/// sentence of reason. It is never empty-by-omission: a stranger who published
/// a competing ceiling into this channel is a fact the reader needs, and
/// dropping it silently would make a stranger's record indistinguishable from
/// no record at all.
pub(super) fn policy_json(fold: &CodingSessionPolicyFold) -> Value {
    json!({
        "policy": fold.selected.as_ref().map(|selected| json!({
            "eventId": selected.event_id,
            "authorPubkey": selected.author,
            "authorIsFounder": selected.author_is_founder,
            "createdAt": selected.created_at,
            "setsAnyPolicy": selected.record.sets_any_policy(),
            "policy": selected.record,
        })),
        "excluded": fold.excluded.iter().map(|item| json!({
            "eventId": item.event_id,
            "authorPubkey": item.author,
            "createdAt": item.created_at,
            "code": item.code.as_str(),
            "reason": item.reason,
        })).collect::<Vec<_>>(),
        "enforcement": POLICY_ENFORCEMENT_DISCLOSURE,
    })
}

#[cfg(test)]
#[path = "policy_tests.rs"]
mod tests;
