//! What declared work says about an assignment, and what it refuses to say.
//!
//! Every fixture is a **signed** event, because every rule under test reads a
//! signature: the 44244 fold verifies before it attributes, authorization is
//! decided from the signer, and the assigner this projection exposes is the
//! event's own pubkey rather than anything in the payload.

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

use super::*;
use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamActiveSeat, CodingSessionTeamAssignment, CodingSessionTeamMissionCompleted,
    CodingSessionTeamReport, CodingSessionTeamTransactionTest,
    CodingSessionTeamTransactionTestOutcome, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;

pub(super) const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
pub(super) const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

pub(super) fn genesis() -> String {
    "ab".repeat(32)
}

pub(super) fn context(founder: &Keys, seats: Vec<(&Keys, &str)>) -> CodingSessionTeamFoldContext {
    context_for(CHANNEL, SESSION, &genesis(), founder, seats)
}

/// The same context for an arbitrary umbrella, so the fixture can build two.
pub(super) fn context_for(
    channel: &str,
    session: &str,
    genesis: &str,
    founder: &Keys,
    seats: Vec<(&Keys, &str)>,
) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: channel.to_owned(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: seats
            .into_iter()
            .map(|(keys, role)| CodingSessionTeamActiveSeat {
                actor_pubkey: keys.public_key().to_hex(),
                role: role.to_owned(),
            })
            .collect(),
        active_grants: Vec::new(),
        // This projection reads no policy set, which is what the field requires
        // of such a caller: the fold then behaves exactly as it did before the
        // field existed, and nothing here renders `false` as a claim.
        verifier_required: false,
    }
}

pub(super) fn payload(
    session: &str,
    genesis: &str,
    body: CodingSessionTeamTransactionBody,
) -> CodingSessionTeamTransactionPayload {
    CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: session.to_owned(),
        genesis_ref: genesis.to_owned(),
        transaction_type: body.transaction_type(),
        supersedes: None,
        delivery_command_id: None,
        body,
    }
}

fn local(body: CodingSessionTeamTransactionBody) -> CodingSessionTeamTransactionPayload {
    payload(SESSION, &genesis(), body)
}

pub(super) fn assignment_body(actor: &Keys, objective: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: actor.public_key().to_hex(),
        assignee_role: "builder".into(),
        objective: objective.into(),
        brief: "Implement the bounded assigned slice and report the gates.".into(),
        branch: Some("work/declared-work-fable".into()),
        base_sha: Some("9a1c4e7b2d3f40516273849506172839405a6b7c".into()),
        file_ownership: vec![
            "crates/buzz-core/src/pulse_declared_work.rs".into(),
            "desktop/src/features/project-pulse/lib/".into(),
        ],
        acceptance_steps: vec![
            "cargo test -p buzz-core pulse_declared_work".into(),
            "pnpm biome check".into(),
        ],
    })
}

pub(super) fn report_body(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
        assignment_ref: assignment_ref.to_owned(),
        summary: "Projection, decoder and fixture are green.".into(),
        branch: Some("work/declared-work-fable".into()),
        base_sha: Some("9a1c4e7b2d3f40516273849506172839405a6b7c".into()),
        head_sha: Some("c1d2e3f4a5b60718293a4b5c6d7e8f9001122334".into()),
        files: vec!["crates/buzz-core/src/pulse_declared_work.rs".into()],
        tests: vec![
            CodingSessionTeamTransactionTest {
                name: "cargo test".into(),
                command: "cargo test -p buzz-core pulse_declared_work".into(),
                outcome: CodingSessionTeamTransactionTestOutcome::Passed,
                evidence: Some("12 passed".into()),
            },
            CodingSessionTeamTransactionTest {
                name: "node --test".into(),
                command: "node --test pulseDeclaredWorkWire.test.mjs".into(),
                outcome: CodingSessionTeamTransactionTestOutcome::Passed,
                evidence: None,
            },
        ],
        red_before_green: Some(true),
        deviations: vec!["Left the surface to Lane U.".into()],
        residuals: vec!["Pagination is Lane Q's.".into()],
        anomalies: Vec::new(),
    })
}

pub(super) fn disposition_body(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
        assignment_ref: assignment_ref.to_owned(),
        report_ref: report_ref.to_owned(),
        refutation_ref: None,
        decision,
        summary: "Governed.".into(),
        findings: Vec::new(),
        required_action: None,
    })
}

/// An approving disposition that **asks** the assignee for something, so an
/// acknowledgement is still owed (lane 210).
pub(super) fn disposition_body_asking(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
        assignment_ref: assignment_ref.to_owned(),
        report_ref: report_ref.to_owned(),
        refutation_ref: None,
        decision,
        summary: "Governed.".into(),
        findings: Vec::new(),
        required_action: Some("Confirm you have read the residuals.".into()),
    })
}

pub(super) fn acknowledgement_body(reference: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Acknowledgement(CodingSessionTeamAcknowledgement {
        acknowledged_event_ref: reference.to_owned(),
        status: CodingSessionTeamAcknowledgementStatus::Received,
        note: None,
    })
}

pub(super) fn completed_body(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::MissionCompleted(CodingSessionTeamMissionCompleted {
        assignment_refs: vec![assignment_ref.to_owned()],
        landed_shas: vec!["c1d2e3f4a5b60718293a4b5c6d7e8f9001122334".into()],
        summary: "Landed.".into(),
        follow_ups: Vec::new(),
    })
}

pub(super) fn signed(
    channel: &str,
    payload: &CodingSessionTeamTransactionPayload,
    keys: &Keys,
    created_at: u64,
) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", channel]).expect("h"),
        Tag::parse(["d", payload.session_ref.as_str()]).expect("d"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("genesis"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("type"),
    ])
    .custom_created_at(Timestamp::from_secs(created_at))
    .sign_with_keys(keys)
    .expect("sign")
}

fn sign(payload: &CodingSessionTeamTransactionPayload, keys: &Keys, at: u64) -> nostr::Event {
    signed(CHANNEL, payload, keys, at)
}

fn sources<'a>(
    context: &'a CodingSessionTeamFoldContext,
    team: &'a [nostr::Event],
    lifecycle: PulseDeclaredWorkLifecycle,
) -> PulseDeclaredWorkSources<'a> {
    PulseDeclaredWorkSources {
        session_key: SESSION,
        channel_id: CHANNEL,
        session_ref: SESSION,
        name: Some("Declared work"),
        lifecycle,
        latest_observation_at: Some(1_756_800_600),
        context,
        team_events: team,
    }
}

// ── The unresolved case, and every source field it carries ───────────────────

#[test]
fn an_assignment_nobody_has_reported_on_is_unresolved_and_carries_its_source() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let events = vec![assignment.clone()];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    assert_eq!(session.session_key, SESSION);
    assert_eq!(session.session_ref, SESSION);
    // The umbrella's coordination key and the signed record its 44244 set
    // names are different facts, and the details block shows both (§6).
    assert_eq!(session.genesis_ref, genesis());
    assert_eq!(session.channel_id, CHANNEL);
    assert_eq!(session.founder_pubkey, founder.public_key().to_hex());
    assert_eq!(session.lifecycle, PulseDeclaredWorkLifecycle::Open);
    assert_eq!(session.unreadable, None);
    assert_eq!(session.excluded_count, 0);
    assert_eq!(session.assignments.len(), 1);

    let declared = &session.assignments[0];
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Unresolved);
    assert_eq!(declared.source_event_id, assignment.id.to_hex());
    assert_eq!(declared.created_at, 10);
    // The assigning author is the signer, and stays inspectable; the assignee
    // is the responsible participant and is a different key.
    assert_eq!(declared.assigner_pubkey, founder.public_key().to_hex());
    assert_eq!(declared.assignee_actor, actor.public_key().to_hex());
    assert_ne!(declared.assigner_pubkey, declared.assignee_actor);
    assert_eq!(declared.assignee_role, "builder");
    assert_eq!(declared.objective, "Build the wire");
    assert!(declared.brief.starts_with("Implement the bounded"));
    assert_eq!(declared.branch.as_deref(), Some("work/declared-work-fable"));
    assert_eq!(
        declared.base_sha.as_deref(),
        Some("9a1c4e7b2d3f40516273849506172839405a6b7c")
    );
    // Declared paths verbatim: no normalisation, no comparison, no inference.
    assert_eq!(
        declared.file_ownership,
        vec![
            "crates/buzz-core/src/pulse_declared_work.rs".to_owned(),
            "desktop/src/features/project-pulse/lib/".to_owned(),
        ]
    );
    assert_eq!(declared.acceptance_steps.len(), 2);
    assert_eq!(declared.supersedes, None);
    assert!(declared.reports.is_empty());
    assert!(declared.dispositions.is_empty());
    assert!(!declared.settlement.settled);
    assert_eq!(declared.settlement.governed_report_event_id, None);
}

// ── A report is evidence of a report ─────────────────────────────────────────

#[test]
fn a_report_makes_an_assignment_reported_and_discloses_an_unseated_author() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    // No seat for the assignee: the assignment names a target, so the report
    // is still canonical, and the missing seat is disclosed rather than hidden.
    let context = context(&founder, vec![]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let events = vec![assignment, report.clone()];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let declared = &session.assignments[0];
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Reported);
    assert!(
        !declared.settlement.settled,
        "a report never settles anything"
    );
    assert_eq!(declared.reports.len(), 1);
    let evidence = &declared.reports[0];
    assert_eq!(evidence.event_id, report.id.to_hex());
    assert_eq!(evidence.author_pubkey, actor.public_key().to_hex());
    assert_eq!(evidence.created_at, 20);
    assert_eq!(
        evidence.summary,
        "Projection, decoder and fixture are green."
    );
    assert_eq!(
        evidence.head_sha.as_deref(),
        Some("c1d2e3f4a5b60718293a4b5c6d7e8f9001122334")
    );
    assert_eq!(evidence.files.len(), 1);
    assert_eq!(evidence.test_count, 2);
    assert_eq!(evidence.deviations.len(), 1);
    assert_eq!(evidence.residuals.len(), 1);
    assert!(
        evidence.author_unseated,
        "the fold's unseated disclosure is carried, not recomputed"
    );
}

#[test]
fn a_seated_reporter_is_not_disclosed_as_unseated() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let events = vec![assignment, report];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    assert!(!session.assignments[0].reports[0].author_unseated);
}

// ── Settlement is the fold's rule, and only the fold's ───────────────────────

#[test]
fn an_approving_disposition_that_asks_for_something_stays_reported() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let report_id = report.id.to_hex();
    let disposition = sign(
        &local(disposition_body_asking(
            &assignment_id,
            &report_id,
            CodingSessionTeamDispositionDecision::Approve,
        )),
        &founder,
        30,
    );
    let events = vec![assignment, report, disposition.clone()];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let declared = &session.assignments[0];
    assert_eq!(
        declared.status,
        PulseDeclaredAssignmentStatus::Reported,
        "an approval that asks the assignee for something is not settlement — \
         its answer is the fold's second half, and this view adds no new gate"
    );
    assert!(!declared.settlement.settled);
    assert_eq!(declared.settlement.settled_by, None);
    assert_eq!(declared.dispositions.len(), 1);
    assert_eq!(declared.dispositions[0].event_id, disposition.id.to_hex());
    assert_eq!(
        declared.dispositions[0].decision,
        CodingSessionTeamDispositionDecision::Approve
    );
    assert_eq!(declared.dispositions[0].report_ref, report_id);
    assert_eq!(
        declared.dispositions[0].author_pubkey,
        founder.public_key().to_hex()
    );
}

#[test]
fn an_approval_the_assignee_acknowledged_is_settled_with_its_three_ids() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let report_id = report.id.to_hex();
    let disposition = sign(
        &local(disposition_body(
            &assignment_id,
            &report_id,
            CodingSessionTeamDispositionDecision::ApproveWithNotes,
        )),
        &founder,
        30,
    );
    let disposition_id = disposition.id.to_hex();
    let acknowledgement = sign(&local(acknowledgement_body(&disposition_id)), &actor, 40);
    let acknowledgement_id = acknowledgement.id.to_hex();
    let events = vec![assignment, report, disposition, acknowledgement];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let declared = &session.assignments[0];
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Settled);
    assert!(declared.settlement.settled);
    assert_eq!(
        declared.settlement.governed_report_event_id.as_deref(),
        Some(report_id.as_str())
    );
    assert_eq!(
        declared.settlement.disposition_event_id.as_deref(),
        Some(disposition_id.as_str())
    );
    assert_eq!(
        declared.settlement.acknowledgement_event_id.as_deref(),
        Some(acknowledgement_id.as_str())
    );
}

#[test]
fn a_changes_requested_disposition_is_listed_and_settles_nothing() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let report_id = report.id.to_hex();
    let disposition = sign(
        &local(disposition_body(
            &assignment_id,
            &report_id,
            CodingSessionTeamDispositionDecision::ChangesRequested,
        )),
        &founder,
        30,
    );
    let events = vec![assignment, report, disposition];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let declared = &session.assignments[0];
    assert_eq!(declared.dispositions.len(), 1);
    assert_eq!(
        declared.dispositions[0].decision,
        CodingSessionTeamDispositionDecision::ChangesRequested
    );
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Reported);
    assert!(!declared.settlement.settled);
}

// ── Supersession: one declaration, and a count for the displaced one ─────────

#[test]
fn a_superseded_assignment_is_a_count_rather_than_a_second_declaration() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let original = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let original_id = original.id.to_hex();
    // A correction keeps author, type and subject; only the wording moves.
    let mut corrected = local(assignment_body(&actor, "Build the wire and the decoder"));
    corrected.supersedes = Some(original_id.clone());
    let correction = sign(&corrected, &founder, 20);
    let events = vec![original, correction.clone()];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    assert_eq!(session.assignments.len(), 1, "one declaration, not two");
    let declared = &session.assignments[0];
    assert_eq!(declared.source_event_id, correction.id.to_hex());
    assert_eq!(declared.objective, "Build the wire and the decoder");
    assert_eq!(declared.supersedes.as_deref(), Some(original_id.as_str()));
    assert_eq!(
        session.excluded_count, 1,
        "the displaced record is a number, never content"
    );
}

// ── Closing settles nothing ──────────────────────────────────────────────────

#[test]
fn a_closed_session_with_a_completion_leaves_an_unreported_assignment_unresolved() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let other = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    // One assignment is settled, so the completion folds; a second assignment
    // the completion never named is untouched by it.
    let settled = sign(
        &local(assignment_body(&actor, "Land the wire")),
        &founder,
        10,
    );
    let settled_id = settled.id.to_hex();
    let report = sign(&local(report_body(&settled_id)), &actor, 20);
    let report_id = report.id.to_hex();
    let disposition = sign(
        &local(disposition_body(
            &settled_id,
            &report_id,
            CodingSessionTeamDispositionDecision::Approve,
        )),
        &founder,
        30,
    );
    let acknowledgement = sign(
        &local(acknowledgement_body(&disposition.id.to_hex())),
        &actor,
        40,
    );
    let unreported = sign(
        &local(assignment_body(&other, "Write the runbook")),
        &founder,
        50,
    );
    let unreported_id = unreported.id.to_hex();
    let completion = sign(&local(completed_body(&settled_id)), &founder, 60);
    let events = vec![
        settled,
        report,
        disposition,
        acknowledgement,
        unreported,
        completion.clone(),
    ];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Closed,
    ));

    assert_eq!(session.lifecycle, PulseDeclaredWorkLifecycle::Closed);
    let terminal = session.terminal.as_ref().expect("a canonical terminal");
    assert_eq!(terminal.event_id, completion.id.to_hex());
    assert_eq!(terminal.terminal_type, "mission.completed");
    assert_eq!(terminal.at, 60);

    let open = session
        .assignments
        .iter()
        .find(|declared| declared.source_event_id == unreported_id)
        .expect("the assignment nobody reported on");
    assert_eq!(
        open.status,
        PulseDeclaredAssignmentStatus::Unresolved,
        "closing an execution, and finishing a different assignment, settle nothing"
    );
    assert!(!open.settlement.settled);

    let done = session
        .assignments
        .iter()
        .find(|declared| declared.source_event_id == settled_id)
        .expect("the settled assignment");
    assert_eq!(done.status, PulseDeclaredAssignmentStatus::Settled);
}

// ── An unreadable session says so, and claims nothing ────────────────────────

#[test]
fn a_fold_error_reads_unreadable_with_no_assignments_rather_than_no_work() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    // The same event twice: a caller filter defect, and a hard fold error.
    let events = vec![assignment.clone(), assignment];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let reason = session.unreadable.as_deref().expect("the fold's sentence");
    assert!(
        reason.starts_with("duplicate supplied team transaction"),
        "the fold's own words, verbatim: {reason}"
    );
    assert!(session.assignments.is_empty());
    assert_eq!(session.excluded_count, 0);
    assert!(session.terminal.is_none());
}

// ── Determinism ──────────────────────────────────────────────────────────────

#[test]
fn assignments_come_back_ascending_by_created_at_then_event_id() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    // Two assignments at the same second, so the tie-break is the event id,
    // and one later — supplied in the wrong order on purpose.
    let same_a = sign(&local(assignment_body(&actor, "Alpha")), &founder, 10);
    let same_b = sign(&local(assignment_body(&actor, "Beta")), &founder, 10);
    let later = sign(&local(assignment_body(&actor, "Gamma")), &founder, 30);
    let events = vec![later.clone(), same_b.clone(), same_a.clone()];

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    let order: Vec<(i64, String)> = session
        .assignments
        .iter()
        .map(|declared| (declared.created_at, declared.source_event_id.clone()))
        .collect();
    let mut expected = vec![
        (10, same_a.id.to_hex()),
        (10, same_b.id.to_hex()),
        (30, later.id.to_hex()),
    ];
    expected.sort();
    assert_eq!(order, expected);
}

// ── Many assignments at once ─────────────────────────────────────────────────

/// Twelve assignments, three reports and one disposition each: the shape the
/// grouping pass exists for.
///
/// Not a timing test — there is no wall-clock assertion here, because a
/// wall-clock assertion measures the machine rather than the code. What it
/// pins is the property the restructure had to preserve: each assignment's
/// evidence is exactly its own, in the fold's included order, however many
/// records share the page.
#[test]
fn every_assignment_in_a_crowded_mission_gets_exactly_its_own_evidence() {
    let founder = Keys::generate();
    let actors: Vec<Keys> = (0..12).map(|_| Keys::generate()).collect();
    let context = context(
        &founder,
        actors.iter().map(|actor| (actor, "builder")).collect(),
    );

    let mut events = Vec::new();
    let mut assignment_ids = Vec::new();
    let mut at = 10_u64;
    for (index, actor) in actors.iter().enumerate() {
        let assignment = sign(
            &local(assignment_body(actor, &format!("Objective {index}"))),
            &founder,
            at,
        );
        at += 1;
        assignment_ids.push(assignment.id.to_hex());
        events.push(assignment);
    }

    // Three reports per assignment, published interleaved across assignments
    // so no list can come out right by accident of contiguity.
    let mut expected_reports: Vec<Vec<String>> = vec![Vec::new(); actors.len()];
    let mut expected_dispositions: Vec<Vec<String>> = vec![Vec::new(); actors.len()];
    for round in 0..3 {
        for (index, actor) in actors.iter().enumerate() {
            let report = sign(&local(report_body(&assignment_ids[index])), actor, at);
            at += 1;
            expected_reports[index].push(report.id.to_hex());
            // One disposition per assignment, on its middle report.
            if round == 1 {
                let disposition = sign(
                    &local(disposition_body(
                        &assignment_ids[index],
                        &report.id.to_hex(),
                        CodingSessionTeamDispositionDecision::ChangesRequested,
                    )),
                    &founder,
                    at + 1_000,
                );
                expected_dispositions[index].push(disposition.id.to_hex());
                events.push(report);
                events.push(disposition);
                continue;
            }
            events.push(report);
        }
    }

    let session = project_declared_work(&sources(
        &context,
        &events,
        PulseDeclaredWorkLifecycle::Open,
    ));

    assert_eq!(session.assignments.len(), 12);
    assert_eq!(session.excluded_count, 0);
    for (index, id) in assignment_ids.iter().enumerate() {
        let declared = session
            .assignments
            .iter()
            .find(|declared| &declared.source_event_id == id)
            .unwrap_or_else(|| panic!("assignment {index} is missing from the projection"));
        assert_eq!(
            declared
                .reports
                .iter()
                .map(|report| report.event_id.clone())
                .collect::<Vec<_>>(),
            expected_reports[index],
            "assignment {index} must carry its own three reports, in included order"
        );
        assert_eq!(
            declared
                .dispositions
                .iter()
                .map(|disposition| disposition.event_id.clone())
                .collect::<Vec<_>>(),
            expected_dispositions[index],
            "assignment {index} must carry only the disposition that names it"
        );
        // Every report names this assignment and nobody else's.
        for report in &declared.reports {
            assert!(
                expected_reports
                    .iter()
                    .enumerate()
                    .all(|(other, ids)| other == index || !ids.contains(&report.event_id)),
                "report {} leaked between assignments",
                report.event_id
            );
        }
        assert_eq!(declared.objective, format!("Objective {index}"));
        assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Reported);
        assert!(!declared.settlement.settled);
    }

    // And the whole list is still ascending by (createdAt, sourceEventId).
    let order: Vec<(i64, &str)> = session
        .assignments
        .iter()
        .map(|declared| (declared.created_at, declared.source_event_id.as_str()))
        .collect();
    let mut sorted = order.clone();
    sorted.sort();
    assert_eq!(order, sorted);
}

#[path = "pulse_declared_work_fixture_tests.rs"]
mod fixture_tests;

/// Lane 210, on this surface: an approving disposition that asks for nothing
/// is settled with no acknowledgement, and the projection says which rule did
/// it rather than leaving a reader to infer settlement from a null receipt id.
#[test]
fn an_approval_that_asks_nothing_is_settled_and_names_the_rule() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, vec![(&actor, "builder")]);
    let assignment = sign(
        &local(assignment_body(&actor, "Build the wire")),
        &founder,
        10,
    );
    let assignment_id = assignment.id.to_hex();
    let report = sign(&local(report_body(&assignment_id)), &actor, 20);
    let report_id = report.id.to_hex();
    let disposition = sign(
        &local(disposition_body(
            &assignment_id,
            &report_id,
            CodingSessionTeamDispositionDecision::Approve,
        )),
        &founder,
        30,
    );

    let session = project_declared_work(&sources(
        &context,
        &[assignment, report, disposition.clone()],
        PulseDeclaredWorkLifecycle::Open,
    ));

    let declared = &session.assignments[0];
    assert_eq!(declared.status, PulseDeclaredAssignmentStatus::Settled);
    assert!(declared.settlement.settled);
    assert_eq!(
        declared.settlement.settled_by.as_deref(),
        Some("approving_disposition_without_ask")
    );
    assert_eq!(declared.settlement.acknowledgement_event_id, None);
    assert_eq!(
        declared.settlement.disposition_event_id.as_deref(),
        Some(disposition.id.to_hex().as_str())
    );
}
