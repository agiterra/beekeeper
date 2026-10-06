//! The first-turn brief binds to the shared prose-join vectors
//! (`conformance/transcript-prose-join`, NIP-CST amendment 3 "Join key").

use super::*;

const VECTORS: &str =
    include_str!("../../../conformance/transcript-prose-join/fixtures/vectors.json");

fn vectors() -> Vec<Value> {
    let file: Value = serde_json::from_str(VECTORS).expect("vectors.json is JSON");
    assert_eq!(
        file["schema"], "buzz.conformance/transcript-prose-join@1",
        "the brief binds to this fixture schema only"
    );
    file["vectors"]
        .as_array()
        .expect("vectors is an array")
        .clone()
}

/// Map one reader envelope `{eventId, signer, content}` to the package item a
/// projector would make of it.
fn history_item(envelope: &Value) -> CodingSessionContextHistoryItem {
    let content = &envelope["content"];
    let session = &content["session"];
    let item = sanitize_coding_session_context_content(&content["item"]);
    let item_kind = item["kind"].as_str().expect("item kind").to_owned();
    CodingSessionContextHistoryItem {
        event_id: envelope["eventId"].as_str().expect("eventId").to_owned(),
        created_at: content["timestamp"].as_u64().expect("timestamp") / 1_000,
        author: envelope["signer"].as_str().expect("signer").to_owned(),
        source_kind: crate::kind::KIND_CODING_SESSION_TRANSCRIPT,
        target: CodingSessionTarget {
            driver: session["driver"].as_str().expect("driver").to_owned(),
            instance_id: session["instanceId"]
                .as_str()
                .expect("instanceId")
                .to_owned(),
            session_id: session["sessionId"].as_str().expect("sessionId").to_owned(),
            generation: session["generation"].as_u64().expect("generation"),
        },
        event_seq: content["eventSeq"].as_u64().expect("eventSeq"),
        turn_id: content["turnId"].as_str().map(str::to_owned),
        role: coding_session_context_role_for_item_kind(&item_kind).expect("known item kind"),
        item_kind,
        content: item,
    }
}

fn package_with(history: Vec<CodingSessionContextHistoryItem>) -> CodingSessionContextPackage {
    let count = history.len() as u64;
    CodingSessionContextPackage {
        v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
        session: CodingSessionContextIdentity {
            session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
            genesis_ref: "cd".repeat(32),
            channel_id: Uuid::nil(),
            name: None,
            goal: None,
            project_ref: None,
        },
        provenance: CodingSessionContextProvenance {
            generated_at: 1,
            complete_as_of: Some(1),
            complete: true,
            truncated: false,
            source_event_count: count + 2,
            source_event_breakdown: None,
            included_history_items: count,
            omitted_history_items: 0,
            total_history_items: Some(count),
            notes: Vec::new(),
        },
        history,
        roster: Vec::new(),
        inbox: Vec::new(),
        policy: None,
    }
}

/// The signers of a vector, in first-seen order. A package is one provider's
/// projection, so each exact target in it has one signer; the brief is built
/// once per signer, as a reader holding that signer's facts would.
fn signers(vector: &Value) -> Vec<String> {
    let mut signers = Vec::<String>::new();
    for envelope in vector["input"].as_array().expect("input") {
        let signer = envelope["signer"].as_str().expect("signer").to_owned();
        if !signers.contains(&signer) {
            signers.push(signer);
        }
    }
    signers
}

fn assert_brief_matches(name: &str, shape: &str, signer: &str, vector: &Value, brief: &Value) {
    let encoded = serde_json::to_string(brief).expect("brief encodes");
    validate_coding_session_first_turn_brief_json(&encoded)
        .unwrap_or_else(|error| panic!("{name} ({shape}): validator refused the brief: {error}"));
    assert_eq!(brief["schema"], "coding-session-first-turn-brief/v1");

    let turns = brief["recentTurns"].as_array().expect("recentTurns");
    let expected = vector["expectedBrief"]
        .as_array()
        .expect("expectedBrief")
        .iter()
        .filter(|entry| entry["signer"] == signer)
        .collect::<Vec<_>>();
    assert_eq!(
        turns.len(),
        expected.len(),
        "{name} ({shape}): one brief turn per expected (target, turn) for signer {signer}"
    );
    for entry in expected {
        let target = &entry["target"];
        let turn = turns
            .iter()
            .find(|turn| {
                turn["turnId"] == entry["turnId"]
                    && turn["target"]["driver"] == target["driver"]
                    && turn["target"]["instanceId"] == target["instanceId"]
                    && turn["target"]["sessionId"] == target["sessionId"]
                    && turn["target"]["generation"] == target["generation"]
            })
            .unwrap_or_else(|| panic!("{name} ({shape}): no brief turn for {entry}"));
        for field in ["latestAssistantEventId", "latestAssistantFirstEventId"] {
            assert_eq!(
                turn[field], entry[field],
                "{name} ({shape}): {field} for {} {}",
                entry["turnId"], target
            );
        }
    }
}

#[test]
fn the_brief_names_each_turns_latest_own_message_by_both_ends_for_every_vector() {
    let vectors = vectors();
    assert!(!vectors.is_empty());
    for vector in &vectors {
        let name = vector["name"].as_str().expect("name");
        let input = vector["input"].as_array().expect("input");
        for signer in signers(vector) {
            let raw = input
                .iter()
                .filter(|envelope| envelope["signer"] == signer.as_str())
                .map(history_item)
                .collect::<Vec<_>>();

            // As delivered: arrival order, repeated deliveries included. The
            // brief must sort by eventSeq and absorb repeats itself.
            let brief = coding_session_first_turn_brief(&package_with(raw.clone()));
            assert_brief_matches(name, "as delivered", &signer, vector, &brief);

            // As a projector stores it: deduplicated, ordered, and valid.
            let mut stored = Vec::<CodingSessionContextHistoryItem>::new();
            for item in raw {
                if !stored.iter().any(|kept| kept.event_id == item.event_id) {
                    stored.push(item);
                }
            }
            stored.sort_by(|left, right| {
                coding_session_target_key(&left.target)
                    .cmp(&coding_session_target_key(&right.target))
                    .then_with(|| left.event_seq.cmp(&right.event_seq))
            });
            let package = package_with(stored);
            package
                .validate()
                .unwrap_or_else(|error| panic!("{name}: stored package is invalid: {error}"));
            let brief = coding_session_first_turn_brief(&package);
            assert_brief_matches(name, "as stored", &signer, vector, &brief);
        }
    }
}

fn piece(seq: u64, kind: &str, content: Value) -> CodingSessionContextHistoryItem {
    CodingSessionContextHistoryItem {
        event_id: format!("{seq:064x}"),
        created_at: seq,
        author: "ab".repeat(32),
        source_kind: crate::kind::KIND_CODING_SESSION_TRANSCRIPT,
        target: CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "claude-primary".into(),
            session_id: "session-1".into(),
            generation: 1,
        },
        event_seq: seq,
        turn_id: Some("turn-1".into()),
        role: coding_session_context_role_for_item_kind(kind).expect("known item kind"),
        item_kind: kind.into(),
        content,
    }
}

#[test]
fn a_ten_paragraph_answer_spends_one_evidence_slot_and_keeps_every_tool_event() {
    let mut history = vec![piece(
        1,
        "user_prompt",
        serde_json::json!({"kind": "user_prompt", "content": "Fix the build", "steered": false}),
    )];
    let mut seq = 1;
    let mut next = || {
        seq += 1;
        seq
    };
    for paragraph in 0..10 {
        history.push(piece(
            next(),
            "assistant_text",
            serde_json::json!({"kind": "assistant_text", "text": format!("paragraph {paragraph}.\n\n")}),
        ));
    }
    let mut tool_event_ids = Vec::new();
    for tool in 0..8 {
        let call = piece(
            next(),
            "tool_call",
            serde_json::json!({
                "kind": "tool_call",
                "tool": {"toolName": "cargo_test", "toolId": format!("tool-{tool}"), "input": {}}
            }),
        );
        tool_event_ids.push(call.event_id.clone());
        history.push(call);
    }
    let closing = (0..3)
        .map(|paragraph| {
            piece(
                next(),
                "assistant_text",
                serde_json::json!({"kind": "assistant_text", "text": format!("closing {paragraph}.\n\n")}),
            )
        })
        .collect::<Vec<_>>();
    let closing_first = closing[0].event_id.clone();
    let closing_last = closing[2].event_id.clone();
    history.extend(closing);
    history.push(piece(
        next(),
        "result",
        serde_json::json!({"kind": "result", "subtype": "success", "isError": false, "durationMs": 1}),
    ));
    let first_answer = history[1].event_id.clone();

    let package = package_with(history);
    package.validate().expect("package is valid");
    let brief = coding_session_first_turn_brief(&package);
    let encoded = serde_json::to_string(&brief).expect("brief encodes");
    validate_coding_session_first_turn_brief_json(&encoded).expect("validator accepts the brief");

    let turn = &brief["recentTurns"][0];
    let evidence = turn["evidenceEventIds"]
        .as_array()
        .expect("evidenceEventIds")
        .iter()
        .map(|id| id.as_str().expect("event id").to_owned())
        .collect::<Vec<_>>();
    for id in &tool_event_ids {
        assert!(evidence.contains(id), "tool event {id} kept as evidence");
    }
    // Prompt, the ten-paragraph run (its first id), eight calls, the closing
    // run (its first id), the result: twelve of sixteen slots.
    assert_eq!(evidence.len(), 12);
    assert!(evidence.contains(&first_answer));
    assert!(evidence.contains(&closing_first));
    assert!(!evidence.contains(&closing_last));
    assert_eq!(
        turn["toolAttempts"].as_array().expect("toolAttempts").len(),
        8
    );
    assert_eq!(turn["latestAssistantEventId"], closing_last.as_str());
    assert_eq!(turn["latestAssistantFirstEventId"], closing_first.as_str());
}

#[test]
fn a_turn_with_only_subagent_prose_names_no_latest_assistant_message() {
    let package = package_with(vec![
        piece(
            1,
            "assistant_text",
            serde_json::json!({"kind": "assistant_text", "text": "from the task", "parentToolId": "task-1"}),
        ),
        piece(
            2,
            "result",
            serde_json::json!({"kind": "result", "subtype": "success", "isError": false, "durationMs": 1}),
        ),
    ]);
    let brief = coding_session_first_turn_brief(&package);
    let turn = &brief["recentTurns"][0];
    assert_eq!(turn["latestAssistantEventId"], Value::Null);
    assert_eq!(turn["latestAssistantFirstEventId"], Value::Null);
    // Subagent prose is still evidence; it just is not the agent's answer.
    assert_eq!(
        turn["evidenceEventIds"].as_array().expect("evidence").len(),
        2
    );
}
