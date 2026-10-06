//! `conformance/team-settlement/fixtures/settlement-vectors.json`, folded.
//!
//! The 204 rule (`docs/UNIFIED_WORK_PLAN.md` § 8 A3.3): a change to what a
//! closed record means lands with one shared fixture that **every** strict
//! reader loads, so a reader left behind fails its own test rather than a live
//! mission. This is that fixture's canonical reader: it signs each vector's
//! records, folds them, and compares the settlement projection field by field.
//!
//! The other three readers are listed in
//! `conformance/team-settlement/README.md`; a lane that changes the rule adds
//! a vector here and every one of them goes red until it ships.

use super::*;

use crate::coding_session_team_transaction::{
    CodingSessionTeamAcknowledgement, CodingSessionTeamAcknowledgementStatus,
    CodingSessionTeamAssignment, CodingSessionTeamDispositionDecision,
    CodingSessionTeamMissionCompleted, CodingSessionTeamReport,
    CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
};
use crate::kind::KIND_CODING_SESSION_TEAM_TRANSACTION;
use nostr::{Event, EventBuilder, Keys, Kind, Tag, Timestamp};
use serde_json::Value;
use std::collections::HashMap;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/// The vectors, verbatim.
pub(crate) const SETTLEMENT_VECTORS: &str =
    include_str!("../../../conformance/team-settlement/fixtures/settlement-vectors.json");

/// Deterministic keys, so a vector's expectations do not move between runs.
fn keys(byte: u8) -> Keys {
    Keys::parse(&format!("{byte:02x}").repeat(32)).expect("a fixed 32-byte secret key")
}

fn genesis() -> String {
    "ab".repeat(32)
}

fn sign(
    body: CodingSessionTeamTransactionBody,
    supersedes: Option<String>,
    author: &Keys,
    at: u64,
) -> Event {
    let payload = CodingSessionTeamTransactionPayload {
        schema: CODING_SESSION_TEAM_TRANSACTION_SCHEMA.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        transaction_type: body.transaction_type(),
        supersedes,
        delivery_command_id: None,
        body,
    };
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_TEAM_TRANSACTION as u16),
        serde_json::to_string(&payload).expect("the payload serializes"),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("tag"),
        Tag::parse(["d", SESSION]).expect("tag"),
        Tag::parse(["cstx-v", CODING_SESSION_TEAM_TRANSACTION_SCHEMA]).expect("tag"),
        Tag::parse(["cstx-genesis", payload.genesis_ref.as_str()]).expect("tag"),
        Tag::parse(["cstx-type", payload.transaction_type.as_str()]).expect("tag"),
    ])
    .custom_created_at(Timestamp::from_secs(at))
    .sign_with_keys(author)
    .expect("the record signs")
}

fn decision(word: &str) -> CodingSessionTeamDispositionDecision {
    match word {
        "approve" => CodingSessionTeamDispositionDecision::Approve,
        "approve-with-notes" => CodingSessionTeamDispositionDecision::ApproveWithNotes,
        "changes-requested" => CodingSessionTeamDispositionDecision::ChangesRequested,
        "reject" => CodingSessionTeamDispositionDecision::Reject,
        "blocked" => CodingSessionTeamDispositionDecision::Blocked,
        other => panic!("a vector names an unknown disposition decision: {other}"),
    }
}

/// One vector's signed records, in fixture order, keyed by symbolic id.
pub(crate) struct SignedVector {
    pub(crate) events: Vec<Event>,
    pub(crate) ids: HashMap<String, String>,
    pub(crate) context: CodingSessionTeamFoldContext,
}

/// Sign one vector's records. Shared by this test and the CLI's reader.
pub(crate) fn sign_vector(vector: &Value) -> SignedVector {
    let founder = keys(0x11);
    let assignee = keys(0x22);
    let stranger = keys(0x33);
    let context = CodingSessionTeamFoldContext {
        channel_ref: CHANNEL.into(),
        session_ref: SESSION.into(),
        genesis_ref: genesis(),
        founder_pubkey: founder.public_key().to_hex(),
        active_seats: vec![CodingSessionTeamActiveSeat {
            actor_pubkey: assignee.public_key().to_hex(),
            role: "builder".into(),
        }],
        active_grants: Vec::new(),
        verifier_required: false,
    };
    let mut ids: HashMap<String, String> = HashMap::new();
    let mut events = Vec::new();
    for (index, record) in vector["records"]
        .as_array()
        .expect("records is a list")
        .iter()
        .enumerate()
    {
        let id = record["id"].as_str().expect("an id").to_owned();
        let author = match record["author"].as_str().expect("an author") {
            "founder" => &founder,
            "assignee" => &assignee,
            "stranger" => &stranger,
            other => panic!("a vector names an unknown author: {other}"),
        };
        let reference = |key: &str| -> String {
            let symbol = record[key].as_str().unwrap_or_else(|| {
                panic!("record {id} is missing {key}");
            });
            ids.get(symbol)
                .unwrap_or_else(|| panic!("record {id} names {symbol}, which is not signed yet"))
                .clone()
        };
        let body = match record["type"].as_str().expect("a type") {
            "assignment" => {
                CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
                    assignee_actor: assignee.public_key().to_hex(),
                    assignee_role: "builder".into(),
                    objective: "Build the slice".into(),
                    brief: "Implement the bounded assigned slice.".into(),
                    branch: None,
                    base_sha: None,
                    file_ownership: vec!["crates/beekeeper-core/src".into()],
                    acceptance_steps: vec!["cargo test -p beekeeper-core".into()],
                })
            }
            "report" => CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
                assignment_ref: reference("assignmentRef"),
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
            }),
            "disposition" => {
                CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
                    assignment_ref: reference("assignmentRef"),
                    report_ref: reference("reportRef"),
                    refutation_ref: None,
                    decision: decision(record["decision"].as_str().expect("a decision")),
                    summary: "Governed".into(),
                    findings: record["findings"]
                        .as_array()
                        .expect("findings is a list")
                        .iter()
                        .map(|item| item.as_str().expect("a finding").to_owned())
                        .collect(),
                    required_action: record["requiredAction"]
                        .as_str()
                        .map(std::borrow::ToOwned::to_owned),
                })
            }
            "acknowledgement" => CodingSessionTeamTransactionBody::Acknowledgement(
                CodingSessionTeamAcknowledgement {
                    acknowledged_event_ref: reference("acknowledgedEventRef"),
                    status: CodingSessionTeamAcknowledgementStatus::Received,
                    note: None,
                },
            ),
            "completion" => CodingSessionTeamTransactionBody::MissionCompleted(
                CodingSessionTeamMissionCompleted {
                    assignment_refs: record["assignmentRefs"]
                        .as_array()
                        .expect("assignmentRefs is a list")
                        .iter()
                        .map(|item| {
                            let symbol = item.as_str().expect("a symbolic id");
                            ids.get(symbol)
                                .unwrap_or_else(|| panic!("{symbol} is not signed yet"))
                                .clone()
                        })
                        .collect(),
                    landed_shas: Vec::new(),
                    summary: "Mission finished".into(),
                    follow_ups: Vec::new(),
                },
            ),
            other => panic!("a vector names an unknown record type: {other}"),
        };
        let supersedes = record["supersedes"]
            .as_str()
            .map(|symbol| ids[symbol].clone());
        let event = sign(body, supersedes, author, 1000 + index as u64);
        ids.insert(id, event.id.to_hex());
        events.push(event);
    }
    SignedVector {
        events,
        ids,
        context,
    }
}

/// Every vector folds to the settlement projection it declares.
#[test]
fn every_settlement_vector_folds_to_its_declared_projection() {
    let fixture: Value = serde_json::from_str(SETTLEMENT_VECTORS).expect("the fixture is JSON");
    let vectors = fixture["vectors"].as_array().expect("a list of vectors");
    assert!(!vectors.is_empty());
    for vector in vectors {
        let name = vector["name"].as_str().expect("a name");
        let signed = sign_vector(vector);
        let fold = fold_coding_session_team_transactions(&signed.events, &signed.context)
            .unwrap_or_else(|error| panic!("{name}: the vector's set must fold: {error}"));
        let expected = &vector["expected"];
        let rows = expected["assignments"].as_array().expect("assignments");
        assert_eq!(
            fold.assignments.len(),
            rows.len(),
            "{name}: assignment count"
        );
        for row in rows {
            let assignment_id = &signed.ids[row["assignment"].as_str().expect("an assignment")];
            let state = fold
                .assignments
                .iter()
                .find(|state| &state.assignment_event_id == assignment_id)
                .unwrap_or_else(|| panic!("{name}: no settlement row for that assignment"));
            assert_eq!(
                state.settled,
                row["settled"].as_bool().expect("settled"),
                "{name}: settled"
            );
            assert_eq!(
                state.settled_by.map(CodingSessionTeamSettledBy::as_str),
                row["settledBy"].as_str(),
                "{name}: settledBy"
            );
            assert_eq!(
                state.governed_report_event_id.as_deref(),
                row["governedReport"]
                    .as_str()
                    .map(|symbol| signed.ids[symbol].as_str()),
                "{name}: governedReport"
            );
            assert_eq!(
                state.disposition_event_id.as_deref(),
                row["disposition"]
                    .as_str()
                    .map(|symbol| signed.ids[symbol].as_str()),
                "{name}: disposition"
            );
            assert_eq!(
                state.acknowledgement_event_id.as_deref(),
                row["acknowledgement"]
                    .as_str()
                    .map(|symbol| signed.ids[symbol].as_str()),
                "{name}: acknowledgement"
            );
            match row["awaiting"].as_object() {
                None => assert!(state.awaiting.is_none(), "{name}: awaiting must be null"),
                Some(awaiting) => {
                    let actual = state
                        .awaiting
                        .as_ref()
                        .unwrap_or_else(|| panic!("{name}: awaiting must be present"));
                    assert_eq!(
                        actual.link.as_str(),
                        awaiting["link"].as_str().expect("a link"),
                        "{name}: awaiting.link"
                    );
                    assert_eq!(
                        actual.owed_by_role.as_str(),
                        awaiting["owedByRole"].as_str().expect("a role"),
                        "{name}: awaiting.owedByRole"
                    );
                    let owed = awaiting["owedByActor"].as_str().map(|symbol| {
                        assert_eq!(symbol, "assignee", "{name}: only the assignee is nameable");
                        keys(0x22).public_key().to_hex()
                    });
                    assert_eq!(actual.owed_by_actor, owed, "{name}: awaiting.owedByActor");
                }
            }
        }
        assert_eq!(
            fold.canonical_terminal
                .as_ref()
                .map(|terminal| terminal.event_id.as_str()),
            expected["terminal"]
                .as_str()
                .map(|symbol| signed.ids[symbol].as_str()),
            "{name}: canonical terminal"
        );
        match expected["pendingCompletion"].as_object() {
            None => assert!(
                fold.pending_completion.is_none(),
                "{name}: pendingCompletion must be null"
            ),
            Some(pending) => {
                let actual = fold
                    .pending_completion
                    .as_ref()
                    .unwrap_or_else(|| panic!("{name}: a pending completion is expected"));
                assert_eq!(
                    actual.event_id,
                    signed.ids[pending["eventId"].as_str().expect("an event id")],
                    "{name}: pendingCompletion.eventId"
                );
                assert_eq!(
                    crate::team_vocabulary::fold_exclusion_wire_code(actual.code),
                    pending["code"].as_str().expect("a code"),
                    "{name}: pendingCompletion.code"
                );
                let unsettled: Vec<String> = pending["unsettled"]
                    .as_array()
                    .expect("a list")
                    .iter()
                    .map(|item| signed.ids[item.as_str().expect("a symbolic id")].clone())
                    .collect();
                assert_eq!(
                    actual.unsettled_assignment_event_ids, unsettled,
                    "{name}: pendingCompletion.unsettled"
                );
            }
        }
        let expected_excluded: Vec<String> = expected["excluded"]
            .as_array()
            .map(|list| {
                list.iter()
                    .map(|item| signed.ids[item.as_str().expect("a symbolic id")].clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut actual_excluded: Vec<String> = fold
            .excluded
            .iter()
            .map(|item| item.event_id.clone())
            .collect();
        actual_excluded.sort();
        let mut expected_excluded = expected_excluded;
        expected_excluded.sort();
        assert_eq!(actual_excluded, expected_excluded, "{name}: excluded set");
    }
}
