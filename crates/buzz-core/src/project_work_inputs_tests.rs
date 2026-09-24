//! Tests for the shared input assembler.
//!
//! **What moved here at A5.** The assembler now derives the canonical 44244
//! projection by *calling* the team fold, which verifies signatures and keys
//! every record by its own event id. The frozen sequences state their facts
//! with fixed fake ids, so a raw set cannot be rebuilt from them any more —
//! a real signed report's id is the hash of its bytes, never `1ea1…`. The
//! 18-sequence oracle therefore lives in `project_work_fold_tests.rs`, where
//! the fold reads the contract's stated input directly, and this file proves
//! the *derivation*: real signed kind:44244 events in, the projection and the
//! facts the fold needs out, with the two cases A5 added
//! (`report_not_canonical`, `disposition_not_canonical`) reproduced
//! end-to-end through `assemble_fold_inputs` + `fold_work`.
//!
//! The parts of the sequences that are not id-bound — goals, ref states,
//! authority, plan-blob keys and the action-definition keys finding 7 is
//! about — are still checked against the fixtures.

use super::*;

use serde_json::{json, Value};

use crate::coding_session_team_transaction::{
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision, CodingSessionTeamReport,
    CodingSessionTeamTransactionPayload, CodingSessionTeamTransactionType,
    CodingSessionTeamVerdict, CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use crate::project_work::{
    ProjectWorkAssignmentBound, ProjectWorkBody, ProjectWorkDeclared, ProjectWorkEvidenceBound,
    ProjectWorkEvidenceKind, ProjectWorkEvidenceRef, ProjectWorkPayload, ProjectWorkPlanRef,
    PROJECT_WORK_SCHEMA,
};
use crate::project_work_fold::{fold_work, WorkCriterionStatus, WorkReasonCode};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

macro_rules! sequence {
    ($name:literal) => {
        Sequence {
            name: $name,
            inputs: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/inputs.json"
            )),
        }
    };
}

struct Sequence {
    name: &'static str,
    inputs: &'static str,
}

const KETTLE_PLAN: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const SEQUENCES: [Sequence; 37] = [
    sequence!("happy-path"),
    sequence!("amendment"),
    sequence!("fork"),
    sequence!("fork-descendant"),
    sequence!("fork-two-roots"),
    sequence!("superseded-observation"),
    sequence!("goal-changed"),
    sequence!("goal-ref-not-a-goal"),
    sequence!("evidence-refusals"),
    sequence!("action-hash-mismatch"),
    sequence!("action-failed"),
    sequence!("action-dirty"),
    sequence!("mixed-artifacts"),
    sequence!("wrong-assignee-report"),
    sequence!("superseded-disposition"),
    sequence!("same-action-two-commits"),
    sequence!("same-action-two-commits-reversed"),
    sequence!("plan-unavailable-before-bindings"),
    sequence!("action-dirty-after"),
    sequence!("action-no-host-result"),
    sequence!("action-not-compiled"),
    sequence!("action-wrong-echo-signer"),
    sequence!("action-wrong-step"),
    sequence!("canonical-exclusions"),
    sequence!("criterion-unassigned-report"),
    sequence!("plan-unreadable-after-bindings"),
    sequence!("projection-empty"),
    sequence!("relay-self-key-absent"),
    sequence!("report-absent-assignment"),
    sequence!("review-unresolved-and-unanswered"),
    sequence!("plan-drift-none"),
    sequence!("plan-drift-drifted"),
    sequence!("plan-drift-superseded"),
    sequence!("plan-drift-unknown"),
    sequence!("plan-drift-on-completed"),
    sequence!("ref-observation-rebound"),
    sequence!("ref-observation-wrong-ref"),
];

const CHANNEL: &str = "22222222-3333-4444-8555-666666666666";
const SESSION: &str = "11111111-2222-4333-8444-555555555555";

/// Split a fold blob key `<coordinate>@<commit>:<path>` back into its parts.
fn split_blob_key(key: &str) -> (String, String, String) {
    let (repository, rest) = key.rsplit_once('@').expect("a blob key names a commit");
    let (commit, path) = rest.split_once(':').expect("a blob key names a path");
    (repository.to_owned(), commit.to_owned(), path.to_owned())
}

/// Split an action-definition key `<coordinate>@<commit>#<name>`.
fn split_action_key(key: &str) -> (String, String, String) {
    let (repository, rest) = key.rsplit_once('@').expect("an action key names a commit");
    let (commit, name) = rest.split_once('#').expect("an action key names an action");
    (repository.to_owned(), commit.to_owned(), name.to_owned())
}

fn event(id: &str, pubkey: &str, created_at: u64, kind: u32, content: Value) -> ProjectWorkEvent {
    ProjectWorkEvent {
        id: id.to_owned(),
        pubkey: pubkey.to_owned(),
        created_at,
        kind,
        tags: Vec::new(),
        content: content.to_string(),
    }
}

/// A kind:44227 goal event in the exact envelope the goal validator accepts.
fn goal_event(id: &str, created_at: u64) -> ProjectWorkEvent {
    ProjectWorkEvent {
        id: id.to_owned(),
        pubkey: "1ead000000000000000000000000000000000000000000000000000000000000".to_owned(),
        created_at,
        kind: KIND_CODING_SESSION_GOAL,
        tags: vec![
            vec!["h".into(), CHANNEL.into()],
            vec!["d".into(), SESSION.into()],
            vec!["csgl-v".into(), CODING_SESSION_GOAL_TAG_VERSION.into()],
        ],
        content: "ship the kettle".to_owned(),
    }
}

/// Everything the assembler is handed for one sequence, minus the team
/// records: those are id-bound and are exercised by the scenarios below.
fn raw_from_fixture(sequence: &Sequence) -> RawWorkInputs {
    let fixture: Value = serde_json::from_str(sequence.inputs).expect("sequence inputs");
    let current = fixture["currentGoalRef"].as_str().map(str::to_owned);
    let mut goal_events = Vec::new();
    for (index, id) in fixture["goalEvents"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let id = id.as_str().expect("goal id");
        let created_at = if Some(id.to_owned()) == current {
            1_789_000_900
        } else {
            1_789_000_100 + index as u64
        };
        goal_events.push(goal_event(id, created_at));
    }

    let empty = serde_json::Map::new();
    let mut host_results = Vec::new();
    let mut host_echoes = Vec::new();
    let mut host_requests = Vec::new();
    for fact in fixture["evidence"].as_object().unwrap_or(&empty).values() {
        if fact["kind"].as_str() != Some("action_result") {
            continue;
        }
        host_results.push(event(
            fact["eventId"].as_str().expect("event id"),
            fact["resultSigner"].as_str().expect("result signer"),
            1_789_000_400,
            KIND_HOST_STEP_RESULT,
            json!({
                "schema": "buzz-host-step/v1",
                "runId": fact["runId"],
                "stepId": fact["stepId"],
                "disposition": fact["disposition"],
                "exitCode": fact["exitCode"],
                "dirty": fact["dirty"],
                "checkout": fact["checkout"],
            }),
        ));
        host_echoes.push(event(
            fact["exitedEventId"].as_str().expect("echo id"),
            fact["echoSigner"].as_str().expect("echo signer"),
            1_789_000_410,
            KIND_WORKFLOW_HOST_STEP_EXITED,
            json!({
                "schema": "buzz-host-step/v1",
                "resultEventId": fact["eventId"],
                "claimedBy": fact["resultSigner"],
            }),
        ));
        host_requests.push(event(
            &format!("{:0>64}", fact["eventId"].as_str().unwrap_or("9e9")),
            fact["echoSigner"].as_str().expect("echo signer"),
            1_789_000_390,
            KIND_WORKFLOW_HOST_STEP_REQUESTED,
            json!({
                "schema": "buzz-host-step/v1",
                "runId": fact["runId"],
                "stepId": fact["stepId"],
                "workflowName": fact["actionName"],
                "definitionHash": fact["definitionHash"],
            }),
        ));
    }

    let mut plan_blobs = BTreeMap::new();
    for key in fixture["planBlobs"].as_object().unwrap_or(&empty).keys() {
        let (repository, commit, path) = split_blob_key(key);
        plan_blobs.insert((repository, commit, path), KETTLE_PLAN.to_owned());
    }
    let mut action_definitions = BTreeMap::new();
    for (key, definition) in fixture["actionDefinitions"].as_object().unwrap_or(&empty) {
        action_definitions.insert(
            split_action_key(key),
            serde_json::from_value(definition.clone()).expect("action definition"),
        );
    }

    RawWorkInputs {
        work_events: Vec::new(),
        team_events: Vec::new(),
        host_results,
        host_echoes,
        host_requests,
        ref_states: serde_json::from_value(fixture["refStates"].clone()).expect("ref states"),
        goal_events,
        authority: RawAuthorityContext {
            channel_ref: Some(CHANNEL.to_owned()),
            genesis_ref: None,
            genesis_event: None,
            founder_pubkey: fixture["authority"]["founderPubkey"]
                .as_str()
                .map(str::to_owned),
            active_seats: serde_json::from_value(fixture["authority"]["activeSeats"].clone())
                .expect("seats"),
            active_grants: serde_json::from_value(fixture["authority"]["activeGrants"].clone())
                .expect("grants"),
        },
        relay_self_key: fixture["relaySelfKey"].as_str().map(str::to_owned),
        plan_blobs,
        action_definitions,
        session_ref: Some(SESSION.to_owned()),
        project_ref: None,
    }
}

/// Every sequence's non-id-bound inputs, rebuilt from raw events.
#[test]
fn assembling_a_raw_event_set_reproduces_every_sequence_input() {
    for sequence in &SEQUENCES {
        let fixture: Value = serde_json::from_str(sequence.inputs).expect("sequence inputs");
        let assembled =
            assemble_fold_inputs(raw_from_fixture(sequence)).expect("the fixture assembles");

        assert_eq!(
            assembled.relay_self_key.as_deref(),
            fixture["relaySelfKey"].as_str(),
            "{}: relaySelfKey",
            sequence.name
        );
        assert_eq!(
            assembled.current_goal_ref.as_deref(),
            fixture["currentGoalRef"].as_str(),
            "{}: currentGoalRef",
            sequence.name
        );
        let expected_goals: BTreeSet<String> =
            serde_json::from_value(fixture["goalEvents"].clone()).unwrap_or_default();
        assert_eq!(
            assembled.goal_events, expected_goals,
            "{}: goalEvents",
            sequence.name
        );
        assert_eq!(
            serde_json::to_value(&assembled.authority).expect("authority"),
            fixture["authority"],
            "{}: authority",
            sequence.name
        );
        // Compared as events, not as bytes: a ref-state row may omit its
        // empty `content`, which the checker reads as `""`
        // (`check-fixtures.mjs:190`), and re-serializing writes it back.
        let expected_states: Vec<ProjectWorkEvent> =
            serde_json::from_value(fixture["refStates"].clone()).expect("ref states");
        assert_eq!(
            assembled.ref_states, expected_states,
            "{}: refStates",
            sequence.name
        );
        let blob_keys: Vec<&String> = assembled.plan_blobs.keys().collect();
        let expected_keys: Vec<&String> = fixture["planBlobs"]
            .as_object()
            .map(|blobs| blobs.keys().collect())
            .unwrap_or_default();
        assert_eq!(blob_keys, expected_keys, "{}: planBlobs", sequence.name);
        // Finding 7: the key an action definition is held under keeps its
        // repository and commit, exactly as the contract writes it.
        assert_eq!(
            serde_json::to_value(&assembled.action_definitions).expect("definitions"),
            if fixture["actionDefinitions"].is_null() {
                json!({})
            } else {
                fixture["actionDefinitions"].clone()
            },
            "{}: actionDefinitions",
            sequence.name
        );
        // The action-result facts the fixture states are still derived from
        // their raw 46023/46014/46013 triples.
        for (event_id, fact) in fixture["evidence"]
            .as_object()
            .unwrap_or(&serde_json::Map::new())
            .iter()
            .filter(|(_, fact)| fact["kind"].as_str() == Some("action_result"))
        {
            assert_eq!(
                serde_json::to_value(assembled.evidence.get(event_id).expect("a derived fact"))
                    .expect("fact"),
                *fact,
                "{}: {event_id}",
                sequence.name
            );
        }
    }
}

// ── the canonical projection, from real signed records ─────────────────────

/// One signed kind:44244 record in the exact five-tag envelope.
fn team_event(
    keys: &Keys,
    genesis: &str,
    transaction_type: CodingSessionTeamTransactionType,
    body: crate::coding_session_team_transaction::CodingSessionTeamTransactionBody,
    created_at: u64,
    supersedes: Option<String>,
) -> nostr::Event {
    let payload = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: genesis.to_owned(),
        transaction_type,
        supersedes,
        delivery_command_id: None,
        body,
    };
    payload.validate().expect("a valid team transaction");
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(&payload).expect("payload"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h"),
        Tag::parse(["d", SESSION]).expect("d"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("v"),
        Tag::parse(["cstx-genesis", genesis]).expect("genesis"),
        Tag::parse(["cstx-type", transaction_type.as_str()]).expect("type"),
    ])
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(keys)
    .expect("signed")
}

fn assignment_body(
    assignee: &str,
) -> crate::coding_session_team_transaction::CodingSessionTeamTransactionBody {
    crate::coding_session_team_transaction::CodingSessionTeamTransactionBody::Assignment(
        CodingSessionTeamAssignment {
            assignee_actor: assignee.to_owned(),
            assignee_role: "builder".to_owned(),
            objective: "ship the kettle CLI".to_owned(),
            brief: "build it".to_owned(),
            branch: None,
            base_sha: None,
            file_ownership: vec!["crates/buzz-cli/".to_owned()],
            acceptance_steps: vec!["cargo test".to_owned()],
        },
    )
}

fn report_body(
    assignment_ref: &str,
    head_sha: &str,
) -> crate::coding_session_team_transaction::CodingSessionTeamTransactionBody {
    crate::coding_session_team_transaction::CodingSessionTeamTransactionBody::Report(
        CodingSessionTeamReport {
            assignment_ref: assignment_ref.to_owned(),
            summary: "done".to_owned(),
            branch: None,
            base_sha: None,
            head_sha: Some(head_sha.to_owned()),
            files: vec!["crates/buzz-cli/src/lib.rs".to_owned()],
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        },
    )
}

fn disposition_body(
    assignment_ref: &str,
    report_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
) -> crate::coding_session_team_transaction::CodingSessionTeamTransactionBody {
    crate::coding_session_team_transaction::CodingSessionTeamTransactionBody::Verdict(
        CodingSessionTeamVerdict::Disposition {
            assignment_ref: assignment_ref.to_owned(),
            report_ref: report_ref.to_owned(),
            refutation_ref: None,
            decision,
            summary: "ruled".to_owned(),
            findings: Vec::new(),
            required_action: None,
        },
    )
}

/// One signed kind:44249 record.
fn work_event(
    keys: &Keys,
    genesis: &str,
    body: ProjectWorkBody,
    created_at: u64,
) -> ProjectWorkEvent {
    let payload = ProjectWorkPayload {
        schema: PROJECT_WORK_SCHEMA.to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: genesis.to_owned(),
        project_ref: format!("30621:{}:kettle", keys.public_key().to_hex()),
        record_type: body.record_type(),
        body,
    };
    let content = payload.canonical_content().expect("content");
    let tags = payload.canonical_tags(CHANNEL);
    let event = EventBuilder::new(Kind::Custom(KIND_PROJECT_WORK_RECORD as u16), content)
        .tags(
            tags.iter()
                .map(|parts| Tag::parse(parts.clone()).expect("tag"))
                .collect::<Vec<_>>(),
        )
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("signed");
    ProjectWorkEvent::from(&event)
}

/// A whole session built from real signed records: the founder leads, one
/// builder owns the assignment, and the plan has one `review` criterion.
struct Scenario {
    lead: Keys,
    builder: Keys,
    genesis: String,
    raw: RawWorkInputs,
    declaration: String,
    assignment: String,
}

const REVIEW_PLAN: &str = r#"---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: the CLI does what the plan says
    proof: {kind: review}
retired_criteria: []
---

Body.
"#;

impl Scenario {
    fn new() -> Self {
        let lead = Keys::generate();
        let builder = Keys::generate();
        let genesis = "ab".repeat(32);
        let repository = format!(
            "30617:{}:kettle-beekeeper-agents",
            lead.public_key().to_hex()
        );
        let commit = "ab".repeat(20);
        let goal_id = "90a1".to_owned() + &"0".repeat(60);

        let assignment = team_event(
            &lead,
            &genesis,
            CodingSessionTeamTransactionType::Assignment,
            assignment_body(&builder.public_key().to_hex()),
            1_789_000_100,
            None,
        );
        let declared = work_event(
            &lead,
            &genesis,
            ProjectWorkBody::Declared(ProjectWorkDeclared {
                work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
                goal_ref: goal_id.clone(),
                decision_ref: None,
                responsible_actor: builder.public_key().to_hex(),
                plan_ref: ProjectWorkPlanRef {
                    repository: repository.clone(),
                    commit: commit.clone(),
                    path: "plans/kettle.md".to_owned(),
                },
                supersedes: Vec::new(),
            }),
            1_789_000_200,
        );
        let bound = work_event(
            &lead,
            &genesis,
            ProjectWorkBody::AssignmentBound(ProjectWorkAssignmentBound {
                declaration_ref: declared.id.clone(),
                criterion_ids: vec!["cli-behaviour".to_owned()],
                assignment_ref: assignment.id.to_hex(),
                replaces_binding: None,
            }),
            1_789_000_210,
        );
        let mut plan_blobs = BTreeMap::new();
        plan_blobs.insert(
            (repository, commit, "plans/kettle.md".to_owned()),
            REVIEW_PLAN.to_owned(),
        );
        let raw = RawWorkInputs {
            work_events: vec![declared.clone(), bound],
            team_events: vec![assignment.clone()],
            goal_events: vec![goal_event(&goal_id, 1_789_000_050)],
            authority: RawAuthorityContext {
                channel_ref: Some(CHANNEL.to_owned()),
                genesis_ref: Some(genesis.clone()),
                genesis_event: None,
                founder_pubkey: Some(lead.public_key().to_hex()),
                active_seats: Vec::new(),
                active_grants: Vec::new(),
            },
            relay_self_key: Some("4e".repeat(32)),
            plan_blobs,
            session_ref: Some(SESSION.to_owned()),
            project_ref: Some(format!("30621:{}:kettle", lead.public_key().to_hex())),
            ..RawWorkInputs::default()
        };
        Self {
            declaration: declared.id.clone(),
            assignment: assignment.id.to_hex(),
            lead,
            builder,
            genesis,
            raw,
        }
    }

    /// Bind evidence to the one criterion, at the artifact commit.
    fn bind_evidence(&mut self, refs: Vec<ProjectWorkEvidenceRef>, artifact: &str) {
        let bound = work_event(
            &self.lead,
            &self.genesis,
            ProjectWorkBody::EvidenceBound(ProjectWorkEvidenceBound {
                declaration_ref: self.declaration.clone(),
                criterion_ids: vec!["cli-behaviour".to_owned()],
                artifact_commit: artifact.to_owned(),
                evidence_refs: refs,
                completion_ref: None,
            }),
            1_789_000_400,
        );
        self.raw.work_events.push(bound);
    }

    /// Fold what the assembler makes of it, and return the one criterion.
    fn criterion(self) -> (WorkCriterionStatus, Option<WorkReasonCode>, String) {
        let inputs = assemble_fold_inputs(self.raw).expect("assembles");
        let projection = fold_work(&inputs);
        let declaration = projection
            .declarations
            .first()
            .expect("one declaration")
            .clone();
        let criterion = declaration.criteria.first().expect("one criterion").clone();
        (
            criterion.status,
            criterion.reason_code,
            criterion.reason.unwrap_or_default(),
        )
    }
}

/// The canonical chain covers the criterion: the assignee reported, the lead
/// approved, and the team fold includes both.
#[test]
fn a_canonical_report_and_approval_cover_the_criterion() {
    let mut scenario = Scenario::new();
    let artifact = "e7".repeat(20);
    let report = team_event(
        &scenario.builder,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Report,
        report_body(&scenario.assignment, &artifact),
        1_789_000_300,
        None,
    );
    let disposition = team_event(
        &scenario.lead,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Verdict,
        disposition_body(
            &scenario.assignment,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        1_789_000_310,
        None,
    );
    scenario.bind_evidence(
        vec![
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Report,
                event_id: report.id.to_hex(),
            },
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Verdict,
                event_id: disposition.id.to_hex(),
            },
        ],
        &artifact,
    );
    scenario.raw.team_events.push(report);
    scenario.raw.team_events.push(disposition);
    let (status, reason_code, _) = scenario.criterion();
    assert_eq!(status, WorkCriterionStatus::Covered);
    assert_eq!(reason_code, None);
}

/// **Reproduces finding 6.** A channel peer publishes a well-formed report
/// about the builder's assignment and the lead approves and binds it. The
/// team fold excludes the report — its signer is not the assignee — and work
/// coverage must reach the same answer, not a second one.
#[test]
fn a_report_by_someone_who_is_not_the_assignee_is_not_evidence() {
    let mut scenario = Scenario::new();
    let peer = Keys::generate();
    let artifact = "e7".repeat(20);
    let report = team_event(
        &peer,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Report,
        report_body(&scenario.assignment, &artifact),
        1_789_000_300,
        None,
    );
    let disposition = team_event(
        &scenario.lead,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Verdict,
        disposition_body(
            &scenario.assignment,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        1_789_000_310,
        None,
    );
    scenario.bind_evidence(
        vec![
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Report,
                event_id: report.id.to_hex(),
            },
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Verdict,
                event_id: disposition.id.to_hex(),
            },
        ],
        &artifact,
    );
    scenario.raw.team_events.push(report);
    scenario.raw.team_events.push(disposition);
    let (status, reason_code, reason) = scenario.criterion();
    assert_eq!(status, WorkCriterionStatus::Open);
    assert_eq!(reason_code, Some(WorkReasonCode::ReportNotCanonical));
    assert!(reason.contains("assignee"), "{reason}");
}

/// **Reproduces finding 6's other half.** An approval the lead later replaced
/// with changes-requested is history: the projection carries the replacement,
/// and a binding still naming the old ruling proves nothing.
#[test]
fn an_approval_a_later_ruling_replaced_is_not_evidence() {
    let mut scenario = Scenario::new();
    let artifact = "e7".repeat(20);
    let report = team_event(
        &scenario.builder,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Report,
        report_body(&scenario.assignment, &artifact),
        1_789_000_300,
        None,
    );
    let approval = team_event(
        &scenario.lead,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Verdict,
        disposition_body(
            &scenario.assignment,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::Approve,
        ),
        1_789_000_310,
        None,
    );
    let replacement = team_event(
        &scenario.lead,
        &scenario.genesis,
        CodingSessionTeamTransactionType::Verdict,
        disposition_body(
            &scenario.assignment,
            &report.id.to_hex(),
            CodingSessionTeamDispositionDecision::ChangesRequested,
        ),
        1_789_000_320,
        Some(approval.id.to_hex()),
    );
    scenario.bind_evidence(
        vec![
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Report,
                event_id: report.id.to_hex(),
            },
            ProjectWorkEvidenceRef {
                kind: ProjectWorkEvidenceKind::Verdict,
                event_id: approval.id.to_hex(),
            },
        ],
        &artifact,
    );
    scenario.raw.team_events.push(report);
    scenario.raw.team_events.push(approval);
    scenario.raw.team_events.push(replacement.clone());
    let (status, reason_code, reason) = scenario.criterion();
    assert_eq!(status, WorkCriterionStatus::Open);
    assert_eq!(reason_code, Some(WorkReasonCode::DispositionNotCanonical));
    assert!(
        reason.contains(&replacement.id.to_hex()[..8]),
        "the refusal names the ruling that replaced it: {reason}"
    );
}

/// A team fold that refuses the supplied set is a refusal here: carrying on
/// would admit records the team contract excludes.
#[test]
fn a_team_fold_refusal_refuses_the_assembly() {
    let scenario = Scenario::new();
    let mut raw = scenario.raw;
    // A record from another session: the team fold refuses the whole set.
    let stranger = team_event(
        &scenario.lead,
        &"cd".repeat(32),
        CodingSessionTeamTransactionType::Assignment,
        assignment_body(&scenario.builder.public_key().to_hex()),
        1_789_000_150,
        None,
    );
    raw.team_events.push(stranger);
    let refusal = assemble_fold_inputs(raw).expect_err("refused");
    assert_eq!(refusal.code, AssembleRefusalCode::TeamProjectionRefused);
    assert_eq!(refusal.code.as_str(), "team_projection_refused");
}

/// The projection carries the assignee of every included assignment, read
/// from the record the projection includes.
#[test]
fn the_projection_names_each_assignments_assignee() {
    let scenario = Scenario::new();
    let assignment = scenario.assignment.clone();
    let builder = scenario.builder.public_key().to_hex();
    let inputs = assemble_fold_inputs(scenario.raw).expect("assembles");
    assert_eq!(
        inputs.team_projection.assignee(&assignment),
        Some(builder.as_str())
    );
    assert!(inputs.team_projection.includes(&assignment));
}

/// The founder is the genesis **signer**, not a field anyone asserts.
#[test]
fn the_founder_comes_from_the_genesis_signer() {
    let founder = "1ead000000000000000000000000000000000000000000000000000000000000";
    let raw = RawWorkInputs {
        authority: RawAuthorityContext {
            genesis_event: Some(event(
                &format!("{:0>64}", "9e5"),
                founder,
                1_789_000_000,
                44222,
                json!({}),
            )),
            // Deliberately disagreeing: the signer wins, and silently.
            founder_pubkey: Some("f".repeat(64)),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    };
    let assembled = assemble_fold_inputs(raw).expect("assembles");
    assert_eq!(assembled.authority.founder_pubkey, founder);
}

/// With no founder at all, every record would be excluded as an unauthorized
/// signer and the projection would read as "nobody did anything". That is a
/// refusal, not an empty answer.
#[test]
fn an_input_set_with_no_founder_is_refused() {
    let refusal = assemble_fold_inputs(RawWorkInputs::default()).expect_err("refused");
    assert_eq!(refusal.code, AssembleRefusalCode::FounderUnknown);
    assert_eq!(refusal.code.as_str(), "founder_unknown");

    let malformed = assemble_fold_inputs(RawWorkInputs {
        authority: RawAuthorityContext {
            founder_pubkey: Some("not-a-pubkey".to_owned()),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    })
    .expect_err("refused");
    assert_eq!(malformed.code, AssembleRefusalCode::FounderMalformed);
}

/// An owner-signed claim about its own branch is not an observation.
#[test]
fn a_ref_state_not_signed_by_the_relay_is_dropped() {
    let relay = "4e1a000000000000000000000000000000000000000000000000000000000000";
    let mut owner_signed = event(
        &format!("{:0>64}", "0b5"),
        "1ead000000000000000000000000000000000000000000000000000000000000",
        1_789_000_050,
        KIND_GIT_REPO_STATE,
        json!({}),
    );
    owner_signed.tags = vec![vec!["d".into(), "pivot-test".into()]];
    let mut relay_signed = owner_signed.clone();
    relay_signed.id = format!("{:0>64}", "0b6");
    relay_signed.pubkey = relay.to_owned();

    let assembled = assemble_fold_inputs(RawWorkInputs {
        ref_states: vec![owner_signed, relay_signed.clone()],
        relay_self_key: Some(relay.to_owned()),
        authority: RawAuthorityContext {
            founder_pubkey: Some("1".repeat(64)),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    })
    .expect("assembles");
    assert_eq!(assembled.ref_states, vec![relay_signed]);
}

/// A host result the relay never echoed is a host's self-report. It yields no
/// fact, so the criterion that needed it reads `unknown` rather than passing.
#[test]
fn a_host_result_with_no_echo_yields_no_fact() {
    let result = event(
        &format!("{:0>64}", "ac7"),
        &"80".repeat(32),
        1_789_000_400,
        KIND_HOST_STEP_RESULT,
        json!({"runId": "77777777-8888-4999-8aaa-bbbbbbbbbbbb", "stepId": "verify",
               "disposition": "exited", "exitCode": 0, "dirty": false,
               "checkout": {"mode": "commit e7", "sha": "e7".repeat(20), "dirtyBefore": false}}),
    );
    let assembled = assemble_fold_inputs(RawWorkInputs {
        host_results: vec![result],
        authority: RawAuthorityContext {
            founder_pubkey: Some("1".repeat(64)),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    })
    .expect("assembles");
    assert!(assembled.evidence.is_empty());
}

/// An absent exit code is not a zero, and an unread pre-execution sample is
/// not a clean tree. A result missing either must never read as a pass.
#[test]
fn a_result_missing_its_exit_code_or_checkout_never_reads_as_a_pass() {
    let run = "77777777-8888-4999-8aaa-bbbbbbbbbbbb";
    let result_id = format!("{:0>64}", "ac8");
    let assembled = assemble_fold_inputs(RawWorkInputs {
        host_results: vec![event(
            &result_id,
            &"80".repeat(32),
            1_789_000_400,
            KIND_HOST_STEP_RESULT,
            json!({"runId": run, "stepId": "verify", "disposition": "refused"}),
        )],
        host_echoes: vec![event(
            &format!("{:0>64}", "ec8"),
            &"4e".repeat(32),
            1_789_000_410,
            KIND_WORKFLOW_HOST_STEP_EXITED,
            json!({"resultEventId": result_id}),
        )],
        host_requests: vec![event(
            &format!("{:0>64}", "9e9"),
            &"4e".repeat(32),
            1_789_000_390,
            KIND_WORKFLOW_HOST_STEP_REQUESTED,
            json!({"runId": run, "stepId": "verify", "workflowName": "verify",
                   "definitionHash": "d1".repeat(32)}),
        )],
        authority: RawAuthorityContext {
            founder_pubkey: Some("1".repeat(64)),
            ..RawAuthorityContext::default()
        },
        ..RawWorkInputs::default()
    })
    .expect("assembles");
    let fact = assembled.evidence.get(&result_id).expect("one fact");
    match fact {
        WorkEvidenceFact::ActionResult {
            exit_code,
            checkout,
            dirty,
            ..
        } => {
            assert_ne!(*exit_code, 0);
            assert!(checkout.dirty_before);
            assert!(*dirty);
        }
        other => panic!("expected an action result, got {other:?}"),
    }
}

/// A report that names no revision cannot say a criterion was met at one.
#[test]
fn a_report_with_no_head_sha_yields_no_fact() {
    let lead = Keys::generate();
    let genesis = "ab".repeat(32);
    let mut body = report_body(&"a5".repeat(32), &"e7".repeat(20));
    if let crate::coding_session_team_transaction::CodingSessionTeamTransactionBody::Report(
        report,
    ) = &mut body
    {
        report.head_sha = None;
    }
    let report = team_event(
        &lead,
        &genesis,
        CodingSessionTeamTransactionType::Report,
        body,
        1_789_000_300,
        None,
    );
    let assembled = assemble_fold_inputs(RawWorkInputs {
        team_events: vec![report],
        authority: RawAuthorityContext {
            channel_ref: Some(CHANNEL.to_owned()),
            genesis_ref: Some(genesis),
            founder_pubkey: Some(lead.public_key().to_hex()),
            ..RawAuthorityContext::default()
        },
        session_ref: Some(SESSION.to_owned()),
        ..RawWorkInputs::default()
    })
    .expect("assembles");
    assert!(assembled.evidence.is_empty());
}

/// Permuting every raw list changes nothing: the assembler is pure and its
/// output collections are ordered by the events' own keys.
#[test]
fn permuting_the_raw_input_changes_nothing() {
    for sequence in &SEQUENCES {
        let straight = assemble_fold_inputs(raw_from_fixture(sequence)).expect("assembles");
        let mut reversed = raw_from_fixture(sequence);
        reversed.work_events.reverse();
        reversed.team_events.reverse();
        reversed.host_results.reverse();
        reversed.host_echoes.reverse();
        reversed.host_requests.reverse();
        reversed.ref_states.reverse();
        reversed.goal_events.reverse();
        let permuted = assemble_fold_inputs(reversed).expect("assembles");
        assert_eq!(straight.evidence, permuted.evidence, "{}", sequence.name);
        assert_eq!(
            straight.ref_states, permuted.ref_states,
            "{}",
            sequence.name
        );
        assert_eq!(
            straight.current_goal_ref, permuted.current_goal_ref,
            "{}",
            sequence.name
        );
        assert_eq!(
            serde_json::to_value(fold_work(&straight)).expect("projection"),
            serde_json::to_value(fold_work(&permuted)).expect("projection"),
            "{}",
            sequence.name
        );
    }
}
