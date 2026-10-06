//! Binds `bee sessions` to every vector in
//! `conformance/transcript-prose-join/fixtures/vectors.json` — read from the
//! repository's conformance directory, never a copy — and pins the Markdown
//! the transcript renders for each.

use serde_json::{json, Value};

use beekeeper_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use beekeeper_core::coding_session_payload::{
    Capabilities, SessionMetadata, SessionStatus, METADATA_SCHEMA,
};
use beekeeper_sdk::kind::{KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_TRANSCRIPT};

use super::prose_join::{join_prose, ArrivalEvidence, JoinedProse, STILL_WRITING_LINE};
use super::{decode_metadata, decode_transcripts, render_markdown_with_evidence, TranscriptRecord};

const VECTORS: &str =
    include_str!("../../../../../conformance/transcript-prose-join/fixtures/vectors.json");

fn vectors() -> Vec<Value> {
    let file: Value = serde_json::from_str(VECTORS).expect("vectors.json parses");
    assert_eq!(file["schema"], "buzz.conformance/transcript-prose-join@1");
    file["vectors"].as_array().expect("vectors array").clone()
}

fn vector(name: &str) -> Value {
    vectors()
        .into_iter()
        .find(|vector| vector["name"] == name)
        .unwrap_or_else(|| panic!("no vector named {name}"))
}

fn target_of(value: &Value) -> CodingSessionTarget {
    serde_json::from_value(value.clone()).expect("target decodes")
}

/// The vector's envelopes as the relay would hand them to `bee`.
fn records(vector: &Value) -> Vec<TranscriptRecord> {
    let events: Vec<Value> = vector["input"]
        .as_array()
        .expect("input")
        .iter()
        .map(|envelope| {
            let content = &envelope["content"];
            json!({
                "id": envelope["eventId"],
                "pubkey": envelope["signer"],
                "kind": KIND_CODING_SESSION_TRANSCRIPT,
                "created_at": content["timestamp"].as_i64().expect("timestamp") / 1000,
                "sig": "0".repeat(128),
                "tags": [
                    ["cs-target", coding_session_target_key(&target_of(&content["session"]))],
                    ["cst-seq", content["eventSeq"].as_u64().expect("seq").to_string()],
                ],
                "content": content.to_string(),
            })
        })
        .collect();
    let (records, stats) = decode_transcripts(&events);
    assert_eq!(stats.malformed, 0, "{}", vector["name"]);
    records
}

/// The reader's status and lease view, exactly as the vector states it.
fn evidence(vector: &Value) -> ArrivalEvidence {
    let mut evidence = ArrivalEvidence::none();
    for status in vector["sessionStatus"].as_array().expect("status") {
        evidence = evidence.with_status(
            status["signer"].as_str().expect("signer"),
            &target_of(&status["target"]),
            status["status"].as_str().expect("status"),
        );
    }
    for lease in vector["sessionLease"].as_array().expect("lease") {
        // `released` and `lapsed` are no live lease the reader holds.
        if lease["lease"] == "live" {
            evidence = evidence.with_live_lease(
                lease["signer"].as_str().expect("signer"),
                &target_of(&lease["target"]),
            );
        }
    }
    evidence
}

fn as_expected(message: &JoinedProse) -> Value {
    json!({
        "kind": message.kind,
        "signer": message.signer,
        "target": serde_json::to_value(&message.target).expect("target"),
        "turnId": message.turn_id,
        "parentToolId": message.parent_tool_id,
        "firstEventId": message.first_event_id,
        "lastEventId": message.last_event_id,
        "text": message.text,
        "arriving": message.arriving,
    })
}

fn render(vector: &Value) -> String {
    render_markdown_with_evidence(None, &records(vector), &evidence(vector))
}

#[test]
fn every_vector_joins_to_its_expected_messages() {
    let all = vectors();
    assert_eq!(all.len(), 15, "a vector was added or removed: bind it here");
    for vector in &all {
        let joined: Vec<Value> = join_prose(&records(vector), &evidence(vector))
            .iter()
            .map(as_expected)
            .collect();
        assert_eq!(
            Value::Array(joined),
            vector["expectedMessages"],
            "vector {}",
            vector["name"]
        );
    }
}

/// Golden text for every vector: each expected message prints exactly once,
/// under one heading, its text whole, with the still-writing line iff it is
/// arriving; and no piece prints a heading of its own.
#[test]
fn every_vector_renders_one_heading_per_joined_message() {
    for vector in vectors() {
        let markdown = render(&vector);
        let expected = vector["expectedMessages"].as_array().expect("messages");
        let headings =
            markdown.matches("**Assistant").count() + markdown.matches("**Reasoning").count();
        assert_eq!(
            headings,
            expected.len(),
            "vector {}\n{markdown}",
            vector["name"]
        );
        for message in expected {
            let label = if message["kind"] == "reasoning" {
                "Reasoning"
            } else {
                "Assistant"
            };
            let heading = match message["parentToolId"].as_str() {
                Some(parent) => format!("**{label} (subagent `{parent}`)**"),
                None => format!("**{label}**"),
            };
            let mut block = format!(
                "{heading}\n\n{}\n\n",
                message["text"].as_str().expect("text")
            );
            if message["arriving"] == true {
                block.push_str(STILL_WRITING_LINE);
            }
            assert_eq!(
                markdown.matches(&block).count(),
                1,
                "vector {}: expected block once\n{block}\n--- got ---\n{markdown}",
                vector["name"]
            );
        }
        let arriving = expected
            .iter()
            .filter(|message| message["arriving"] == true)
            .count();
        assert_eq!(
            markdown.matches(STILL_WRITING_LINE).count(),
            arriving,
            "vector {}",
            vector["name"]
        );
    }
}

#[test]
fn three_paragraphs_print_under_one_assistant_heading() {
    let vector = vector("three-paragraphs-one-turn");
    let text = vector["expectedMessages"][0]["text"]
        .as_str()
        .expect("text");
    let markdown = render(&vector);
    assert_eq!(
        markdown,
        format!(
            "# vector-session\n\n\
             `claude-agent-acp` · session `vector-session` · generation 1\n\n\
             ## Turn 1 (`turn-1`)\n\n\
             **User** _(cmd `cmd`)_\n\nExplain the change.\n\n\
             **Assistant**\n\n{text}\n\n\
             \n_result: success · 1234 ms_\n\n"
        )
    );
    assert_eq!(markdown.matches("**Assistant**").count(), 1);
}

#[test]
fn prose_tool_prose_prints_two_assistant_headings() {
    let markdown = render(&vector("prose-tool-prose"));
    assert_eq!(markdown.matches("**Assistant**").count(), 2, "{markdown}");
    assert_eq!(markdown.matches("**Reasoning**").count(), 1, "{markdown}");
    let first = markdown.find("**Assistant**").expect("first");
    let tool = markdown.find("- tool `").expect("tool line");
    let second = markdown.rfind("**Assistant**").expect("second");
    assert!(first < tool && tool < second, "{markdown}");
}

#[test]
fn subagent_prose_is_headed_as_the_subagent() {
    let markdown = render(&vector("own-prose-around-subagent-prose"));
    assert_eq!(
        markdown
            .matches("**Assistant (subagent `task-1`)**")
            .count(),
        2,
        "{markdown}"
    );
    assert_eq!(markdown.matches("**Assistant**").count(), 2, "{markdown}");
    assert!(
        markdown.contains("**Assistant**\n\nI will ask a subagent.\n\nIt knows the codebase.\n\n"),
        "{markdown}"
    );
}

#[test]
fn an_open_turn_with_a_live_lease_says_it_is_still_writing() {
    let vector = vector("open-turn-arriving");
    let markdown = render(&vector);
    let text = vector["expectedMessages"][1]["text"]
        .as_str()
        .expect("text");
    assert!(
        markdown.ends_with(&format!("**Assistant**\n\n{text}\n\n{STILL_WRITING_LINE}")),
        "{markdown}"
    );
    assert_eq!(
        markdown.matches(STILL_WRITING_LINE).count(),
        1,
        "{markdown}"
    );
}

/// Turn state alone is never enough: without a lease the reader holds, the
/// same open turn prints no still-writing line (contract rule 7).
#[test]
fn an_open_turn_without_lease_evidence_claims_nothing() {
    let vector = vector("open-turn-arriving");
    let markdown = render_markdown_with_evidence(None, &records(&vector), &ArrivalEvidence::none());
    assert!(!markdown.contains(STILL_WRITING_LINE), "{markdown}");
    assert!(!super::render_markdown(None, &records(&vector)).contains(STILL_WRITING_LINE));
}

#[test]
fn a_duplicate_delivery_prints_once_and_counts_once() {
    let vector = vector("duplicate-delivery-is-one-piece");
    let input = vector["input"].as_array().expect("input");
    let distinct: std::collections::HashSet<&str> = input
        .iter()
        .filter_map(|envelope| envelope["eventId"].as_str())
        .collect();
    assert!(
        distinct.len() < input.len(),
        "the vector must repeat an event"
    );
    let markdown = render(&vector);
    for message in vector["expectedMessages"].as_array().expect("messages") {
        let text = message["text"].as_str().expect("text");
        assert_eq!(markdown.matches(text).count(), 1, "{markdown}");
    }
}

// ── ArrivalEvidence::from_wire ───────────────────────────────────────────────

const SIGNER: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";
const NOW: i64 = 1_785_513_000;

fn open_turn() -> (Value, CodingSessionTarget) {
    let vector = vector("open-turn-arriving");
    let target = target_of(&vector["sessionLease"][0]["target"]);
    (vector, target)
}

fn lease_event(target: &CodingSessionTarget, state: &str, sequence: u64, created_at: i64) -> Value {
    json!({
        "id": format!("{sequence:064}"),
        "pubkey": SIGNER,
        "kind": 24223,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [],
        "content": json!({
            "schema": "buzz-coding-session-lease/v1",
            "target": target,
            "state": state,
            "leaseSequence": sequence,
        }).to_string(),
    })
}

fn metadata_event(target: &CodingSessionTarget, status: SessionStatus, created_at: i64) -> Value {
    let payload = SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: None,
        role: None,
        provider: None,
        runtime: None,
        model: None,
        status,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: None,
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
        compose_ref: None,
    };
    json!({
        "id": format!("{created_at:064}"),
        "pubkey": SIGNER,
        "kind": KIND_CODING_SESSION_METADATA,
        "created_at": created_at,
        "sig": "0".repeat(128),
        "tags": [["cs-target", coding_session_target_key(target)]],
        "content": serde_json::to_string(&payload).expect("serialize"),
    })
}

fn arriving_count(evidence: &ArrivalEvidence, records: &[TranscriptRecord]) -> usize {
    join_prose(records, evidence)
        .iter()
        .filter(|message| message.arriving)
        .count()
}

#[test]
fn from_wire_reads_a_live_unexpired_lease() {
    let (vector, target) = open_turn();
    let records = records(&vector);
    let live = ArrivalEvidence::from_wire(
        &records,
        &[],
        &[lease_event(&target, "live", 3, NOW - 30)],
        NOW,
    );
    assert_eq!(arriving_count(&live, &records), 1);
}

#[test]
fn from_wire_treats_a_lapsed_or_released_lease_as_none() {
    let (vector, target) = open_turn();
    let records = records(&vector);
    let lapsed = ArrivalEvidence::from_wire(
        &records,
        &[],
        &[lease_event(&target, "live", 3, NOW - 600)],
        NOW,
    );
    assert_eq!(arriving_count(&lapsed, &records), 0);
    // The released record wins on lease sequence, so the earlier live one no
    // longer counts.
    let released = ArrivalEvidence::from_wire(
        &records,
        &[],
        &[
            lease_event(&target, "live", 3, NOW - 30),
            lease_event(&target, "released", 4, NOW - 10),
        ],
        NOW,
    );
    assert_eq!(arriving_count(&released, &records), 0);
}

#[test]
fn from_wire_ends_a_target_on_a_session_ending_status_only() {
    let (vector, target) = open_turn();
    let records = records(&vector);
    let lease = [lease_event(&target, "live", 3, NOW - 30)];
    for (status, arriving) in [
        (SessionStatus::Stopped, 0),
        (SessionStatus::Completed, 0),
        (SessionStatus::Disconnected, 0),
        (SessionStatus::Interrupted, 1),
        (SessionStatus::Failed, 1),
        (SessionStatus::Running, 1),
    ] {
        let (metadata, _) = decode_metadata(&[metadata_event(&target, status, NOW - 5)]);
        assert_eq!(metadata.len(), 1, "{status:?}");
        let evidence = ArrivalEvidence::from_wire(&records, &metadata, &lease, NOW);
        assert_eq!(arriving_count(&evidence, &records), arriving, "{status:?}");
    }
}

#[test]
fn from_wire_ends_a_generation_a_newer_one_superseded() {
    let vector = vector("abandoned-generation-not-arriving");
    let records = records(&vector);
    let generation_one = target_of(&vector["sessionLease"][0]["target"]);
    let only_generation_one: Vec<TranscriptRecord> = records
        .iter()
        .filter(|record| record.envelope.session.generation == generation_one.generation)
        .cloned()
        .collect();
    let lease = [lease_event(&generation_one, "live", 3, NOW - 30)];
    // Rendering one generation alone cannot see its successor; the evidence,
    // built over every transcript the caller fetched, can.
    let blind = ArrivalEvidence::from_wire(&only_generation_one, &[], &lease, NOW);
    assert_eq!(arriving_count(&blind, &only_generation_one), 1);
    let informed = ArrivalEvidence::from_wire(&records, &[], &lease, NOW);
    assert_eq!(arriving_count(&informed, &only_generation_one), 0);
}
