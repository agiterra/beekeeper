//! Arm **(C)** after the 2026-09-03 follow-up ruling: a verifier's clearance is
//! *half* of what a verdict-gated ref wants, and the other half is arm (B)'s
//! own evidence — every required gate observed green, by the mission's
//! provider, on the exact commit being pushed, over a clean worktree.
//!
//! # Why this file exists
//!
//! L22 §6.3 left the question open and L27 answers it with the orchestrator's
//! default: **proof scaled to risk means the riskier class gets strictly more
//! proof, not different proof.** A mission whose founder set
//! `gates.verifierRequired: true` asked for a second seat *on top of* the
//! gates, not instead of them — and the rows exist anyway, because the
//! provider signs them whether or not anyone reads them.
//!
//! Before this, the two arms were alternatives and a verifier-required mission
//! was the *weaker* of the two: arm (B) demanded three green gates on the
//! pushed commit and arm (C) demanded none. A founder who tightened the policy
//! got a looser landing.
//!
//! # Why its own file
//!
//! Its fixtures need both halves at once — a kind 44244 assignment → report →
//! disposition → refutation chain **and** folded kind 44246 observations —
//! which neither [`super::arms_tests`] (transactions only) nor
//! [`super::observed_tests`] (observations only) carries. Splitting rather
//! than growing either one keeps every file under the repository's 1,000-line
//! ceiling.

use super::*;

use crate::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
    CodingSessionObservationSource,
};
use crate::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamRefutationDecision, CodingSessionTeamReport,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "c0066ddd-8214-4baf-81d2-3046fead0d32";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79";
const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";
const OTHER_SHA: &str = "1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b1b";
const BRANCH: &str = "whoami/cli";

// ── the 44244 half ───────────────────────────────────────────────────────

fn payload(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    }
}

fn signed(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, created_at: u64) -> Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("payload serializes"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("version tag"),
        Tag::parse(["cstx-genesis", GENESIS]).expect("genesis tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type tag"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("event signs")
}

// ── the 44246 half ───────────────────────────────────────────────────────
//
// Built through `super::gate_fixture` so these cases and the arm-(C) fixtures
// in the sibling files share one construction path — and one provenance fold.

use super::gate_fixture::{default_green, folded, signed_observation};

/// Sign one gate observation for this mission.
fn observation(
    keys: &Keys,
    source: CodingSessionObservationSource,
    rows: Vec<CodingSessionObservationGateRow>,
) -> Event {
    signed_observation(keys, CHANNEL, SESSION, GENESIS, source, rows)
}

// ── the mission both halves belong to ────────────────────────────────────

/// A mission with a lead, a builder, a verifier and a provider: the lead
/// settles the assignment, the verifier fails to refute the report, and the
/// provider watches the gates. Arm (C) now wants all three.
struct Verified {
    founder: Keys,
    lead: Keys,
    builder: Keys,
    verifier: Keys,
    provider: Keys,
    transactions: Vec<Event>,
    seats: Vec<CodingSessionTeamActiveSeat>,
}

fn verified() -> Verified {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let builder = Keys::generate();
    let verifier = Keys::generate();
    let provider = Keys::generate();
    let seats = vec![
        CodingSessionTeamActiveSeat {
            actor_pubkey: lead.public_key().to_hex(),
            role: "lead".into(),
        },
        CodingSessionTeamActiveSeat {
            actor_pubkey: builder.public_key().to_hex(),
            role: "builder".into(),
        },
        CodingSessionTeamActiveSeat {
            actor_pubkey: verifier.public_key().to_hex(),
            role: "verifier".into(),
        },
    ];

    let assignment = signed(
        &payload(CodingSessionTeamTransactionBody::Assignment(
            CodingSessionTeamAssignment {
                assignee_actor: builder.public_key().to_hex(),
                assignee_role: "builder".into(),
                objective: "Land the branch".into(),
                brief: "Build the bounded slice and report.".into(),
                branch: Some(BRANCH.into()),
                base_sha: None,
                file_ownership: vec!["crates/buzz-cli/src".into()],
                acceptance_steps: vec!["cargo test -p buzz-cli".into()],
            },
        )),
        &lead,
        100,
    );
    let report = signed(
        &payload(CodingSessionTeamTransactionBody::Report(
            CodingSessionTeamReport {
                assignment_ref: assignment.id.to_hex(),
                summary: "Done".into(),
                branch: Some(BRANCH.into()),
                base_sha: None,
                head_sha: Some(HEAD_SHA.into()),
                files: Vec::new(),
                tests: Vec::new(),
                red_before_green: None,
                deviations: Vec::new(),
                residuals: Vec::new(),
                anomalies: Vec::new(),
            },
        )),
        &builder,
        200,
    );
    let disposition = signed(
        &payload(CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Disposition {
                assignment_ref: assignment.id.to_hex(),
                report_ref: report.id.to_hex(),
                refutation_ref: None,
                decision: CodingSessionTeamDispositionDecision::Approve,
                summary: "Ruled".into(),
                findings: Vec::new(),
                required_action: None,
            },
        )),
        &lead,
        300,
    );
    let refutation = signed(
        &payload(CodingSessionTeamTransactionBody::Verdict(
            CodingSessionTeamVerdict::Refutation {
                assignment_ref: assignment.id.to_hex(),
                report_ref: report.id.to_hex(),
                decision: CodingSessionTeamRefutationDecision::NotRefuted,
                summary: "Checked".into(),
                findings: Vec::new(),
                required_action: None,
            },
        )),
        &verifier,
        350,
    );

    Verified {
        founder,
        lead,
        builder,
        verifier,
        provider,
        transactions: vec![assignment, report, disposition, refutation],
        seats,
    }
}

/// The candidate the rule judges: the folded transaction chain, plus whatever
/// observations `rows` describes, folded with the provider set supplied so a
/// misclaimed `observed` is downgraded exactly as the relay downgrades it.
fn candidate(
    mission: &Verified,
    observations: Vec<Event>,
    gate_policy: Option<VerdictAdmissionGatePolicy>,
) -> VerdictAdmissionCandidate {
    let context = verdict_admission_fold_context(
        CHANNEL,
        SESSION,
        GENESIS,
        mission.founder.public_key().to_hex(),
        mission.seats.clone(),
    );
    let canonical =
        fold_candidate_records(&mission.transactions, &context).expect("the fixture folds cleanly");
    let gates = folded(&mission.provider, SESSION, GENESIS, &observations);
    VerdictAdmissionCandidate {
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: mission.founder.public_key().to_hex(),
        canonical,
        active_seats: mission.seats.clone(),
        observed_gates: gates,
        gate_policy,
    }
}

/// The policy a verifier-required mission carries — the case this ruling is
/// about, and the one that makes arm (B) silent so arm (C) is the only route.
use super::gate_fixture::verifier_required_policy as verifier_required;

fn query<'a>(pusher: &'a str, founders: &'a [String]) -> VerdictAdmissionQuery<'a> {
    VerdictAdmissionQuery {
        ref_name: "refs/heads/whoami/cli",
        new_oid: HEAD_SHA,
        pusher_pubkey: pusher,
        repo_founders: founders,
        // These cases are about arm (C)'s evidence, not about where the
        // missions were found; the bound channel is the lookup every other
        // arm's fixtures use (L28).
        candidate_source: &super::VERDICT_ADMISSION_BOUND_CHANNEL,
    }
}

/// The green observations every admitting case here shares.
fn watched_green(mission: &Verified) -> Vec<Event> {
    vec![observation(
        &mission.provider,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )]
}

// ── the ruling ───────────────────────────────────────────────────────────

/// Both halves present: the lead settled it, the verifier failed to break it,
/// and the provider watched every required gate pass on this exact commit.
/// A seat lands it.
#[test]
fn c_a_clearance_plus_observed_green_rows_admits_a_seat_push() {
    let mission = verified();
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(
            &mission,
            watched_green(&mission),
            verifier_required(),
        )],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    assert!(
        outcome.is_admitted(),
        "a verifier clearance over gates observed green on the pushed commit is the landing, \
         got {outcome:?}"
    );
}

/// The half this ruling adds. A verifier cleared the commit and **no** gate row
/// names it: before L27 this landed, which made a verifier-required mission the
/// weaker of the two arms.
#[test]
fn c_a_clearance_with_no_gate_rows_at_all_is_refused_and_names_the_rows() {
    let mission = verified();
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission, Vec::new(), verifier_required())],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    let VerdictAdmission::Refused(VerdictAdmissionRefusal::VerifiedButGatesNotGreen {
        verifier,
        gates,
        ..
    }) = &outcome
    else {
        panic!("a cleared commit with no observed gate row must not land, got {outcome:?}");
    };
    assert_eq!(verifier, &mission.verifier.public_key().to_hex());
    assert!(
        matches!(
            **gates,
            VerdictAdmissionRefusal::RequiredGateNotObserved { .. }
        ),
        "a commit no row names is `no such row`, never `0 declared rows`: {gates:?}"
    );
    let refusal = match &outcome {
        VerdictAdmission::Refused(refusal) => refusal,
        other => panic!("{other:?}"),
    };
    let reason = refusal.reason();
    assert!(
        reason.contains(&mission.verifier.public_key().to_hex()),
        "the refusal names the half that *is* satisfied: {reason}"
    );
    for gate in DEFAULT_REQUIRED_GATES {
        assert!(
            reason.contains(gate),
            "the refusal names the rows it wanted: {reason}"
        );
    }
}

/// A verifier cleared it and a gate was observed **red** on the same commit.
/// The sentence the spec asks for: *"verifier cleared <sha> but gate
/// `cargo test` was observed red on it."*
#[test]
fn c_a_clearance_over_a_red_gate_is_refused_and_names_the_gate() {
    let mission = verified();
    let mut rows = default_green(HEAD_SHA);
    rows[2].outcome = CodingSessionObservationGateOutcome::Failed;
    let observations = vec![observation(
        &mission.provider,
        CodingSessionObservationSource::Observed,
        rows,
    )];
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission, observations, verifier_required())],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    let VerdictAdmission::Refused(refusal) = &outcome else {
        panic!("a red gate on the pushed commit must not land, got {outcome:?}");
    };
    assert!(
        matches!(
            refusal,
            VerdictAdmissionRefusal::VerifiedButGatesNotGreen { gates, .. }
                if matches!(**gates, VerdictAdmissionRefusal::ObservedGateRed { .. })
        ),
        "the refusal carries arm (B)'s own red-gate sentence whole: {refusal:?}"
    );
    let reason = refusal.reason();
    assert!(
        reason.contains(&format!(
            "gate `{}` was observed red",
            DEFAULT_REQUIRED_GATES[2]
        )),
        "the refusal names the red gate: {reason}"
    );
    assert!(
        reason.contains(&mission.verifier.public_key().to_hex()),
        "and names the verifier who cleared it, so neither half is hidden: {reason}"
    );
}

/// Rows the **seat** signed are `declared` after the provenance fold, and a
/// verifier's clearance does not launder them into evidence.
#[test]
fn c_a_clearance_over_rows_the_seat_declared_is_refused() {
    let mission = verified();
    let observations = vec![observation(
        &mission.builder,
        CodingSessionObservationSource::Observed,
        default_green(HEAD_SHA),
    )];
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission, observations, verifier_required())],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    assert!(
        !outcome.is_admitted(),
        "a self-declared row is its own subject speaking, whoever cleared the report; got \
         {outcome:?}"
    );
}

/// Green rows naming a **different** commit admit nothing, exactly as under arm
/// (B): finding 27's shape does not become safe because a verifier ruled.
#[test]
fn c_a_clearance_over_rows_for_another_commit_is_refused() {
    let mission = verified();
    let observations = vec![observation(
        &mission.provider,
        CodingSessionObservationSource::Observed,
        default_green(OTHER_SHA),
    )];
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission, observations, verifier_required())],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    assert!(
        !outcome.is_admitted(),
        "green somewhere else is not green here, got {outcome:?}"
    );
}

/// A founder's push is untouched: arm (A) is answered before any mission is
/// read, and gate rows are not among the facts it consults.
#[test]
fn c_the_new_half_does_not_reach_a_founder_push() {
    let mission = verified();
    let founder = mission.founder.public_key().to_hex();
    assert!(
        evaluate_verdict_admission(
            &[candidate(&mission, Vec::new(), verifier_required())],
            &query(&founder, std::slice::from_ref(&founder)),
        )
        .is_admitted(),
        "humans never gate a landing"
    );
}

/// The lead may land it too — the commit is what was ruled on, not the carrier.
#[test]
fn c_any_seat_lands_a_cleared_and_watched_commit() {
    let mission = verified();
    let founder = mission.founder.public_key().to_hex();
    let lead = mission.lead.public_key().to_hex();
    assert!(
        evaluate_verdict_admission(
            &[candidate(
                &mission,
                watched_green(&mission),
                verifier_required()
            )],
            &query(&lead, std::slice::from_ref(&founder)),
        )
        .is_admitted(),
        "any active seat may land what both halves cleared"
    );
}

/// The admitting evidence names **both** halves, so no surface can render
/// "a verifier cleared it" over an arm that also checked three gates.
#[test]
fn c_the_evidence_names_the_gates_as_well_as_the_verifier() {
    let mission = verified();
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(
            &mission,
            watched_green(&mission),
            verifier_required(),
        )],
        &query(&pusher, std::slice::from_ref(&founder)),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
            gates,
            row_event_ids,
            verifier_pubkey,
            ..
        }) => {
            assert_eq!(gates, DEFAULT_REQUIRED_GATES.to_vec());
            assert_eq!(
                row_event_ids.len(),
                gates.len(),
                "one readable row behind each gate"
            );
            assert_eq!(verifier_pubkey, mission.verifier.public_key().to_hex());
        }
        other => panic!("expected a verifier verdict admission, got {other:?}"),
    }
}

/// The refusal's copy, checked as copy: it names both halves, never repeats
/// the ref, and never invents a remedy the rule cannot deliver.
#[test]
fn c_the_new_refusal_names_both_halves_and_never_repeats_the_ref() {
    let reason = VerdictAdmissionRefusal::VerifiedButGatesNotGreen {
        new_oid: HEAD_SHA.into(),
        verifier: "0".repeat(64),
        gates: Box::new(VerdictAdmissionRefusal::ObservedGateRed {
            gate: "cargo test".into(),
            new_oid: HEAD_SHA.into(),
        }),
    }
    .reason();
    assert!(
        reason.starts_with(&format!("verifier {} cleared {HEAD_SHA}", "0".repeat(64))),
        "the satisfied half comes first, so a reader is not sent looking for a verifier they \
         already have: {reason}"
    );
    assert!(
        reason.contains("gate `cargo test` was observed red"),
        "and arm (B)'s own sentence is carried whole, not paraphrased: {reason}"
    );
    assert!(
        !reason.contains("refs/heads/"),
        "the renderer prefixes the ref; {reason}"
    );
}
