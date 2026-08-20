use super::*;

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "codex-primary".into(),
        session_id: "session-1".into(),
        generation: 1,
    }
}

fn history(seq: u64, kind: &str) -> CodingSessionContextHistoryItem {
    CodingSessionContextHistoryItem {
        event_id: format!("{seq:064x}"),
        created_at: seq,
        author: "ab".repeat(32),
        source_kind: crate::kind::KIND_CODING_SESSION_TRANSCRIPT,
        target: target(),
        event_seq: seq,
        turn_id: Some("turn-1".into()),
        role: coding_session_context_role_for_item_kind(kind).unwrap(),
        item_kind: kind.into(),
        content: serde_json::json!({"kind": kind, "text": "verified relay fact"}),
    }
}

fn package() -> CodingSessionContextPackage {
    CodingSessionContextPackage {
        v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
        session: CodingSessionContextIdentity {
            session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".into(),
            genesis_ref: "cd".repeat(32),
            channel_id: Uuid::nil(),
            name: Some("Rehydrated context".into()),
            goal: Some("Continue from verified durable facts".into()),
            project_ref: Some(format!("30621:{}:buzz", "ef".repeat(32))),
        },
        provenance: CodingSessionContextProvenance {
            generated_at: 1,
            complete_as_of: Some(1),
            complete: true,
            truncated: false,
            source_event_count: 4,
            source_event_breakdown: None,
            included_history_items: 2,
            omitted_history_items: 0,
            total_history_items: Some(2),
            notes: vec!["Relay query reached EOSE without a local package truncation".into()],
        },
        history: vec![history(1, "user_prompt"), history(2, "assistant_text")],
    }
}

#[test]
fn valid_package_round_trips_as_strict_json() {
    let package = package();
    package.validate().unwrap();
    let encoded = serde_json::to_string(&package).unwrap();
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, package);
    decoded.validate().unwrap();
}

#[test]
fn additive_watermark_keeps_legacy_packages_readable() {
    let mut encoded = serde_json::to_value(package()).unwrap();
    encoded["provenance"]
        .as_object_mut()
        .unwrap()
        .remove("completeAsOf");
    let decoded: CodingSessionContextPackage = serde_json::from_value(encoded).unwrap();
    assert_eq!(decoded.provenance.complete_as_of, None);
    decoded.validate().unwrap();
}

#[test]
fn completeness_and_truncation_are_independent_but_accounted() {
    let mut context = package();
    context.provenance.complete = false;
    context.provenance.complete_as_of = None;
    context.provenance.total_history_items = None;
    context.validate().unwrap();
    context.provenance.truncated = true;
    context.provenance.omitted_history_items = 3;
    context.validate().unwrap();
    context.provenance.truncated = false;
    assert!(context.validate().is_err());

    let mut unsafe_note = package();
    unsafe_note.provenance.notes = vec!["queried /Users/alice/private/repo".into()];
    assert!(unsafe_note.validate().is_err());
}

#[test]
fn rejects_conflicts_and_semantic_role_substitution() {
    let mut conflicting = package();
    conflicting.history[1].event_seq = 1;
    assert!(conflicting.validate().is_err());

    let mut substituted = package();
    substituted.history[0].role = CodingSessionContextRole::Assistant;
    assert!(substituted.validate().is_err());

    let mut mismatched = package();
    mismatched.history[0].content["kind"] = Value::String("assistant_text".into());
    assert!(mismatched.validate().is_err());
}

#[test]
fn rejects_unknown_fields_and_unrecognized_item_kinds() {
    let encoded = serde_json::to_value(package()).unwrap();
    let mut smuggled = encoded.clone();
    smuggled
        .as_object_mut()
        .unwrap()
        .insert("privateKey".into(), Value::String("secret".into()));
    assert!(serde_json::from_value::<CodingSessionContextPackage>(smuggled).is_err());

    let mut unrecognized = package();
    unrecognized.history[0].item_kind = "native_provider_state".into();
    unrecognized.history[0].content["kind"] = Value::String("native_provider_state".into());
    assert!(unrecognized.validate().is_err());

    let mut host_project = package();
    host_project.session.project_ref = Some(format!(
        "30621:{}:/Users/alice/private/repo",
        "ef".repeat(32)
    ));
    assert!(host_project.validate().is_err());
}

#[test]
fn rejects_an_item_too_large_to_return_in_full() {
    let mut package = package();
    package.history[0].content["text"] =
        Value::String("x".repeat(MAX_CONTEXT_HISTORY_CONTENT_BYTES + 1));
    assert!(package.validate().is_err());
}

#[test]
fn first_turn_brief_indexes_outcomes_without_replaying_sensitive_content() {
    let mut package = package();
    package.history = vec![
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "user_prompt",
                "content": "Inspect /Users/alice/private/repo with password hunter2",
                "operatorPubkey": "ab".repeat(32),
                "steered": false
            }),
            ..history(1, "user_prompt")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "tool_call",
                "tool": {"toolName": "cargo_test", "toolId": "tool-1", "input": {}}
            }),
            ..history(2, "tool_call")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "tool_result",
                "toolName": "cargo_test",
                "toolId": "tool-1",
                "content": "failed at /Users/alice/private/repo",
                "isError": true
            }),
            ..history(3, "tool_result")
        },
        CodingSessionContextHistoryItem {
            content: serde_json::json!({
                "kind": "result",
                "subtype": "error",
                "isError": true,
                "durationMs": 10,
                "result": "stopped: token limit reached"
            }),
            ..history(4, "result")
        },
    ];
    package.provenance.included_history_items = 4;
    package.provenance.total_history_items = Some(4);

    let brief = coding_session_first_turn_brief(&package);
    let encoded = serde_json::to_string(&brief).unwrap();
    validate_coding_session_first_turn_brief_json(&encoded).unwrap();
    assert!(!encoded.contains("/Users/alice"));
    assert!(!encoded.contains("hunter2"));
    assert_eq!(brief["recentTurns"][0]["request"], Value::Null);
    assert_eq!(brief["recentTurns"][0]["requestOmittedForSafety"], true);
    assert_eq!(brief["recentTurns"][0]["outcome"], "token_limit");
    assert_eq!(
        brief["recentTurns"][0]["toolAttempts"][0]["outcome"],
        "failed"
    );
    assert_eq!(brief["snapshot"]["completeAsOf"], 1);

    let substituted = encoded.replace(
        "coding-session-first-turn-brief/v1",
        "/Users/alice/private/brief",
    );
    assert!(validate_coding_session_first_turn_brief_json(&substituted).is_err());

    let path_only = sanitize_coding_session_context_text(
        "Inspect /Users/alice/private/repo then crates/buzz-core/src/lib.rs",
    );
    assert!(path_only.starts_with("Inspect [elided private context:"));
    assert!(!path_only.contains("/Users/alice"));
    assert!(path_only.contains("crates/buzz-core/src/lib.rs"));
    assert!(
        !sanitize_coding_session_context_text("Read(/Users/alice/repo)").contains("/Users/alice")
    );

    let secret_field = serde_json::json!({"kind": "tool_call", "password": "hunter2"});
    let sanitized = sanitize_coding_session_context_content(&secret_field);
    assert_eq!(
        sanitize_coding_session_context_content(&sanitized),
        sanitized,
        "sanitization must be idempotent so strict package validation accepts it"
    );
    let token_field = sanitize_coding_session_context_content(
        &serde_json::json!({"kind": "tool_call", "accessToken": "opaque"}),
    );
    assert!(!token_field.to_string().contains("opaque"));
}

#[test]
fn first_turn_brief_is_byte_bounded_and_reports_omitted_turns() {
    let mut package = package();
    package.history = (1..=40)
        .map(|seq| CodingSessionContextHistoryItem {
            turn_id: Some(format!("turn-{seq}")),
            content: serde_json::json!({
                "kind": "user_prompt",
                "content": "x".repeat(MAX_CONTEXT_BRIEF_TEXT_BYTES)
            }),
            ..history(seq, "user_prompt")
        })
        .collect();
    package.provenance.included_history_items = 40;
    package.provenance.total_history_items = Some(40);

    let brief = coding_session_first_turn_brief(&package);
    let encoded = serde_json::to_vec(&brief).unwrap();

    assert!(encoded.len() <= MAX_CONTEXT_FIRST_TURN_BRIEF_BYTES);
    assert_eq!(brief["snapshot"]["indexedTurnCount"], 40);
    assert!(brief["snapshot"]["omittedTurnCount"].as_u64().unwrap() > 0);
}

/// A package whose `sourceEventCount` is reconciled by an explicit breakdown:
/// one genesis, two authority links (two events each), one name revision, one
/// goal revision, one generation (three bookkeeping events) and the two
/// transcript facts that became history items.
fn reconciled() -> CodingSessionContextPackage {
    let breakdown = CodingSessionContextSourceBreakdown {
        genesis_events: 1,
        authority_link_events: 4,
        name_revision_events: 1,
        goal_revision_events: 1,
        generation_bookkeeping_events: 3,
        transcript_events: 2,
    };
    let mut package = package();
    package.provenance.source_event_count = breakdown.total();
    package.provenance.source_event_breakdown = Some(breakdown);
    package
}

#[test]
fn source_breakdown_terms_sum_to_source_event_count() {
    let package = reconciled();
    package.validate().unwrap();

    let breakdown = package.provenance.source_event_breakdown.clone().unwrap();
    assert_eq!(breakdown.total(), 12);
    assert_eq!(breakdown.total(), package.provenance.source_event_count);
    // Only transcript events can become history items — that is the whole
    // reason sourceEventCount exceeds totalHistoryItems.
    assert_eq!(
        breakdown.transcript_events,
        package.provenance.total_history_items.unwrap()
    );

    let encoded = serde_json::to_string(&package).unwrap();
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, package);
    decoded.validate().unwrap();
    for key in [
        "genesisEvents",
        "authorityLinkEvents",
        "nameRevisionEvents",
        "goalRevisionEvents",
        "generationBookkeepingEvents",
        "transcriptEvents",
    ] {
        assert!(encoded.contains(key), "{key} must be on the wire");
    }
}

#[test]
fn a_package_whose_breakdown_does_not_sum_is_rejected() {
    for terms in [2, 4] {
        let mut package = reconciled();
        let breakdown = package.provenance.source_event_breakdown.as_mut().unwrap();
        breakdown.generation_bookkeeping_events = terms;
        let error = package.validate().unwrap_err();
        assert!(
            error.contains("sourceEventBreakdown"),
            "unexpected error: {error}"
        );
    }

    // A term large enough to overflow the sum is a rejection, not a wrap.
    let mut overflowing = reconciled();
    overflowing
        .provenance
        .source_event_breakdown
        .as_mut()
        .unwrap()
        .transcript_events = u64::MAX;
    let error = overflowing.validate().unwrap_err();
    assert!(error.contains("overflows"), "unexpected error: {error}");

    // The count moving without the breakdown moving is the same defect.
    let mut package = reconciled();
    package.provenance.source_event_count += 1;
    assert!(package.validate().is_err());
}

#[test]
fn a_package_whose_transcript_events_are_fewer_than_included_plus_omitted_is_rejected() {
    let mut package = reconciled();
    package.provenance.truncated = true;
    package.provenance.omitted_history_items = 1;
    package.provenance.total_history_items = Some(3);
    let error = package.validate().unwrap_err();
    assert!(
        error.contains("transcriptEvents"),
        "unexpected error: {error}"
    );

    // Accounting for the omitted item in the breakdown makes it reconcile.
    let breakdown = package.provenance.source_event_breakdown.as_mut().unwrap();
    breakdown.transcript_events = 3;
    package.provenance.source_event_count += 1;
    package.validate().unwrap();
}

#[test]
fn a_version_1_package_without_a_breakdown_still_validates() {
    let mut package = package();
    package.v = MIN_SUPPORTED_CONTEXT_PACKAGE_VERSION;
    assert_eq!(package.provenance.source_event_breakdown, None);
    package.validate().unwrap();

    let encoded = serde_json::to_string(&package).unwrap();
    assert!(
        !encoded.contains("sourceEventBreakdown"),
        "an absent breakdown must be omitted, never emitted as null"
    );
    let decoded: CodingSessionContextPackage = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.provenance.source_event_breakdown, None);
    decoded.validate().unwrap();
}

#[test]
fn a_version_3_package_is_rejected_by_name() {
    let mut package = reconciled();
    package.v = CODING_SESSION_CONTEXT_PACKAGE_VERSION + 1;
    let error = package.validate().unwrap_err();
    assert!(error.contains("unsupported"), "unexpected error: {error}");
    assert!(error.contains('3'), "unexpected error: {error}");
    assert!(error.contains("1..=2"), "unexpected error: {error}");
}

#[test]
fn the_first_turn_brief_carries_the_source_breakdown() {
    let reconciled = reconciled();
    let brief = coding_session_first_turn_brief(&reconciled);
    let encoded = serde_json::to_string(&brief).unwrap();
    validate_coding_session_first_turn_brief_json(&encoded).unwrap();

    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"],
        serde_json::to_value(
            reconciled
                .provenance
                .source_event_breakdown
                .clone()
                .unwrap()
        )
        .unwrap()
    );
    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"]["genesisEvents"],
        1
    );
    assert_eq!(
        brief["snapshot"]["sourceEventBreakdown"]["transcriptEvents"],
        2
    );

    // The brief is injected standalone as the ACP bootstrap prompt, so the
    // number the six terms reconcile has to travel in the brief itself — not
    // only in the surrounding `session_overview` response.
    assert_eq!(
        brief["snapshot"]["sourceEventCount"],
        serde_json::json!(reconciled.provenance.source_event_count)
    );
    let terms = brief["snapshot"]["sourceEventBreakdown"]
        .as_object()
        .unwrap()
        .values()
        .map(|term| term.as_u64().unwrap())
        .sum::<u64>();
    assert_eq!(
        terms,
        brief["snapshot"]["sourceEventCount"].as_u64().unwrap(),
        "the breakdown terms must sum to the count printed beside them"
    );

    let rules = brief["rules"].as_array().unwrap();
    assert!(
        rules
            .iter()
            .filter_map(Value::as_str)
            .any(|rule| rule
                .contains("sourceEventBreakdown reconciles it against totalHistoryItems")),
        "the brief must say how the two numbers reconcile"
    );

    // A package with no breakdown says so explicitly rather than omitting the
    // key from a rendering a reader scans for it.
    let legacy = coding_session_first_turn_brief(&package());
    assert_eq!(legacy["snapshot"]["sourceEventBreakdown"], Value::Null);
    validate_coding_session_first_turn_brief_json(&serde_json::to_string(&legacy).unwrap())
        .unwrap();
}
