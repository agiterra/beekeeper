//! The refusal set and the idempotence of the host's automatic evidence
//! binding (ledger 257(d)), over the conformance set's own kettle plan.

use super::*;
use crate::project_plan::parse_plan;
use crate::project_work::{
    ProjectWorkPayload, ProjectWorkPlanRef, ProjectWorkRecordType, PROJECT_WORK_SCHEMA,
};

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

const RELAY: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
const OWNER: &str = "bb00000000000000000000000000000000000000000000000000000000000000";
const DECLARATION: &str = "d100000000000000000000000000000000000000000000000000000000000000";
const RESULT: &str = "4623000000000000000000000000000000000000000000000000000000000000";
const DELIVERED: &str = "d7b4a649c3399e5c4da95753220bb6712bcfae5e";
const OTHER: &str = "7070283000000000000000000000000000000000";

fn plan(text: &str) -> Plan {
    parse_plan(text.as_bytes()).expect("fixture plan parses")
}

fn green() -> HostResultFacts {
    HostResultFacts {
        result_event_id: RESULT.into(),
        action_name: "verify".into(),
        step_id: "verify".into(),
        exited: true,
        exit_code: Some(0),
        dirty: Some(false),
        head_sha: Some(DELIVERED.into()),
    }
}

fn observed_at(commit: &str) -> RefObservation {
    RefObservation {
        event_id: "3061800000000000000000000000000000000000000000000000000000000000".into(),
        commit: commit.into(),
    }
}

fn work_record(
    id: &str,
    record_type: ProjectWorkRecordType,
    body: ProjectWorkBody,
) -> ProjectWorkEvent {
    let payload = ProjectWorkPayload {
        schema: PROJECT_WORK_SCHEMA.into(),
        session_ref: "8b1b1b1b-0000-4000-8000-000000000001".into(),
        genesis_ref: "ee".repeat(32),
        project_ref: format!("30621:{OWNER}:kettle"),
        record_type,
        body,
    };
    ProjectWorkEvent {
        id: id.into(),
        pubkey: OWNER.into(),
        created_at: 1,
        kind: KIND_PROJECT_WORK_RECORD,
        tags: Vec::new(),
        content: payload.canonical_content().expect("canonical content"),
    }
}

fn declared(id: &str, supersedes: Vec<String>) -> ProjectWorkEvent {
    work_record(
        id,
        ProjectWorkRecordType::Declared,
        ProjectWorkBody::Declared(ProjectWorkDeclared {
            work_id: "0b6f6a3c-7c1a-5a8e-9c2d-0a1b2c3d4e5f".into(),
            goal_ref: "cc".repeat(32),
            decision_ref: None,
            responsible_actor: OWNER.into(),
            plan_ref: ProjectWorkPlanRef {
                repository: format!("30617:{OWNER}:kettle-agents"),
                commit: "21ece7f946797575e79902ae8b1002913be0152c".into(),
                path: "plans/kettle.md".into(),
            },
            supersedes,
        }),
    )
}

#[test]
fn a_green_delivered_result_binds_the_action_and_the_git_ref_criteria() {
    let bindings = auto_bindings(
        &plan(KETTLE),
        DECLARATION,
        &[],
        &green(),
        &observed_at(DELIVERED),
    )
    .expect("binds");
    assert_eq!(bindings.len(), 2);
    let action = &bindings[0].body;
    assert_eq!(
        action.criterion_ids,
        vec!["verified-landed-revision".to_owned()]
    );
    assert_eq!(action.artifact_commit, DELIVERED);
    assert_eq!(
        action.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::ActionResult
    );
    assert_eq!(action.evidence_refs[0].event_id, RESULT);
    let git_ref = &bindings[1].body;
    assert_eq!(git_ref.criterion_ids, vec!["delivered-main".to_owned()]);
    assert_eq!(
        git_ref.evidence_refs[0].kind,
        ProjectWorkEvidenceKind::RefObservation
    );
    assert!(bindings.iter().all(|binding| binding.existing.is_none()));
}

#[test]
fn the_same_result_twice_finds_its_own_records_and_binds_nothing_new() {
    let first = auto_bindings(
        &plan(KETTLE),
        DECLARATION,
        &[],
        &green(),
        &observed_at(DELIVERED),
    )
    .expect("binds");
    let on_wire: Vec<ProjectWorkEvent> = first
        .iter()
        .enumerate()
        .map(|(index, binding)| {
            work_record(
                &format!("{index:0>64}"),
                ProjectWorkRecordType::EvidenceBound,
                ProjectWorkBody::EvidenceBound(binding.body.clone()),
            )
        })
        .collect();
    let second = auto_bindings(
        &plan(KETTLE),
        DECLARATION,
        &on_wire,
        &green(),
        &observed_at(DELIVERED),
    )
    .expect("still decides");
    assert_eq!(second.len(), 2);
    assert!(
        second.iter().all(|binding| binding.existing.is_some()),
        "a replay must find both records and republish neither: {second:?}"
    );
}

#[test]
fn a_failing_result_binds_nothing() {
    let mut result = green();
    result.exit_code = Some(1);
    let skip = auto_bindings(
        &plan(KETTLE),
        DECLARATION,
        &[],
        &result,
        &observed_at(DELIVERED),
    )
    .expect_err("exit 1 binds nothing");
    assert_eq!(skip, AutoBindSkip::NotGreen("exit code 1".into()));
}

#[test]
fn a_dirty_or_unrecorded_checkout_binds_nothing() {
    for dirty in [Some(true), None] {
        let mut result = green();
        result.dirty = dirty;
        assert!(matches!(
            auto_bindings(
                &plan(KETTLE),
                DECLARATION,
                &[],
                &result,
                &observed_at(DELIVERED)
            ),
            Err(AutoBindSkip::NotGreen(_))
        ));
    }
}

#[test]
fn a_result_at_an_undelivered_commit_binds_nothing() {
    let mut result = green();
    result.head_sha = Some(OTHER.into());
    let skip = auto_bindings(
        &plan(KETTLE),
        DECLARATION,
        &[],
        &result,
        &observed_at(DELIVERED),
    )
    .expect_err("headSha is not the delivered commit");
    assert!(
        matches!(skip, AutoBindSkip::NotDelivered { .. }),
        "{skip:?}"
    );
}

#[test]
fn a_plan_without_an_action_criterion_binds_nothing_not_even_its_git_ref() {
    let skip = auto_bindings(
        &plan(REVIEW_ONLY),
        DECLARATION,
        &[],
        &green(),
        &observed_at(DELIVERED),
    )
    .expect_err("no action criterion");
    assert!(
        matches!(skip, AutoBindSkip::NoActionCriterion { .. }),
        "{skip:?}"
    );
}

#[test]
fn another_action_or_step_binds_nothing() {
    let mut result = green();
    result.action_name = "deploy".into();
    assert!(matches!(
        auto_bindings(
            &plan(KETTLE),
            DECLARATION,
            &[],
            &result,
            &observed_at(DELIVERED)
        ),
        Err(AutoBindSkip::NoActionCriterion { .. })
    ));
}

#[test]
fn the_current_declaration_is_the_one_nothing_supersedes() {
    let first = declared(&"d0".repeat(32), Vec::new());
    let amended = declared(DECLARATION, vec!["d0".repeat(32)]);
    let (id, _) = current_declaration(&[first.clone(), amended]).expect("one head");
    assert_eq!(id, DECLARATION);
    let rival = declared(&"d2".repeat(32), Vec::new());
    assert!(current_declaration(&[first.clone(), rival]).is_err());
    assert!(current_declaration(&[]).is_err());
}

fn ref_state(id: &str, author: &str, created_at: u64, commit: &str) -> ProjectWorkEvent {
    ProjectWorkEvent {
        id: id.into(),
        pubkey: author.into(),
        created_at,
        kind: KIND_GIT_REPO_STATE,
        tags: vec![
            vec!["d".into(), "pivot-test".into()],
            vec!["refs/heads/main".into(), commit.into()],
        ],
        content: String::new(),
    }
}

#[test]
fn only_the_relays_newest_ref_state_is_an_observation() {
    let states = vec![
        ref_state(&"01".repeat(32), RELAY, 10, OTHER),
        ref_state(&"02".repeat(32), RELAY, 20, DELIVERED),
        // Newer, but the owner's own claim about its branch.
        ref_state(&"03".repeat(32), OWNER, 30, OTHER),
    ];
    let observed =
        newest_ref_observation(&states, RELAY, "pivot-test", "refs/heads/main").expect("observed");
    assert_eq!(observed.event_id, "02".repeat(32));
    assert_eq!(observed.commit, DELIVERED);
    assert_eq!(
        newest_ref_observation(&states, RELAY, "pivot-test", "refs/heads/release"),
        Err(RefObservationMissing::NoDeliveryRef {
            state_id: "02".repeat(32)
        })
    );
    assert_eq!(
        newest_ref_observation(&states, RELAY, "other-repo", "refs/heads/main"),
        Err(RefObservationMissing::NoState)
    );
}

/// Control run 6 (kettle-control-6, 2026-09-24): the host's read answered
/// `NoState` while three relay-signed kind:30618 for the repository existed,
/// the newest nine seconds old. The project is private, and the relay's
/// NIP-MP read gate withholds a private project's repository events — its
/// relay-signed ref state included, since the gate exempts only the event's
/// own author — from every key off the project's roster. The host's key held
/// the session's operator grant (kind:44228 seq 1) but was not on the roster
/// (kind:9010), while the lead's key was. The same filter therefore served
/// the lead the record and the host nothing, and nothing is `NoState`.
#[test]
fn a_private_projects_ref_state_the_relay_withholds_reads_as_no_state() {
    use crate::kind::repo_event_hidden_from;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use std::collections::HashSet;

    let relay = Keys::generate();
    let tags = [["d", "kettle-control-6"], ["refs/heads/main", DELIVERED]]
        .into_iter()
        .map(|parts| Tag::parse(parts).expect("tag"))
        .collect::<Vec<_>>();
    let state = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_STATE as u16), "")
        .tags(tags)
        .sign_with_keys(&relay)
        .expect("sign");
    let relay_self = relay.public_key().to_hex();
    let host = Keys::generate().public_key().to_hex();
    let lead = Keys::generate().public_key().to_hex();
    // `hidden_repos_for_reader`: the host is off the roster, so the private
    // project's repository names are in its hidden set; the lead's is empty.
    let host_hidden: HashSet<String> = ["kettle-control-6".to_owned()].into();
    let none: HashSet<String> = HashSet::new();
    let served = |reader: &str, hidden: &HashSet<String>| -> Vec<ProjectWorkEvent> {
        if repo_event_hidden_from(&state, reader, hidden, &none) {
            Vec::new()
        } else {
            vec![ProjectWorkEvent::from(&state)]
        }
    };

    assert_eq!(
        newest_ref_observation(
            &served(&host, &host_hidden),
            &relay_self,
            "kettle-control-6",
            "refs/heads/main"
        ),
        Err(RefObservationMissing::NoState)
    );
    let observed = newest_ref_observation(
        &served(&lead, &none),
        &relay_self,
        "kettle-control-6",
        "refs/heads/main",
    )
    .expect("the roster member reads the observation");
    assert_eq!(observed.commit, DELIVERED);
}
