//! The host's automatic evidence binding (ledger 257(d)): what it signs, that
//! the records are the CLI's records under this host's key, and every case in
//! which it binds nothing.

use std::cell::Cell;

use buzz_core::kind::KIND_GIT_REPO_STATE;
use buzz_core::project_work::{
    decode_project_work_content, ProjectWorkBody, ProjectWorkDeclared, ProjectWorkEvidenceKind,
};
use buzz_sdk::project_work::build_project_work_declared;

use super::*;
use crate::host_result_wake::is_host_result_pointer;
use crate::host_result_wake::HostResultWakeStore;

const KETTLE: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const REVIEW_ONLY: &str = r#"---
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
  - id: delivered-main
    accept: it is on main
    proof: {kind: git-ref}
retired_criteria: []
---

Body.
"#;

const DELIVERED: &str = "d7b4a649c3399e5c4da95753220bb6712bcfae5e";
const SEED: &str = "7070283000000000000000000000000000000000";
const RESULT: &str = "4623000000000000000000000000000000000000000000000000000000000000";
const REF_STATE: &str = "3061800000000000000000000000000000000000000000000000000000000000";

fn envelope() -> ProjectWorkEnvelope {
    ProjectWorkEnvelope {
        channel_ref: "56ef0396-f1d0-4cbd-9cc2-4f4fb592e823".into(),
        session_ref: "ce7d32cb-b703-4db8-b06e-2adbd2b942b3".into(),
        genesis_ref: "12".repeat(32),
        project_ref: format!("30621:{}:kettle", "3d".repeat(32)),
    }
}

/// A fake relay and clone: each read answers from a field and counts itself.
struct Fixture {
    may_bind: bool,
    plan: &'static str,
    records: Vec<ProjectWorkEvent>,
    delivered: &'static str,
    relay: Keys,
    ref_reads: Cell<u32>,
}

impl Fixture {
    fn new(lead: &Keys) -> Self {
        let declared = build_project_work_declared(
            &envelope(),
            ProjectWorkDeclared {
                work_id: "0b6f6a3c-7c1a-5a8e-9c2d-0a1b2c3d4e5f".into(),
                goal_ref: "cc".repeat(32),
                decision_ref: None,
                responsible_actor: lead.public_key().to_hex(),
                plan_ref: ProjectWorkPlanRef {
                    repository: format!("30617:{}:kettle-agents", "3d".repeat(32)),
                    commit: "21ece7f946797575e79902ae8b1002913be0152c".into(),
                    path: "plans/kettle.md".into(),
                },
                supersedes: Vec::new(),
            },
        )
        .expect("declared")
        .sign_with_keys(lead)
        .expect("sign");
        Self {
            may_bind: true,
            plan: KETTLE,
            records: vec![ProjectWorkEvent::from(&declared)],
            delivered: DELIVERED,
            relay: Keys::generate(),
            ref_reads: Cell::new(0),
        }
    }

    fn input<'a>(&'a self, keys: &'a Keys, relay_self: &'a str) -> AutoEvidenceInput<'a> {
        AutoEvidenceInput {
            keys,
            relay_self,
            envelope: envelope(),
            result: HostResultFacts {
                result_event_id: RESULT.into(),
                action_name: "verify".into(),
                step_id: "verify".into(),
                exited: true,
                exit_code: Some(0),
                dirty: Some(false),
                head_sha: Some(DELIVERED.into()),
            },
        }
    }
}

impl AutoEvidenceReads for Fixture {
    async fn provider_may_bind(&self, _provider: &str) -> Result<bool, String> {
        Ok(self.may_bind)
    }

    async fn work_records(&self) -> Result<Vec<ProjectWorkEvent>, String> {
        Ok(self.records.clone())
    }

    async fn plan_blob(&self, _plan_ref: &ProjectWorkPlanRef) -> Result<String, String> {
        Ok(self.plan.to_owned())
    }

    async fn ref_states(&self, repository: &str) -> Result<Vec<ProjectWorkEvent>, String> {
        self.ref_reads.set(self.ref_reads.get() + 1);
        Ok(vec![ProjectWorkEvent {
            id: REF_STATE.into(),
            pubkey: self.relay.public_key().to_hex(),
            created_at: 100,
            kind: KIND_GIT_REPO_STATE,
            tags: vec![
                vec!["d".into(), repository.into()],
                vec!["refs/heads/main".into(), self.delivered.into()],
            ],
            content: String::new(),
        }])
    }
}

fn body_of(event: &Event) -> ProjectWorkEvidenceBound {
    match decode_project_work_content(&event.content)
        .expect("decodes")
        .body
    {
        ProjectWorkBody::EvidenceBound(body) => body,
        other => panic!("not an evidence binding: {other:?}"),
    }
}

#[tokio::test]
async fn a_green_delivered_verify_is_bound_under_this_hosts_key() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;

    assert_eq!(prepared.summary.skipped, None, "{:?}", prepared.summary);
    assert_eq!(prepared.events.len(), 2);
    assert_eq!(prepared.summary.signed_by, provider.public_key().to_hex());
    assert_eq!(prepared.summary.host_result, RESULT);
    assert_eq!(prepared.summary.ref_state.as_deref(), Some(REF_STATE));
    for event in &prepared.events {
        assert_eq!(event.pubkey, provider.public_key(), "never the seat's key");
        event.verify().expect("a valid signature");
        validate_project_work_envelope(&ProjectWorkEvent::from(event))
            .expect("a record the relay admits");
    }
    let action = body_of(&prepared.events[0]);
    assert_eq!(action.criterion_ids, vec!["verified-landed-revision"]);
    assert_eq!(action.artifact_commit, DELIVERED);
    assert_eq!(
        action.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::ActionResult
    );
    assert_eq!(action.evidence_refs[0].event_id, RESULT);
    assert_eq!(action.declaration_ref, fixture.records[0].id);
    let git_ref = body_of(&prepared.events[1]);
    assert_eq!(git_ref.criterion_ids, vec!["delivered-main"]);
    assert_eq!(
        git_ref.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::RefObservation
    );
    assert_eq!(git_ref.evidence_refs[0].event_id, REF_STATE);
    assert!(prepared
        .summary
        .bound
        .iter()
        .all(|binding| binding.published));
    assert_eq!(
        prepared.summary.bound[0].evidence,
        format!("action_result:{RESULT}")
    );
}

#[tokio::test]
async fn the_same_result_twice_publishes_one_set_of_records() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let first = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert_eq!(first.events.len(), 2);

    // The durable custody: a second arrival of the same result id, in this
    // process or after a restart, is refused before any read.
    let dir = tempfile::tempdir().expect("tempdir");
    let mut store = HostResultWakeStore::open(dir.path()).expect("store");
    assert!(store.claim_evidence(RESULT).expect("first claim"));
    assert!(!store.claim_evidence(RESULT).expect("second claim"));
    let reopened = HostResultWakeStore::open(dir.path()).expect("reopen");
    assert!(
        reopened.evidence_checked(RESULT),
        "custody survives a restart"
    );

    // And if custody were ever lost, the records already on the wire are
    // found and nothing is republished.
    fixture
        .records
        .extend(first.events.iter().map(ProjectWorkEvent::from));
    let second = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(second.events.is_empty(), "{:?}", second.summary);
    assert_eq!(second.summary.bound.len(), 2);
    assert!(second
        .summary
        .bound
        .iter()
        .all(|binding| !binding.published));
}

#[tokio::test]
async fn a_failing_result_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    let relay_self = fixture.relay.public_key().to_hex();
    let mut input = fixture.input(&provider, &relay_self);
    input.result.exit_code = Some(1);
    let prepared = prepare(&fixture, &input).await;
    assert!(prepared.events.is_empty());
    assert!(prepared.summary.bound.is_empty());
    assert_eq!(fixture.ref_reads.get(), 0, "refused before any ref read");
    assert!(prepared.summary.skipped.is_some());
}

#[tokio::test]
async fn a_result_at_a_commit_the_relay_has_not_seen_delivered_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.delivered = SEED;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty());
    assert!(prepared.summary.bound.is_empty());
    let skipped = prepared.summary.skipped.expect("says why");
    assert!(skipped.contains("names the delivery ref at"), "{skipped}");
}

#[tokio::test]
async fn a_plan_without_an_action_criterion_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.plan = REVIEW_ONLY;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty(), "not even the git-ref criterion");
    assert_eq!(fixture.ref_reads.get(), 0);
}

#[tokio::test]
async fn a_host_without_standing_in_the_session_binds_nothing() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let mut fixture = Fixture::new(&lead);
    fixture.may_bind = false;
    let relay_self = fixture.relay.public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &relay_self)).await;
    assert!(prepared.events.is_empty());
    assert!(prepared
        .summary
        .skipped
        .as_deref()
        .is_some_and(|why| why.contains("operator grantee")));
}

#[tokio::test]
async fn an_owner_signed_ref_state_is_not_an_observation() {
    let lead = Keys::generate();
    let provider = Keys::generate();
    let fixture = Fixture::new(&lead);
    // The fixture's ref state is signed by `fixture.relay`; name another key
    // as the relay and it becomes somebody's claim about their own branch.
    let impostor_relay = Keys::generate().public_key().to_hex();
    let prepared = prepare(&fixture, &fixture.input(&provider, &impostor_relay)).await;
    assert!(prepared.events.is_empty());
}

#[test]
fn a_wake_carrying_what_the_host_bound_is_still_a_host_result_pointer() {
    let summary = AutoEvidenceSummary {
        host_result: RESULT.into(),
        ref_state: Some(REF_STATE.into()),
        signed_by: "ab".repeat(32),
        bound: vec![AutoEvidenceBinding {
            event_id: "ef".repeat(32),
            criteria: vec!["verified-landed-revision".into()],
            evidence: format!("action_result:{RESULT}"),
            published: true,
        }],
        skipped: None,
    };
    let pointer = serde_json::json!({
        "schema": crate::host_result_wake::HOST_RESULT_WAKE_SCHEMA,
        "type": "host_result",
        "runId": "r",
        "stepId": "verify",
        "disposition": "exited",
        "exitCode": 0,
        "resultEventId": RESULT,
        "autoEvidence": summary,
    });
    assert!(is_host_result_pointer(pointer.as_object().expect("object")));
    let mut wrong = pointer;
    wrong["autoEvidence"] = serde_json::json!("bound everything");
    assert!(!is_host_result_pointer(wrong.as_object().expect("object")));
}
