//! Ledger 183: `bee sessions complete` publishes an early completion and says
//! what it is waiting for, instead of refusing it.

use super::*;

use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamActiveSeat, CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamFoldContext, CodingSessionTeamMissionCompleted, CodingSessionTeamReport,
    CodingSessionTeamTransactionBody, CodingSessionTeamVerdict,
};
use beekeeper_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};
use nostr::Keys;

use super::super::super::operations_precheck::{FetchedSession, PrecheckedOperation};

const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS: &str = "abababababababababababababababababababababababababababababababab";

fn signed(body: CodingSessionTeamTransactionBody, keys: &Keys) -> Event {
    let payload = coding_session_team_transaction_payload(
        SESSION.to_owned(),
        GENESIS.to_owned(),
        None,
        None,
        body,
    );
    build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the record builds")
        .sign_with_keys(keys)
        .expect("the record signs")
}

fn context(founder: &Keys, actor: &Keys) -> CodingSessionTeamFoldContext {
    CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: GENESIS.into(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: actor.public_key().to_hex(),
            role: "builder".into(),
        }],
        active_grants: Vec::new(),
        verifier_required: false,
    }
}

fn assignment(actor: &Keys) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
        assignee_actor: actor.public_key().to_hex(),
        assignee_role: "builder".into(),
        objective: "Build the slice".into(),
        brief: "Implement the bounded assigned slice.".into(),
        branch: None,
        base_sha: None,
        file_ownership: vec!["crates/beekeeper-core/src".into()],
        acceptance_steps: vec!["cargo test -p beekeeper-core".into()],
    })
}

fn report(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
        assignment_ref: assignment_ref.into(),
        summary: "Done".into(),
        branch: None,
        base_sha: None,
        head_sha: None,
        files: Vec::new(),
        tests: Vec::new(),
        red_before_green: None,
        deviations: Vec::new(),
        residuals: Vec::new(),
        anomalies: Vec::new(),
    })
}

fn approval(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
        assignment_ref: assignment_ref.into(),
        report_ref: report_ref.into(),
        refutation_ref: None,
        decision: CodingSessionTeamDispositionDecision::Approve,
        summary: "Governed".into(),
        findings: Vec::new(),
        required_action: None,
    })
}

/// An approving disposition that **asks** the assignee for something, so an
/// explicit answer is still owed after lane 210.
fn approval_asking(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
        assignment_ref: assignment_ref.into(),
        report_ref: report_ref.into(),
        refutation_ref: None,
        decision: CodingSessionTeamDispositionDecision::Approve,
        summary: "Governed".into(),
        findings: Vec::new(),
        required_action: Some("Confirm you have read the residuals.".into()),
    })
}

fn completion(assignment_ref: &str) -> CodingSessionTeamTransactionBody {
    CodingSessionTeamTransactionBody::MissionCompleted(CodingSessionTeamMissionCompleted {
        assignment_refs: vec![assignment_ref.into()],
        landed_shas: Vec::new(),
        summary: "Mission finished".into(),
        follow_ups: Vec::new(),
    })
}

fn prechecked(events: Vec<Event>, context: CodingSessionTeamFoldContext) -> PrecheckedOperation {
    PrecheckedOperation {
        supersedes: None,
        session: Some(FetchedSession { events, context }),
    }
}

/// The exact refusal of ledger 179(a), now an answer: the completion is
/// classified `pending`, and the missing acknowledgement is named with the
/// role and actor that owe it.
///
/// The approval **asks** for something (lane 210): that is now what makes an
/// acknowledgement owed at all. An approval that asked nothing settles on
/// arrival and this completion would be terminal —
/// `a_completion_over_an_approval_that_asks_nothing_is_terminal_with_no_receipt`
/// is that case, and it is the one this lane exists to stop charging a turn
/// for.
#[test]
fn a_completion_missing_an_acknowledgement_is_pending_and_names_the_owed_receipt() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let report = signed(report(&assignment.id.to_hex()), &actor);
    let approval = signed(
        approval_asking(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
    );
    let candidate = signed(completion(&assignment.id.to_hex()), &founder);
    let prechecked = prechecked(vec![assignment.clone(), report, approval], context.clone());

    let outcome = classify_completion_before_submit(&prechecked, &candidate)
        .expect("an early completion is published, not refused");
    let CompletionOutcome::Pending(pending) = &outcome else {
        panic!("expected a pending completion, got {outcome:?}");
    };
    assert_eq!(pending["code"], json!("completion_not_approved"));
    assert_eq!(pending["awaiting"].as_array().expect("a list").len(), 1);
    let awaiting = &pending["awaiting"][0];
    assert_eq!(awaiting["assignmentEventId"], json!(assignment.id.to_hex()));
    assert_eq!(awaiting["link"], json!("acknowledgement"));
    assert_eq!(awaiting["owedByRole"], json!("builder"));
    assert_eq!(awaiting["owedByActor"], json!(actor.public_key().to_hex()));
    // Present and null, never absent: no ruling is open, and "no ruling" and
    // "not disclosed" must stay different answers.
    assert_eq!(pending["waitingOnDecision"], Value::Null);
    assert_eq!(pending["message"], json!(COMPLETION_PENDING_MESSAGE));

    // And the answer a caller reads says `pending` with that object attached.
    let mut output = json!({"eventId": candidate.id.to_hex(), "accepted": true});
    disclose_completion_outcome(&mut output, &outcome);
    assert_eq!(output["outcome"], json!("pending"));
    assert_eq!(output["pending"]["code"], json!("completion_not_approved"));
}

/// The complete chain still folds terminal, and says so in the same two keys,
/// so a reader parses one shape either way.
#[test]
fn a_complete_chain_is_terminal_and_discloses_no_pending_wait() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let report = signed(report(&assignment.id.to_hex()), &actor);
    let approval = signed(
        approval(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
    );
    let acknowledgement = signed(
        CodingSessionTeamTransactionBody::Acknowledgement(CodingSessionTeamAcknowledgement {
            acknowledged_event_ref: approval.id.to_hex(),
            status: CodingSessionTeamAcknowledgementStatus::Received,
            note: None,
        }),
        &actor,
    );
    let candidate = signed(completion(&assignment.id.to_hex()), &founder);
    let prechecked = prechecked(vec![assignment, report, approval, acknowledgement], context);

    let outcome = classify_completion_before_submit(&prechecked, &candidate)
        .expect("a settled mission completes");
    assert_eq!(outcome, CompletionOutcome::Terminal);
    let mut output = json!({"eventId": candidate.id.to_hex()});
    disclose_completion_outcome(&mut output, &outcome);
    assert_eq!(output["outcome"], json!("terminal"));
    assert_eq!(output["pending"], Value::Null);
}

/// A completion nobody was entitled to sign is still refused before it reaches
/// the relay: no fact that later arrives would make it terminal, so calling it
/// "pending" would be a promise the fold will never keep.
#[test]
fn an_unauthorized_completion_is_still_refused_before_publication() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let stranger = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let candidate = signed(completion(&assignment.id.to_hex()), &stranger);
    let prechecked = prechecked(vec![assignment], context);

    let error = classify_completion_before_submit(&prechecked, &candidate)
        .expect_err("an unauthorized completion is refused");
    let message = error.to_string();
    assert!(
        message.contains("completion refused"),
        "unexpected refusal: {message}"
    );
}

/// The pending sentence is printed to a person, so its shape is pinned exactly
/// as `COMPLETION_REFUSED_FALLBACK`'s is (REVIEW-L7 F3 found eighteen spaces
/// in the middle of that one).
#[test]
fn the_pending_message_reads_as_prose() {
    assert!(
        !COMPLETION_PENDING_MESSAGE.contains("  "),
        "the pending message carries a run of spaces: {COMPLETION_PENDING_MESSAGE:?}"
    );
    assert!(!COMPLETION_PENDING_MESSAGE.contains('\n'));
    assert!(COMPLETION_PENDING_MESSAGE.contains("no further turn from anybody"));
}

// ── Coverage before a terminal (lane 201) ──────────────────────────────────

use beekeeper_core::project_work_fold::{
    WorkCoverageReasonCode, WorkCriterionProjection, WorkCriterionStatus,
    WorkDeclarationProjection, WorkDeclarationState, WorkProjection, WorkReasonCode,
};

fn criterion(id: &str, status: WorkCriterionStatus) -> WorkCriterionProjection {
    WorkCriterionProjection {
        criterion_id: id.to_owned(),
        proof: None,
        status,
        assignment_refs: Vec::new(),
        evidence: Vec::new(),
        artifact_commit: None,
        reason_code: (status != WorkCriterionStatus::Covered)
            .then_some(WorkReasonCode::EvidenceUnavailable),
        reason: None,
    }
}

fn projection(
    state: WorkDeclarationState,
    criteria: Vec<WorkCriterionProjection>,
    complete: bool,
) -> WorkProjection {
    projection_with(state, criteria, complete, true, None)
}

/// A projection with the two declaration-level facts the gate must read on
/// their own: whether the plan resolved, and which clause of coverage failed.
fn projection_with(
    state: WorkDeclarationState,
    criteria: Vec<WorkCriterionProjection>,
    complete: bool,
    plan_resolved: bool,
    coverage_reason_code: Option<WorkCoverageReasonCode>,
) -> WorkProjection {
    WorkProjection {
        schema: "buzz-project-work-coverage/v1".to_owned(),
        session_ref: SESSION.to_owned(),
        project_ref: "30621:1e:kettle".to_owned(),
        declarations: vec![WorkDeclarationProjection {
            work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
            declaration_ref: "1a".repeat(32),
            plan_ref: beekeeper_core::project_work::ProjectWorkPlanRef {
                repository: "30617:1e:kettle-beekeeper-agents".to_owned(),
                commit: "ab".repeat(20),
                path: "plans/kettle.md".to_owned(),
            },
            state,
            supersedes: Vec::new(),
            superseded_by: Vec::new(),
            state_reason_code: None,
            state_reason: None,
            plan_resolved,
            // A10: drift is disclosure and never a gate, so the gate's own
            // tests hold it at the answer that asserts nothing.
            plan_drift: beekeeper_core::project_work_fold::WorkPlanDrift {
                declared_commit: "ab".repeat(20),
                current_commit: None,
                state: beekeeper_core::project_work_fold::WorkPlanDriftState::Unknown,
            },
            candidate_artifact: None,
            artifact_commits: Vec::new(),
            criteria,
            coverage_complete: complete,
            coverage_reason_code: coverage_reason_code
                .or((!complete).then_some(WorkCoverageReasonCode::CriteriaNotCovered)),
            coverage_reason: (!complete).then(|| "1 of 2 criteria are not covered".to_owned()),
        }],
        excluded: Vec::new(),
        conflicts: Vec::new(),
    }
}

/// A session with **no declaration** has no contract to measure against: the
/// gate finds nothing incomplete and every pre-NIP-PW session completes
/// exactly as it did.
#[test]
fn a_session_with_no_declaration_is_unchanged() {
    let empty = WorkProjection {
        schema: "buzz-project-work-coverage/v1".to_owned(),
        session_ref: SESSION.to_owned(),
        project_ref: String::new(),
        declarations: Vec::new(),
        excluded: Vec::new(),
        conflicts: Vec::new(),
    };
    assert!(incomplete_head(&empty).is_none());
}

/// A fully covered head blocks nothing.
#[test]
fn a_covered_head_blocks_nothing() {
    let covered = projection(
        WorkDeclarationState::Head,
        vec![criterion("cli-behaviour", WorkCriterionStatus::Covered)],
        true,
    );
    assert!(incomplete_head(&covered).is_none());
}

/// An open, stale or unknown criterion is named in the refusal, with its
/// reason — the operator must not have to go and ask what is missing.
#[test]
fn an_incomplete_head_names_every_criterion_and_its_reason() {
    for status in [
        WorkCriterionStatus::Open,
        WorkCriterionStatus::Stale,
        WorkCriterionStatus::Unknown,
    ] {
        let incomplete = projection(
            WorkDeclarationState::Head,
            vec![
                criterion("cli-behaviour", WorkCriterionStatus::Covered),
                criterion("usage-documentation", status),
            ],
            false,
        );
        let open = incomplete_head(&incomplete).expect("incomplete");
        assert_eq!(open.criteria.len(), 1, "only the uncovered one is listed");
        let line = &open.criteria[0];
        assert!(line.contains("usage-documentation"), "{line}");
        assert!(line.contains(status.as_str()), "{line}");
        assert!(line.contains("evidence_unavailable"), "{line}");
        assert!(open.coverage_reason.contains("not covered"));
    }
}

/// A superseded declaration carries no criteria, and never blocks a
/// completion by itself: only a current contract can.
#[test]
fn a_superseded_declaration_never_blocks_a_completion() {
    let superseded = projection(WorkDeclarationState::Superseded, Vec::new(), false);
    assert!(incomplete_head(&superseded).is_none());
}

// ── Finding 5 (ledger 213): the gate fails CLOSED ──────────────────────────

/// **Reproduces finding 5.** Adopt, then complete with no `--agents-repo` and
/// no bindings: the plan was never read, so the projection renders **no
/// criterion rows at all**. Gating on rendered rows let that complete
/// silently; the gate reads the declaration's own state instead.
#[test]
fn a_head_whose_plan_could_not_be_read_is_refused() {
    let unreadable = projection_with(
        WorkDeclarationState::Head,
        Vec::new(),
        false,
        false,
        Some(WorkCoverageReasonCode::PlanUnavailable),
    );
    let open = incomplete_head(&unreadable).expect("an unreadable plan blocks a completion");
    assert!(open.criteria.is_empty(), "there are no rows to list");
    assert!(
        open.message.contains("plan"),
        "the refusal must say the plan could not be read: {}",
        open.message
    );
}

/// **Reproduces finding 5.** Two heads of one `workId` carry no criteria by
/// contract (`README` § (c): a conflicted declaration is not a current
/// contract). That must refuse a terminal, not permit one.
#[test]
fn conflicted_heads_are_refused() {
    let mut conflicted = projection_with(
        WorkDeclarationState::Conflict,
        Vec::new(),
        false,
        true,
        Some(WorkCoverageReasonCode::Conflict),
    );
    conflicted.conflicts = vec![beekeeper_core::project_work_fold::WorkConflict {
        work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
        heads: vec!["1a".repeat(32), "2b".repeat(32)],
        message: "two heads compete".to_owned(),
    }];
    let open = incomplete_head(&conflicted).expect("a fork blocks a completion");
    assert!(
        open.message.contains("conflict"),
        "the refusal must name the fork: {}",
        open.message
    );
}

/// A `stale` declaration is still a current contract — the goal moved, the
/// contract stayed pinned — so its incomplete coverage still refuses.
#[test]
fn a_stale_head_still_gates() {
    let stale = projection(
        WorkDeclarationState::Stale,
        vec![criterion("cli-behaviour", WorkCriterionStatus::Open)],
        false,
    );
    assert!(incomplete_head(&stale).is_some());
}

/// A superseded declaration is not a current contract and never blocks by
/// itself, whatever its coverage says.
#[test]
fn a_superseded_declaration_with_criteria_still_blocks_nothing() {
    let superseded = projection_with(
        WorkDeclarationState::Superseded,
        vec![criterion("cli-behaviour", WorkCriterionStatus::Open)],
        false,
        true,
        Some(WorkCoverageReasonCode::Superseded),
    );
    assert!(incomplete_head(&superseded).is_none());
}

/// Lane 210 at the CLI's own surface: `bee sessions complete` must stop
/// telling a lead that an acknowledgement is owed when none is.
///
/// Same three records as the pending case above, with the one difference the
/// rule reads — the approval asks for nothing — and the completion is
/// `terminal` on publication instead of `pending` on a receipt nobody owes.
#[test]
fn a_completion_over_an_approval_that_asks_nothing_is_terminal_with_no_receipt() {
    let founder = Keys::generate();
    let actor = Keys::generate();
    let context = context(&founder, &actor);
    let assignment = signed(assignment(&actor), &founder);
    let report = signed(report(&assignment.id.to_hex()), &actor);
    let approval = signed(
        approval(&assignment.id.to_hex(), &report.id.to_hex()),
        &founder,
    );
    let candidate = signed(completion(&assignment.id.to_hex()), &founder);
    let prechecked = prechecked(vec![assignment, report, approval], context);

    let outcome = classify_completion_before_submit(&prechecked, &candidate)
        .expect("a completion over a settled assignment is published");
    assert_eq!(outcome, CompletionOutcome::Terminal);

    let mut output = json!({"eventId": candidate.id.to_hex(), "accepted": true});
    disclose_completion_outcome(&mut output, &outcome);
    assert_eq!(output["outcome"], json!("terminal"));
    assert_eq!(output["pending"], Value::Null);
}

// ── Shared settlement conformance vectors (ledger 204's rule) ───────────────

/// The same file `buzz-core`, the provider and the desktop decoder load.
const SETTLEMENT_VECTORS: &str =
    include_str!("../../../../../conformance/team-settlement/fixtures/settlement-vectors.json");

/// Deterministic keys, matching the other readers' founder/assignee/stranger.
fn vector_keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("a fixed 32-byte secret key")
}

fn signed_with_supersedes(
    body: CodingSessionTeamTransactionBody,
    supersedes: Option<String>,
    keys: &Keys,
) -> Event {
    let payload = coding_session_team_transaction_payload(
        SESSION.to_owned(),
        GENESIS.to_owned(),
        supersedes,
        None,
        body,
    );
    build_coding_session_team_transaction(CHANNEL, payload)
        .expect("the record builds")
        .sign_with_keys(keys)
        .expect("the record signs")
}

/// The CLI's own reader of the vectors: the `fold` object
/// `bee sessions operation list|get` prints must carry the fold's settlement
/// rule verbatim, and must never say an acknowledgement is awaited for an
/// assignment the rule settled.
#[test]
fn the_cli_fold_object_renders_every_settlement_vector() {
    use std::collections::HashMap;
    let fixture: serde_json::Value =
        serde_json::from_str(SETTLEMENT_VECTORS).expect("the fixture is JSON");
    let founder = vector_keys(0x11);
    let assignee = vector_keys(0x22);
    let stranger = vector_keys(0x33);
    for vector in fixture["vectors"].as_array().expect("vectors") {
        let name = vector["name"].as_str().expect("a name");
        let mut ids: HashMap<String, String> = HashMap::new();
        let mut events: Vec<Event> = Vec::new();
        for record in vector["records"].as_array().expect("records") {
            let id = record["id"].as_str().expect("an id").to_owned();
            let author = match record["author"].as_str().expect("an author") {
                "founder" => &founder,
                "assignee" => &assignee,
                _ => &stranger,
            };
            let reference =
                |key: &str| ids[record[key].as_str().expect("a symbolic reference")].clone();
            let body = match record["type"].as_str().expect("a type") {
                "assignment" => {
                    CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                        assignee_actor: assignee.public_key().to_hex(),
                        assignee_role: "builder".into(),
                        // The symbolic id, so two structurally identical
                        // assignments in one vector are two distinct events.
                        objective: format!("Build the slice ({id})"),
                        brief: "Implement the bounded assigned slice.".into(),
                        branch: None,
                        base_sha: None,
                        file_ownership: vec!["crates/beekeeper-core/src".into()],
                        acceptance_steps: vec!["cargo test -p beekeeper-core".into()],
                    })
                }
                "report" => CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                    assignment_ref: reference("assignmentRef"),
                    summary: format!("Done ({id})"),
                    branch: None,
                    base_sha: None,
                    head_sha: None,
                    files: Vec::new(),
                    tests: Vec::new(),
                    red_before_green: None,
                    deviations: Vec::new(),
                    residuals: Vec::new(),
                    anomalies: Vec::new(),
                }),
                "disposition" => CodingSessionTeamTransactionBody::Verdict(
                    CodingSessionTeamVerdict::Disposition {
                        assignment_ref: reference("assignmentRef"),
                        report_ref: reference("reportRef"),
                        refutation_ref: None,
                        decision: match record["decision"].as_str().expect("a decision") {
                            "approve" => CodingSessionTeamDispositionDecision::Approve,
                            "approve-with-notes" => {
                                CodingSessionTeamDispositionDecision::ApproveWithNotes
                            }
                            "changes-requested" => {
                                CodingSessionTeamDispositionDecision::ChangesRequested
                            }
                            "reject" => CodingSessionTeamDispositionDecision::Reject,
                            _ => CodingSessionTeamDispositionDecision::Blocked,
                        },
                        summary: format!("Governed ({id})"),
                        findings: record["findings"]
                            .as_array()
                            .expect("findings")
                            .iter()
                            .map(|item| item.as_str().expect("a finding").to_owned())
                            .collect(),
                        required_action: record["requiredAction"]
                            .as_str()
                            .map(std::borrow::ToOwned::to_owned),
                    },
                ),
                "acknowledgement" => CodingSessionTeamTransactionBody::Acknowledgement(
                    CodingSessionTeamAcknowledgement {
                        acknowledged_event_ref: reference("acknowledgedEventRef"),
                        status: CodingSessionTeamAcknowledgementStatus::Received,
                        // The symbolic id, so a vector carrying two receipts
                        // of one disposition signs two distinct events.
                        note: Some(id.clone()),
                    },
                ),
                _ => CodingSessionTeamTransactionBody::MissionCompleted(
                    CodingSessionTeamMissionCompleted {
                        assignment_refs: record["assignmentRefs"]
                            .as_array()
                            .expect("assignmentRefs")
                            .iter()
                            .map(|item| ids[item.as_str().expect("a symbolic id")].clone())
                            .collect(),
                        landed_shas: Vec::new(),
                        summary: "Mission finished".into(),
                        follow_ups: Vec::new(),
                    },
                ),
            };
            let supersedes = record["supersedes"]
                .as_str()
                .map(|symbol| ids[symbol].clone());
            let event = signed_with_supersedes(body, supersedes, author);
            ids.insert(id, event.id.to_hex());
            events.push(event);
        }
        let context = context(&founder, &assignee);
        let fold =
            beekeeper_core::coding_session_team_transaction::fold_coding_session_team_transactions(
                &events, &context,
            )
            .unwrap_or_else(|error| panic!("{name}: the vector folds: {error}"));
        let rendered = crate::commands::sessions::operations::fold_json(&fold);
        for row in vector["expected"]["assignments"]
            .as_array()
            .expect("assignments")
        {
            let assignment_id = &ids[row["assignment"].as_str().expect("an assignment")];
            let printed = rendered["assignments"]
                .as_array()
                .expect("a list")
                .iter()
                .find(|item| item["assignmentEventId"] == json!(assignment_id))
                .unwrap_or_else(|| panic!("{name}: the fold object omits an assignment"));
            assert_eq!(
                printed["settled"],
                json!(row["settled"].as_bool().expect("settled")),
                "{name}: settled"
            );
            // Present and null, never absent: "not settled" and "this build
            // does not disclose the rule" must stay different answers.
            assert!(
                printed.get("settledBy").is_some(),
                "{name}: the fold object must always carry settledBy"
            );
            assert_eq!(
                printed["settledBy"],
                row["settledBy"].clone(),
                "{name}: settledBy"
            );
            let awaiting_link = printed["awaiting"]
                .as_object()
                .map(|awaiting| awaiting["link"].clone());
            assert_eq!(
                awaiting_link,
                row["awaiting"]
                    .as_object()
                    .map(|awaiting| awaiting["link"].clone()),
                "{name}: awaiting.link"
            );
            if row["settled"].as_bool() == Some(true) {
                assert!(
                    printed["awaiting"].is_null(),
                    "{name}: a settled assignment awaits nothing"
                );
            }
        }
    }
}

// ── drift travels with a completion and never gates it (A10 § 2) ───────────

/// The same projection at `drifted` and at `none` reaches the **same**
/// verdict.
///
/// Until this test, every completion case here pinned `planDrift` at
/// `unknown`, so A10 § 2's two claims about completion — the fact travels,
/// and it never refuses — were shipped untested (refuter, 2026-09-22).
fn drifted(complete: bool) -> WorkProjection {
    let mut coverage = projection(
        WorkDeclarationState::Head,
        vec![criterion(
            "cli-behaviour",
            if complete {
                WorkCriterionStatus::Covered
            } else {
                WorkCriterionStatus::Open
            },
        )],
        complete,
    );
    coverage.declarations[0].plan_drift = beekeeper_core::project_work_fold::WorkPlanDrift {
        declared_commit: "ab".repeat(20),
        current_commit: Some("fe".repeat(20)),
        state: beekeeper_core::project_work_fold::WorkPlanDriftState::Drifted,
    };
    coverage
}

/// A completed contract whose agents repository has moved on still completes:
/// the gate finds nothing incomplete, exactly as it does with no drift.
#[test]
fn a_drifted_declaration_never_blocks_a_completion() {
    assert!(incomplete_head(&drifted(true)).is_none());
    // And the drift is not what makes an incomplete one refuse: the refusal
    // is about the criterion, and it reads the same either way.
    let open = incomplete_head(&drifted(false)).expect("the open criterion refuses");
    let without_drift = incomplete_head(&projection(
        WorkDeclarationState::Head,
        vec![criterion("cli-behaviour", WorkCriterionStatus::Open)],
        false,
    ))
    .expect("the same criterion refuses");
    assert_eq!(open.message, without_drift.message);
    assert_eq!(open.criteria, without_drift.criteria);
}

/// The completion result carries the fact: both commits and the re-adopt
/// command, from the same renderer `work status` prints.
#[test]
fn a_completion_carries_the_drift_fact_with_both_commits() {
    let coverage = drifted(true);
    let declaration = &coverage.declarations[0];
    let line = crate::commands::sessions::work::plan_drift_line(
        declaration,
        "22222222-3333-4444-8555-666666666666",
        SESSION,
    )
    .expect("the completion path prints this line");
    assert!(line.starts_with("agents repo main moved on:"), "{line}");
    assert!(line.contains(&"ab".repeat(20)[..12]), "{line}");
    assert!(line.contains(&"fe".repeat(20)[..12]), "{line}");
    assert!(line.contains("bee sessions work adopt"), "{line}");
    assert!(
        line.contains(&format!("--supersedes {}", declaration.declaration_ref)),
        "{line}"
    );
    // The disclosure the completion path emits says it refuses nothing.
    assert!(!line.contains("refus"), "{line}");
}
