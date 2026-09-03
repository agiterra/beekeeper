//! The two arms a verdict-gated push is admitted by, after Brian's ruling of
//! 2026-09-03: **(A)** the pusher is a founder, and **(C)** a lead's approving
//! disposition over a report naming the pushed commit, *cleared* by an
//! independent verifier seat's `not-refuted` refutation of that same report.
//!
//! A sibling of [`super::tests`] rather than more of it, because that file was
//! already 918 lines and the repository's ceiling is 1,000.
//!
//! # The arm that is not here
//!
//! Arm **(B)** — provider-observed gate rows green on the pushed SHA — was
//! built on 2026-09-03 once the gate row could name a commit, and its cases
//! live in [`super::observed_tests`], whose fixtures are kind 44246
//! observations rather than kind 44244 transactions.

use super::*;

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
const BRANCH: &str = "whoami/cli";

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

/// A mission with a builder, a lead and a verifier — the shape arm (C) judges.
struct Mission {
    founder: Keys,
    lead: Keys,
    builder: Keys,
    verifier: Keys,
    events: Vec<Event>,
    seats: Vec<CodingSessionTeamActiveSeat>,
}

/// Build the mission. The **lead** always settles the assignment with an
/// approving disposition — the only author the governance fold authorises for
/// that verb (`coding_session_team_transaction_fold.rs:712`) — and `clearing`
/// says who, if anyone, published the `not-refuted` refutation arm (C) needs.
fn mission(clearing: Option<Clearing>) -> Mission {
    let founder = Keys::generate();
    let lead = Keys::generate();
    let builder = Keys::generate();
    let verifier = Keys::generate();
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

    let mut events = vec![assignment.clone(), report.clone(), disposition];
    if let Some(clearing) = clearing {
        let (signer, decision) = match clearing {
            Clearing::Verifier => (&verifier, CodingSessionTeamRefutationDecision::NotRefuted),
            Clearing::VerifierConfirmedTheFailure => {
                (&verifier, CodingSessionTeamRefutationDecision::Confirmed)
            }
            Clearing::Builder => (&builder, CodingSessionTeamRefutationDecision::NotRefuted),
        };
        events.push(signed(
            &payload(CodingSessionTeamTransactionBody::Verdict(
                CodingSessionTeamVerdict::Refutation {
                    assignment_ref: assignment.id.to_hex(),
                    report_ref: report.id.to_hex(),
                    decision,
                    summary: "Checked".into(),
                    findings: Vec::new(),
                    required_action: None,
                },
            )),
            signer,
            350,
        ));
    }

    Mission {
        founder,
        lead,
        builder,
        verifier,
        events,
        seats,
    }
}

/// Who published the refutation that would clear the report, when one exists.
#[derive(Clone, Copy)]
enum Clearing {
    /// The verifier seat failed to refute it — arm (C)'s clearing record.
    Verifier,
    /// The verifier seat found the failure: a ruling, and one against.
    VerifierConfirmedTheFailure,
    /// The report's own author, seated as the verifier in the test that needs
    /// it, clearing their own work.
    Builder,
}

fn candidate(mission: &Mission) -> VerdictAdmissionCandidate {
    let context = verdict_admission_fold_context(
        CHANNEL,
        SESSION,
        GENESIS,
        mission.founder.public_key().to_hex(),
        mission.seats.clone(),
    );
    let canonical =
        fold_candidate_records(&mission.events, &context).expect("the fixture folds cleanly");
    VerdictAdmissionCandidate {
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: mission.founder.public_key().to_hex(),
        canonical,
        active_seats: mission.seats.clone(),
        // L27: arm (C) requires arm (B)'s evidence on top of the clearance, so
        // an arm-(C) fixture that publishes no gate row now tests the *new*
        // refusal rather than the verdict it was written for. Both halves are
        // supplied here; the cases that are about a missing half live in
        // `super::verified_tests`, which varies them one at a time.
        observed_gates: super::gate_fixture::observed_green_gates(
            CHANNEL, SESSION, GENESIS, HEAD_SHA,
        ),
        // And `verifierRequired`, so arm (B) stays silent and the arm that
        // answers is the one under test.
        gate_policy: super::gate_fixture::verifier_required_policy(),
    }
}

fn query<'a>(
    pusher: &'a str,
    founders: &'a [String],
    new_oid: &'a str,
) -> VerdictAdmissionQuery<'a> {
    VerdictAdmissionQuery {
        ref_name: "refs/heads/whoami/cli",
        new_oid,
        pusher_pubkey: pusher,
        repo_founders: founders,
    }
}

// ── arm (A): a founder's push ────────────────────────────────────────────

/// The ruling's first sentence: *"humans never gate a landing."* A founder
/// pushing a verdict-gated ref is admitted with **no verdict at all** — the
/// fixture here has an assignment and a report and no disposition of any kind.
#[test]
fn a_founder_push_admits_with_no_verdict_at_all() {
    let mission = mission(None);
    let founder = mission.founder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&founder, std::slice::from_ref(&founder), HEAD_SHA),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::FounderPush { pusher_pubkey }) => {
            assert_eq!(pusher_pubkey, founder);
        }
        other => panic!("expected a founder push admission, got {other:?}"),
    }
}

/// Arm (A) does not need a mission at all. A founder pushing a commit no
/// mission has ever mentioned still lands: the gate is not for founders.
#[test]
fn a_founder_push_admits_with_no_missions_at_all() {
    let founder = Keys::generate().public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[],
        &query(&founder, std::slice::from_ref(&founder), HEAD_SHA),
    );
    assert!(
        outcome.is_admitted(),
        "a founder push must not depend on a mission being readable, got {outcome:?}"
    );
}

/// A co-founder who founded no mission here is still a founder of the
/// repository. Finding 33's first half, now unconditional.
#[test]
fn a_co_founder_push_admits() {
    let mission = mission(None);
    let co_founder = Keys::generate().public_key().to_hex();
    let founders = vec![mission.founder.public_key().to_hex(), co_founder.clone()];
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&co_founder, &founders, HEAD_SHA),
    );
    assert!(outcome.is_admitted(), "expected admission, got {outcome:?}");
}

// ── arm (C): a verifier's verdict ────────────────────────────────────────

/// The machine verdict that replaces review: a seat holding role `verifier`
/// approved a report whose `headSha` is the commit, and a seat lands it.
#[test]
fn c_a_verifier_clearance_over_the_pushed_head_sha_admits_a_seat_push() {
    let mission = mission(Some(Clearing::Verifier));
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&founder), HEAD_SHA),
    );
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
            head_sha,
            verifier_pubkey,
            session_ref,
            refutation_event_id,
            ..
        }) => {
            assert_eq!(head_sha, HEAD_SHA);
            assert_eq!(session_ref, SESSION);
            assert_eq!(verifier_pubkey, mission.verifier.public_key().to_hex());
            assert!(
                !refutation_event_id.is_empty(),
                "the evidence names the record that cleared it"
            );
        }
        other => panic!("expected a verifier verdict admission, got {other:?}"),
    }

    // Any active seat may land it, not only the one that wrote the report:
    // the verdict is about the commit, not about who carries it.
    let lead = mission.lead.public_key().to_hex();
    assert!(
        evaluate_verdict_admission(
            &[candidate(&mission)],
            &query(&lead, std::slice::from_ref(&founder), HEAD_SHA),
        )
        .is_admitted(),
        "the lead holds an active seat of this mission"
    );
}

/// The report's author approving their own report is not a verdict, whatever
/// seat they hold. Live run 3's failure mode was a report nobody independent
/// had read; a self-approval reproduces it exactly.
#[test]
fn c_a_builder_clearing_their_own_report_does_not_admit() {
    let mission = mission(Some(Clearing::Builder));
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&founder), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "a self-approved report must not admit, got {outcome:?}"
    );
}

/// The report's author holding the **verifier** seat is the same defect with
/// a better title. `completion_not_verified` accepts this shape (its case (b),
/// `coding_session_completion_verification.rs:118`); the push gate does not,
/// and the divergence is deliberate — a completion is a claim about work, a
/// push is the work.
#[test]
fn c_a_verifier_clearing_their_own_report_does_not_admit() {
    let mut mission = mission(Some(Clearing::Builder));
    // One key, both roles: the builder wrote the report and holds the seat
    // that is supposed to check it.
    let builder_hex = mission.builder.public_key().to_hex();
    mission
        .seats
        .retain(|seat| seat.actor_pubkey != builder_hex);
    mission.seats.push(CodingSessionTeamActiveSeat {
        actor_pubkey: builder_hex.clone(),
        role: "verifier".into(),
    });
    let founder = mission.founder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&builder_hex, std::slice::from_ref(&founder), HEAD_SHA),
    );
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::VerifierIsTheReportAuthor {
            verifier,
            ..
        }) => assert_eq!(verifier, builder_hex),
        other => panic!("expected VerifierIsTheReportAuthor, got {other:?}"),
    }
}

/// An approving disposition with nobody behind it does not admit. The lead
/// settled the assignment; settling is not checking.
#[test]
fn c_an_approval_no_verifier_cleared_does_not_admit_a_seat_push() {
    let mission = mission(None);
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&founder), HEAD_SHA),
    );
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::ApprovedButNotVerified {
            new_oid,
            approvals,
        }) => {
            assert_eq!(new_oid, HEAD_SHA);
            assert_eq!(approvals, 1);
        }
        other => panic!("expected ApprovedButNotVerified, got {other:?}"),
    }
}

/// `confirmed` is a ruling too, and it is a ruling *against*: a verifier who
/// found the failure has cleared nothing. Neither has one who was `blocked`.
#[test]
fn c_a_verifier_who_confirmed_the_failure_admits_nothing() {
    let mission = mission(Some(Clearing::VerifierConfirmedTheFailure));
    let founder = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&founder), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "a confirmed refutation is not a clearance, got {outcome:?}"
    );
}

/// A verified commit still needs a key that belongs to the mission. A stranger
/// holding the same patch is not one of its seats.
#[test]
fn c_a_verified_commit_pushed_by_a_stranger_is_refused() {
    let mission = mission(Some(Clearing::Verifier));
    let founder = mission.founder.public_key().to_hex();
    let stranger = Keys::generate().public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&stranger, std::slice::from_ref(&founder), HEAD_SHA),
    );
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::PushNotSeated { seats, .. }) => {
            assert_eq!(
                seats, 3,
                "the refusal discloses how many seats the mission has"
            );
        }
        other => panic!("expected PushNotSeated, got {other:?}"),
    }
}

/// Every refusal this lane adds names the arm and the nearest missing fact,
/// and none of them invents a remedy or repeats the ref.
#[test]
fn the_new_refusals_name_the_arm_and_never_repeat_the_ref() {
    let refusals = [
        VerdictAdmissionRefusal::ApprovedButNotVerified {
            new_oid: HEAD_SHA.into(),
            approvals: 1,
        },
        VerdictAdmissionRefusal::VerifierIsTheReportAuthor {
            new_oid: HEAD_SHA.into(),
            verifier: "0".repeat(64),
        },
        VerdictAdmissionRefusal::PushNotSeated {
            new_oid: HEAD_SHA.into(),
            session_ref: SESSION.into(),
            seats: 3,
        },
    ];
    for refusal in refusals {
        let reason = refusal.reason();
        assert!(
            !reason.contains("refs/heads/"),
            "the renderer prefixes the ref; {reason}"
        );
        assert!(
            reason.contains("verifier") || reason.contains("seat"),
            "a refusal must name the arm it fell short of; {reason}"
        );
    }
}
