//! Unit tests for [`super`] — the pure half of the verdict-gated push rule.
//!
//! Fixtures are real signed events folded by the real fold, because the rule
//! is defined against canonical records and a hand-built "canonical" list
//! would test a different function than the relay runs.

use super::*;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionCompleted, CodingSessionTeamRefutationDecision,
    CodingSessionTeamReport, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

const CHANNEL: &str = "c0066ddd-8214-4baf-81d2-3046fead0d32";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79c3b8ae79";
const HEAD_SHA: &str = "07c470be007c470be007c470be007c470be007c4";
const OTHER_SHA: &str = "0123456789012345678901234567890123456789";

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

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: actor.public_key().to_hex(),
            assignee_role: "builder".into(),
            objective: "Land the branch".into(),
            brief: "Build the bounded slice and report.".into(),
            branch: Some("whoami/cli".into()),
            base_sha: None,
            file_ownership: vec!["crates/buzz-cli/src".into()],
            acceptance_steps: vec!["cargo test -p buzz-cli".into()],
        },
    ))
}

fn report(
    assignment_ref: &str,
    branch: Option<&str>,
    head_sha: Option<&str>,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Done".into(),
            branch: branch.map(str::to_string),
            base_sha: None,
            head_sha: head_sha.map(str::to_string),
            files: Vec::new(),
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    ))
}

fn disposition(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            refutation_ref: None,
            decision,
            summary: "Ruled".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

/// A verifier's `not-refuted` refutation — arm (C)'s clearing record.
fn refutation(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamRefutationDecision,
) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Refutation {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            decision,
            summary: "Could not break it".into(),
            findings: Vec::new(),
            required_action: None,
        },
    ))
}

fn completed(assignment_ref: &str, landed: &str) -> CodingSessionTeamTransactionPayload {
    payload(CodingSessionTeamTransactionBody::MissionCompleted(
        CodingSessionTeamMissionCompleted {
            assignment_refs: vec![assignment_ref.into()],
            landed_shas: vec![landed.into()],
            summary: "Landed".into(),
            follow_ups: Vec::new(),
        },
    ))
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

/// One mission shaped like live run 3: the founder seats a lead, the lead
/// assigns, the builder reports, and someone rules on the report.
struct Mission {
    founder: Keys,
    lead: Keys,
    builder: Keys,
    verifier: Keys,
    events: Vec<Event>,
    seats: Vec<CodingSessionTeamActiveSeat>,
}

fn mission(
    head_sha: Option<&str>,
    branch: Option<&str>,
    decision: CodingSessionTeamDispositionDecision,
    ruled_by_lead: bool,
) -> Mission {
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
    let assignment = signed(&assignment(&builder), &lead, 100);
    let report = signed(
        &report(&assignment.id.to_hex(), branch, head_sha),
        &builder,
        200,
    );
    // A `disposition` is `may_lead` in the governance fold
    // (`coding_session_team_transaction_fold.rs:712`), so the ruler is the
    // lead or the founder — never the verifier, whose disposition the fold
    // would exclude `Unauthorized`.
    let ruler = if ruled_by_lead { &lead } else { &founder };
    let disposition = signed(
        &disposition(&assignment.id.to_hex(), &report.id.to_hex(), decision),
        ruler,
        300,
    );
    // Arm (C)'s second record: the verifier independently fails to refute the
    // same report (`:709` is the rule that authorises this one).
    let cleared = signed(
        &refutation(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            CodingSessionTeamRefutationDecision::NotRefuted,
        ),
        &verifier,
        350,
    );
    let completion = signed(&completed(&assignment.id.to_hex(), HEAD_SHA), &lead, 400);
    Mission {
        founder,
        lead,
        builder,
        verifier,
        events: vec![assignment, report, disposition, cleared, completion],
        seats,
    }
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

/// A push of `new_oid` to the branch the fixture's report names.
///
/// The default is the mission's own branch because that is the update an
/// approval is *for*; pushing the same commit somewhere else is a separate
/// question with its own test.
fn query<'a>(
    pusher: &'a str,
    founders: &'a [String],
    new_oid: &'a str,
) -> VerdictAdmissionQuery<'a> {
    query_on("refs/heads/whoami/cli", pusher, founders, new_oid)
}

fn query_on<'a>(
    ref_name: &'a str,
    pusher: &'a str,
    founders: &'a [String],
    new_oid: &'a str,
) -> VerdictAdmissionQuery<'a> {
    VerdictAdmissionQuery {
        ref_name,
        new_oid,
        pusher_pubkey: pusher,
        repo_founders: founders,
        // These cases are about the rule, not the lookup; the source only
        // shapes the sentence one refusal renders.
        candidate_source: &super::VERDICT_ADMISSION_BOUND_CHANNEL,
    }
}

/// The `(sessionRef, headSha)` an arm-(C) admission stood on.
///
/// [`VerdictAdmissionEvidence`] became an enum with the 2026-09-03 ruling —
/// a founder push stands on no report at all — so a test that wants the
/// verdict's own facts has to say which arm it expected.
fn verifier_evidence(outcome: VerdictAdmission) -> (String, String) {
    match outcome {
        VerdictAdmission::Admitted(VerdictAdmissionEvidence::VerifierVerdict {
            session_ref,
            head_sha,
            ..
        }) => (session_ref, head_sha),
        other => panic!("expected a verifier verdict admission, got {other:?}"),
    }
}

// ── the rule ─────────────────────────────────────────────────────────────

#[test]
fn founder_signed_approval_over_the_pushed_head_sha_admits() {
    let mission = mission(
        Some(HEAD_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let (session_ref, head_sha) = verifier_evidence(outcome);
    assert_eq!(head_sha, HEAD_SHA);
    assert_eq!(session_ref, SESSION);
}

#[test]
fn approve_with_notes_admits_and_changes_requested_refuses() {
    for (decision, admits) in [
        (CodingSessionTeamDispositionDecision::ApproveWithNotes, true),
        (
            CodingSessionTeamDispositionDecision::ChangesRequested,
            false,
        ),
        (CodingSessionTeamDispositionDecision::Reject, false),
        (CodingSessionTeamDispositionDecision::Blocked, false),
    ] {
        let mission = mission(Some(HEAD_SHA), None, decision, false);
        let owner = mission.founder.public_key().to_hex();
        let pusher = mission.builder.public_key().to_hex();
        let outcome = evaluate_verdict_admission(
            &[candidate(&mission)],
            &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
        );
        assert_eq!(
            outcome.is_admitted(),
            admits,
            "{decision:?} admission should be {admits}"
        );
    }
}

/// Live run 3's own shape: the verdict was `changes-requested` and the commit
/// reached `main` anyway.
#[test]
fn the_run_three_fixture_refuses_with_the_frozen_copy() {
    let mission = mission(
        Some(HEAD_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::ChangesRequested,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("a changes-requested verdict must not admit its own commit");
    };
    assert_eq!(
        refusal.reason(),
        format!(
            "require-verdict is set and no mission verdict names this commit: no approved report \
             names {HEAD_SHA}. This key holds no seat in the newest 512 authority transitions \
             this relay could read, and this repository names no project, so the search fell \
             back to the channel it is bound to. Searched 1 mission(s) — the \
             newest 16 on that channel whose founder is a founder of this repository — over one \
             shared page of the newest 512 team transactions on it. An older ruling can fall \
             outside both. No observed gate row names {HEAD_SHA} either, so the gate-row route \
             is not open for it: that route wants every required gate published green on this \
             exact commit, by the mission's own provider, over a clean worktree. Run each gate \
             as its own command so the host can record it."
        ),
        "both caps are named in words, not just implied by the mission count (fix round 1, F4)"
    );
}

#[test]
fn a_report_naming_only_a_branch_never_admits() {
    let mission = mission(
        None,
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("a branch name is not a commit");
    };
    assert_eq!(
        refusal,
        VerdictAdmissionRefusal::ApprovedReportNamesBranchOnly
    );
    assert_eq!(
        refusal.reason(),
        "require-verdict is set and no mission verdict names this commit: the approved report for \
         this work names a branch and no headSha, and a branch name is not a commit."
    );
}

/// The completion's `landedShas` is the lead's own claim — finding 27 caught
/// exactly that claim being false, so it can never be what admits a push.
#[test]
fn landed_shas_never_admit() {
    let mission = mission(
        Some(OTHER_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    // The completion in the fixture names HEAD_SHA in `landedShas`.
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "landedShas is a claim, not a ruling"
    );
}

#[test]
fn a_disposition_in_a_stranger_umbrella_never_admits() {
    let mission = mission(
        Some(HEAD_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let stranger = Keys::generate().public_key().to_hex();
    let founder = mission.founder.public_key().to_hex();
    // Pushed by a **seat** of the mission: the repository's own founder is a
    // stranger to it, so no candidate survives the founder check. Pushing as
    // that founder would be admitted by arm (A) and would test nothing.
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        // The repository is owned by someone who did not found this mission.
        &query(&pusher, std::slice::from_ref(&stranger), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "founder {founder} owns no repo here"
    );
    match outcome {
        VerdictAdmission::Refused(VerdictAdmissionRefusal::NoApprovingVerdict {
            searched_sessions,
            ..
        }) => assert_eq!(
            searched_sessions, 1,
            "the bound is disclosed, not the match"
        ),
        other => panic!("unexpected {other:?}"),
    }
}

/// A 64-hex event id and a 40-hex object id are different names. Comparing
/// whole is what stops a report naming an event from admitting a commit.
#[test]
fn a_sixty_four_hex_head_sha_does_not_admit_a_forty_hex_push() {
    let long = "07c470be007c470be007c470be007c470be007c470be007c470be007c470be00";
    let mission = mission(
        Some(long),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    assert!(!outcome.is_admitted(), "a prefix is not a match");
}

/// The wire refuses an uppercase `headSha` outright, so the folding that can
/// matter is on the pushed oid: the hook's own validator accepts either case.
#[test]
fn head_sha_matching_is_case_folded() {
    let mission = mission(
        Some(HEAD_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let pushed = HEAD_SHA.to_ascii_uppercase();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), &pushed),
    );
    let (_, head_sha) = verifier_evidence(outcome);
    assert_eq!(head_sha, HEAD_SHA, "the report's own bytes");
}

// ── ruling 1: the pusher ─────────────────────────────────────────────────

/// Since the 2026-09-03 ruling a verifier's verdict admits **any active seat**
/// of that mission — the two `VerdictAdmissionRules` flags this section used
/// to toggle are gone, because the ruling decided both of the questions they
/// deferred. What is left is the boundary they were protecting: seats yes,
/// strangers no.
#[test]
fn any_active_seat_lands_what_a_verifier_cleared_and_a_stranger_does_not() {
    let mission = mission(
        Some(HEAD_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    for (who, seat) in [
        ("the builder", mission.builder.public_key().to_hex()),
        ("the lead", mission.lead.public_key().to_hex()),
        ("the verifier", mission.verifier.public_key().to_hex()),
    ] {
        assert!(
            evaluate_verdict_admission(
                &[candidate(&mission)],
                &query(&seat, std::slice::from_ref(&owner), HEAD_SHA),
            )
            .is_admitted(),
            "{who} holds an active seat of the mission whose verifier cleared this commit"
        );
    }

    let stranger = Keys::generate().public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&stranger, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("a key holding no seat in this umbrella lands nothing");
    };
    assert_eq!(
        refusal.reason(),
        format!(
            "commit {HEAD_SHA} carries a verifier's verdict on mission {SESSION}, and this key \
             is not an active seat of it (3 seat(s)). A founder of this repository may land it, \
             or a seat of that mission may."
        )
    );
}

/// An approval with no verifier behind it does not admit a seat's push,
/// whoever signed the approval. The refusal says which fact is missing.
#[test]
fn an_approval_no_verifier_cleared_does_not_admit_a_seat_push() {
    let mut mission = mission(
        Some(HEAD_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        true,
    );
    // Drop the verifier's refutation: the lead approved and nobody checked.
    mission.events.retain(|event| {
        !matches!(
            validate_coding_session_team_transaction_envelope(event)
                .expect("fixture decodes")
                .body,
            CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation { .. })
        )
    });
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("only a verifier's verdict admits a seat's push");
    };
    assert!(
        refusal
            .reason()
            .contains("no active verifier seat has cleared the report it approves"),
        "{}",
        refusal.reason()
    );
}

// ── copy and bounds ──────────────────────────────────────────────────────

#[test]
fn an_unbound_repository_says_so_rather_than_searching() {
    assert_eq!(
        VerdictAdmissionRefusal::RepositoryUnbound.reason(),
        "require-verdict is set and there is nowhere to look for a mission verdict: this key \
         holds no seat in the newest 512 authority transitions this relay could read, this \
         repository names no project, and it is bound to no channel. Remove the rule, put the \
         repository in a project, or bind it to the mission's channel.",
        "finding 56: three lookups can be empty, and the copy names all three"
    );
}

/// The refusal that reports a *count* also reports what was counted. Before
/// finding 56 the sentence said "the newest 16 on this channel" whatever the
/// caller had actually searched, which is how live run 4's seat read a
/// truthful count of a pointless search.
#[test]
fn the_refusal_names_the_lookup_that_found_the_missions() {
    let seat = VerdictAdmissionCandidateSource::SeatOfMission {
        seat: "0123abcd".to_owned(),
        seats: 2,
    };
    let clause = seat.searched_clause(2);
    assert!(clause.starts_with("Searched 2 mission(s)"), "{clause}");
    assert!(clause.contains("that seat 0123abcd"), "{clause}");

    let project = VerdictAdmissionCandidateSource::ProjectSessions {
        project: "30621:ab:beekeeper".to_owned(),
        channels: 3,
    };
    let clause = project.searched_clause(5);
    assert!(clause.contains("This key holds no seat"), "{clause}");
    assert!(
        clause.contains("3 session channel(s) of 30621:ab:beekeeper"),
        "{clause}"
    );

    let mission = VerdictAdmissionCandidateSource::ThisMission {
        session_ref: "aa58f6a2".to_owned(),
    };
    assert_eq!(
        mission.searched_clause(1),
        "Searched only mission aa58f6a2, the one this screen is showing.",
        "a screen looking at one mission must not imply it swept anything wider"
    );
}

#[test]
fn no_candidates_discloses_a_zero_search() {
    let owner = Keys::generate().public_key().to_hex();
    // Not the founder: arm (A) admits a founder before any search happens,
    // and this test is about what the search says when it finds nothing.
    let pusher = Keys::generate().public_key().to_hex();
    let outcome =
        evaluate_verdict_admission(&[], &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA));
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("nothing to search admits nothing");
    };
    assert!(
        refusal.reason().contains("Searched 0 mission(s)"),
        "{}",
        refusal.reason()
    );
    assert!(
        !refusal.reason().contains("refs/heads/"),
        "the reason never repeats the ref; the renderer prefixes it"
    );
}

#[test]
fn the_search_bounds_are_the_ones_the_refusal_discloses() {
    assert_eq!(VERDICT_ADMISSION_MAX_SESSIONS, 16);
    assert_eq!(VERDICT_ADMISSION_MAX_TRANSACTIONS, 512);
}

/// An excluded record is not evidence: a disposition the fold rejected as
/// unauthorized cannot admit a push. Here an outsider signs the ruling.
#[test]
fn an_unauthorized_disposition_is_not_canonical_and_never_admits() {
    let founder = Keys::generate();
    let builder = Keys::generate();
    let outsider = Keys::generate();
    let seats = vec![CodingSessionTeamActiveSeat {
        actor_pubkey: builder.public_key().to_hex(),
        role: "builder".into(),
    }];
    let assignment = signed(&assignment(&builder), &founder, 100);
    let report = signed(
        &report(&assignment.id.to_hex(), None, Some(HEAD_SHA)),
        &builder,
        200,
    );
    let ruling = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &outsider,
        300,
    );
    let context = verdict_admission_fold_context(
        CHANNEL,
        SESSION,
        GENESIS,
        founder.public_key().to_hex(),
        seats,
    );
    let canonical = fold_candidate_records(&[assignment, report, ruling], &context)
        .expect("the fixture folds cleanly");
    let owner = founder.public_key().to_hex();
    let pusher = builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[VerdictAdmissionCandidate {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            founder_pubkey: owner.clone(),
            canonical,
            active_seats: Vec::new(),
            observed_gates: Vec::new(),
            gate_policy: None,
        }],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "the fold excluded the ruling; the search reads the fold, not the events"
    );
}

// ── fix round 1: an approval is scoped to the branch its report named (F7) ──

/// Approving a commit *for a branch* is not approval to put it anywhere.
///
/// Before fix round 1, `VerdictAdmissionQuery::ref_name` was carried into the
/// rule and never read, so one approval admitted the same commit onto every
/// gated ref forever — including a rollback of `main` to an older approved
/// state. The refusal names the branch the report actually approved.
#[test]
fn an_approval_is_scoped_to_the_branch_its_report_named() {
    let mission = mission(
        Some(HEAD_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();

    // The branch the report named — admitted, by short ref and by full ref.
    for ref_name in ["refs/heads/whoami/cli", "whoami/cli"] {
        assert!(
            evaluate_verdict_admission(
                &[candidate(&mission)],
                &query_on(ref_name, &pusher, std::slice::from_ref(&owner), HEAD_SHA),
            )
            .is_admitted(),
            "{ref_name} is the branch the approved report named"
        );
    }

    // Any other ref — refused, and the refusal says which branch was approved.
    for ref_name in ["refs/heads/main", "refs/heads/release", "refs/tags/v9"] {
        let outcome = evaluate_verdict_admission(
            &[candidate(&mission)],
            &query_on(ref_name, &pusher, std::slice::from_ref(&owner), HEAD_SHA),
        );
        let VerdictAdmission::Refused(refusal) = outcome else {
            panic!("an approval for whoami/cli must not admit {ref_name}");
        };
        assert_eq!(
            refusal,
            VerdictAdmissionRefusal::ApprovedForAnotherRef {
                new_oid: HEAD_SHA.to_string(),
                approved_branch: "whoami/cli".to_string(),
            },
            "the refusal names the branch the verdict was actually for"
        );
        assert_eq!(
            refusal.reason(),
            format!(
                "commit {HEAD_SHA} is approved for whoami/cli, and this is a different ref. A \
                 verdict admits a commit to the branch its report named."
            )
        );
    }
}

/// A `main`-shaped branch prefix must not be matched loosely.
#[test]
fn branch_scoping_matches_whole_names_only() {
    let mission = mission(
        Some(HEAD_SHA),
        Some("main"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    assert!(evaluate_verdict_admission(
        &[candidate(&mission)],
        &query_on(
            "refs/heads/main",
            &pusher,
            std::slice::from_ref(&owner),
            HEAD_SHA
        ),
    )
    .is_admitted());
    assert!(
        !evaluate_verdict_admission(
            &[candidate(&mission)],
            &query_on(
                "refs/heads/main-2",
                &pusher,
                std::slice::from_ref(&owner),
                HEAD_SHA
            ),
        )
        .is_admitted(),
        "`main` does not name `main-2`"
    );
}

/// A report naming no branch scopes nothing — it says only "this commit is
/// good". Stated as a test so the silence is deliberate rather than an
/// oversight, and so the residual is visible if it ever needs tightening.
#[test]
fn a_report_naming_no_branch_scopes_no_ref() {
    let mission = mission(
        Some(HEAD_SHA),
        None,
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    for ref_name in ["refs/heads/main", "refs/heads/release"] {
        assert!(
            evaluate_verdict_admission(
                &[candidate(&mission)],
                &query_on(ref_name, &pusher, std::slice::from_ref(&owner), HEAD_SHA),
            )
            .is_admitted(),
            "a report with no branch constrains no ref"
        );
    }
}

// ── fix round 1: the three attacks the reviewer ran and the lane had not ──

/// A retracted approval must not still admit.
///
/// The founder approves, then publishes a `changes-requested` disposition that
/// `supersedes` the approval. The fold drops the superseded event from
/// `included_event_ids`, so `canonical_records` never sees it — this asserts
/// that end of the chain rather than assuming it.
#[test]
fn a_superseded_approval_no_longer_admits() {
    let founder = Keys::generate();
    let builder = Keys::generate();
    let seats = vec![CodingSessionTeamActiveSeat {
        actor_pubkey: builder.public_key().to_hex(),
        role: "builder".into(),
    }];

    let assignment = signed(&assignment(&builder), &founder, 1_000);
    let report_event = signed(
        &report(&assignment.id.to_hex(), Some("whoami/cli"), Some(HEAD_SHA)),
        &builder,
        1_100,
    );
    let approval = signed(
        &disposition(
            &assignment.id.to_hex(),
            &report_event.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        &founder,
        1_200,
    );
    let mut retraction = disposition(
        &assignment.id.to_hex(),
        &report_event.id.to_hex(),
        CodingSessionTeamDispositionDecision::ChangesRequested,
    );
    retraction.supersedes = Some(approval.id.to_hex());
    let retraction = signed(&retraction, &founder, 1_300);

    let context = verdict_admission_fold_context(
        CHANNEL,
        SESSION,
        GENESIS,
        founder.public_key().to_hex(),
        seats,
    );
    let events = vec![assignment, report_event, approval.clone(), retraction];
    let canonical = fold_candidate_records(&events, &context).expect("the fixture folds cleanly");
    assert!(
        !canonical
            .iter()
            .any(|record| record.event_id == approval.id.to_hex()),
        "the fold must drop the superseded approval, not merely rank it lower"
    );

    let owner = founder.public_key().to_hex();
    let pusher = builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[VerdictAdmissionCandidate {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            founder_pubkey: owner.clone(),
            canonical,
            active_seats: Vec::new(),
            observed_gates: Vec::new(),
            gate_policy: None,
        }],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    assert!(
        !outcome.is_admitted(),
        "a retracted approval admits nothing: {outcome:?}"
    );
}

/// An approved report naming the PARENT commit must not admit the child.
///
/// The realistic near-miss: the verdict was about the commit before the one
/// being pushed. Whole-string comparison is what refuses it, and the refusal
/// names the *pushed* oid so a person can see which commit was unapproved.
#[test]
fn a_report_naming_the_parent_sha_refuses_the_child() {
    let mission = mission(
        Some(OTHER_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = mission.founder.public_key().to_hex();
    let pusher = mission.builder.public_key().to_hex();
    let outcome = evaluate_verdict_admission(
        &[candidate(&mission)],
        &query(&pusher, std::slice::from_ref(&owner), HEAD_SHA),
    );
    let VerdictAdmission::Refused(refusal) = outcome else {
        panic!("an approval of a different commit is not an approval of this one");
    };
    assert!(
        refusal.reason().contains(HEAD_SHA),
        "the refusal names the pushed oid, not the approved one: {}",
        refusal.reason()
    );
    assert!(
        !refusal.reason().contains(OTHER_SHA),
        "and does not quote the approved one as if it were the answer"
    );
}

/// A `reportRef` pointing at a report that lives in ANOTHER mission's fold
/// must resolve to nothing — no cross-mission report borrowing.
#[test]
fn a_report_ref_pointing_outside_the_mission_never_admits() {
    let victim = mission(
        Some(HEAD_SHA),
        Some("whoami/cli"),
        CodingSessionTeamDispositionDecision::Approve,
        false,
    );
    let owner = victim.founder.public_key().to_hex();

    // The attacker's candidate: the victim's canonical records, but a
    // disposition whose reportRef names an id this fold does not contain.
    let mut canonical = candidate(&victim).canonical;
    canonical.retain(|record| {
        !matches!(
            record.payload.body,
            CodingSessionTeamTransactionBody::Report(_)
        )
    });
    let outcome = evaluate_verdict_admission(
        &[VerdictAdmissionCandidate {
            session_ref: SESSION.into(),
            genesis_ref: GENESIS.into(),
            founder_pubkey: owner.clone(),
            canonical,
            active_seats: Vec::new(),
            observed_gates: Vec::new(),
            gate_policy: None,
        }],
        &query(
            &victim.builder.public_key().to_hex(),
            std::slice::from_ref(&owner),
            HEAD_SHA,
        ),
    );
    assert!(
        !outcome.is_admitted(),
        "a reportRef that resolves to nothing in THIS fold admits nothing: {outcome:?}"
    );
}

/// Finding 33's cases live in their own file: they need every fixture above,
/// and this one is already 900 lines.
#[path = "coding_session_verdict_admission_founder_tests.rs"]
mod founders;
