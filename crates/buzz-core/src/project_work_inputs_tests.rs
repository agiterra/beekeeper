//! Tests for the shared input assembler, bound to the frozen sequences.
//!
//! The oracle is not written here. Every sequence's `inputs.json` is the
//! contract's own statement of what the fold must be given; these tests build
//! a **raw event set** that would have produced it — 44244 reports and
//! verdicts, 46023/46014/46013 host-run triples, 44227 goals, 30618 ref state
//! — hand it to [`assemble_fold_inputs`], and assert the assembler rebuilds
//! that same input and that `fold_work` over it still equals
//! `expected-fold.json`.
//!
//! That is the property W5 and W3b depend on: whatever a caller fetched, one
//! assembler turns it into the one input the contract fixes.

use super::*;

use serde_json::{json, Value};

use crate::project_work_fold::fold_work;

macro_rules! sequence {
    ($name:literal) => {
        Sequence {
            name: $name,
            events: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/events.json"
            )),
            inputs: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/inputs.json"
            )),
            expected: include_str!(concat!(
                "../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/expected-fold.json"
            )),
        }
    };
}

struct Sequence {
    name: &'static str,
    events: &'static str,
    inputs: &'static str,
    expected: &'static str,
}

const KETTLE_PLAN: &str =
    include_str!("../../../conformance/project-work/fixtures/plans/valid/kettle.md");

const SEQUENCES: [Sequence; 13] = [
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
];

/// Split a fold blob key `<coordinate>@<commit>:<path>` back into its parts.
///
/// The coordinate itself carries colons, so the commit is taken after the
/// last `@` and the path after the first `:` that follows it.
fn split_blob_key(key: &str) -> (String, String, String) {
    let (repository, rest) = key.rsplit_once('@').expect("a blob key names a commit");
    let (commit, path) = rest.split_once(':').expect("a blob key names a path");
    (repository.to_owned(), commit.to_owned(), path.to_owned())
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
            vec!["h".into(), "22222222-3333-4444-8555-666666666666".into()],
            vec!["d".into(), "11111111-2222-4333-8444-555555555555".into()],
            vec!["csgl-v".into(), CODING_SESSION_GOAL_TAG_VERSION.into()],
        ],
        content: "ship the kettle".to_owned(),
    }
}

/// Everything the assembler is handed for one sequence, derived from the
/// facts the fixture states — never from the fold's own types.
fn raw_from_fixture(sequence: &Sequence) -> RawWorkInputs {
    let fixture: Value = serde_json::from_str(sequence.inputs).expect("sequence inputs");
    let work_events: Vec<ProjectWorkEvent> =
        serde_json::from_str(sequence.events).expect("sequence events");

    // Goals: the current one is made the newest, because "current" is the
    // newest by (created_at, id) and the fixture states it directly.
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

    // Evidence: one raw event set per stated fact.
    let mut team_events = Vec::new();
    let mut host_results = Vec::new();
    let mut host_echoes = Vec::new();
    let mut host_requests = Vec::new();
    let empty = serde_json::Map::new();
    for fact in fixture["evidence"].as_object().unwrap_or(&empty).values() {
        match fact["kind"].as_str().expect("fact kind") {
            "report" => team_events.push(event(
                fact["eventId"].as_str().expect("event id"),
                fact["signer"].as_str().expect("signer"),
                1_789_000_200,
                KIND_CODING_SESSION_TEAM_TRANSACTION,
                json!({
                    "schema": "buzz-coding-session-team-transaction/v1",
                    "type": "report",
                    "body": {
                        "assignmentRef": fact["assignmentRef"],
                        "summary": "done",
                        "headSha": fact["headSha"],
                    },
                }),
            )),
            "verdict" => team_events.push(event(
                fact["eventId"].as_str().expect("event id"),
                fact["signer"].as_str().expect("signer"),
                1_789_000_300,
                KIND_CODING_SESSION_TEAM_TRANSACTION,
                json!({
                    "schema": "buzz-coding-session-team-transaction/v1",
                    "type": "verdict",
                    "body": {
                        "subtype": fact["subtype"],
                        "decision": fact["decision"],
                        "assignmentRef": fact["assignmentRef"],
                        "reportRef": fact["reportRef"],
                        "summary": "ruled",
                    },
                }),
            )),
            "action_result" => {
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
                    &format!("{:0>64}", "9e9"),
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
            other => panic!("{}: unknown evidence fact kind {other}", sequence.name),
        }
    }

    // Plan blobs and action definitions, re-keyed as a caller holds them:
    // by the repository and commit it read them at.
    let mut plan_blobs = BTreeMap::new();
    let mut coordinate = None;
    for key in fixture["planBlobs"].as_object().unwrap_or(&empty).keys() {
        let (repository, commit, path) = split_blob_key(key);
        coordinate = Some((repository.clone(), commit.clone()));
        plan_blobs.insert((repository, commit, path), KETTLE_PLAN.to_owned());
    }
    let mut action_definitions = BTreeMap::new();
    for (name, definition) in fixture["actionDefinitions"].as_object().unwrap_or(&empty) {
        let (repository, commit) = coordinate
            .clone()
            .expect("a sequence with an action also names a plan blob");
        action_definitions.insert(
            (repository, commit, name.clone()),
            serde_json::from_value(definition.clone()).expect("action definition"),
        );
    }

    RawWorkInputs {
        work_events,
        team_events,
        host_results,
        host_echoes,
        host_requests,
        ref_states: serde_json::from_value(fixture["refStates"].clone()).expect("ref states"),
        goal_events,
        authority: RawAuthorityContext {
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
        session_ref: None,
        project_ref: None,
    }
}

/// Every sequence's `inputs.json`, rebuilt from a raw event set.
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
        assert_eq!(
            serde_json::to_value(&assembled.evidence).expect("evidence"),
            if fixture["evidence"].is_null() {
                json!({})
            } else {
                fixture["evidence"].clone()
            },
            "{}: evidence",
            sequence.name
        );
        assert_eq!(
            serde_json::to_value(&assembled.ref_states).expect("ref states"),
            fixture["refStates"],
            "{}: refStates",
            sequence.name
        );
        let blob_keys: Vec<&String> = assembled.plan_blobs.keys().collect();
        let expected_keys: Vec<&String> = fixture["planBlobs"]
            .as_object()
            .map(|blobs| blobs.keys().collect())
            .unwrap_or_default();
        assert_eq!(blob_keys, expected_keys, "{}: planBlobs", sequence.name);
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
    }
}

/// The whole point: assemble, then fold, and the contract's expected output
/// still comes out.
#[test]
fn folding_an_assembled_input_matches_every_expected_fold() {
    for sequence in &SEQUENCES {
        let assembled =
            assemble_fold_inputs(raw_from_fixture(sequence)).expect("the fixture assembles");
        let projection = serde_json::to_value(fold_work(&assembled)).expect("projection");
        let expected: Value = serde_json::from_str(sequence.expected).expect("expected fold");
        assert_eq!(projection, expected, "{}", sequence.name);
    }
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

/// A refutation verdict rules on nothing a criterion can be covered by, and a
/// report that names no revision cannot say a criterion was met at one — but
/// both are read, because the fold names the reason a claim did not hold.
#[test]
fn a_report_with_no_head_sha_yields_no_fact() {
    let assembled = assemble_fold_inputs(RawWorkInputs {
        team_events: vec![event(
            &format!("{:0>64}", "1ea"),
            &"b0".repeat(32),
            1_789_000_200,
            KIND_CODING_SESSION_TEAM_TRANSACTION,
            json!({"type": "report", "body": {"assignmentRef": "a5".repeat(32),
                   "summary": "done", "headSha": null}}),
        )],
        authority: RawAuthorityContext {
            founder_pubkey: Some("1".repeat(64)),
            ..RawAuthorityContext::default()
        },
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
