use super::*;
use crate::redaction_vault;

const CLAIM: &str = "CLAIM_CANARY_8fc31";
const PRIOR: &str = "PRIOR_CANARY_290fd";
const REASON: &str = "REASON_CANARY_429ae";
const EXCERPT: &str = "EXCERPT_CANARY_42a89";
const DIRECTION: &str = "/Users/private/DIRECTION_CANARY_64f09";

fn translator() -> TranscriptTranslator {
    let mut translator = TranscriptTranslator::new(true).with_paragraph_flush(true);
    translator
        .register_continuity_fragments(
            &[CLAIM, PRIOR, REASON, EXCERPT, DIRECTION].map(str::to_owned),
        )
        .unwrap();
    translator
}

fn chunk(text: &str) -> Value {
    json!({"sessionUpdate":"agent_message_chunk", "content":{"type":"text", "text":text}})
}

// Exercise the actual publish fitter/recorder, then resolve every recorded
// digest as another local reader could. Neither wire nor vault may carry it.
fn assert_private(items: Vec<Value>) -> String {
    let dir = tempfile::tempdir().unwrap();
    let mut published = Vec::new();
    let mut digests = Vec::new();
    for item in items {
        let (item, records) = fit_item_recording(item, 512, 32768);
        digests.extend(records.iter().map(|r| r.digest.clone()));
        redaction_vault::append(
            dir.path(),
            "privacy-test",
            &records,
            1,
            redaction_vault::RetentionPolicy::default(),
        )
        .unwrap();
        published.push(item);
    }
    let wire = serde_json::to_string(&published).unwrap();
    let recovered = serde_json::to_string(
        &redaction_vault::resolve(dir.path(), "privacy-test", &digests).unwrap(),
    )
    .unwrap();
    for private in [CLAIM, PRIOR, REASON, EXCERPT, DIRECTION] {
        assert!(!wire.contains(private), "private text on wire");
        assert!(!recovered.contains(private), "private text recoverable");
        let digest = hex::encode(Sha256::digest(serde_json::to_vec(private).unwrap()));
        assert!(
            !wire.contains(&digest),
            "private dictionary-testable digest on wire"
        );
    }
    assert!(!wire.contains("DIRECTION_CANARY"));
    assert!(!recovered.contains("DIRECTION_CANARY"));
    wire
}

#[test]
fn continuity_privacy_adapter_echo_tools_results_and_errors() {
    let mut t = translator();
    let mut items = t.begin_turn("ordinary signed user request", None, None, None, 0);
    items.extend(t.on_update(&chunk(&format!(
        "Claim: {CLAIM}; was {PRIOR}; because {REASON}"
    ))));
    items.extend(
        t.on_update(&json!({"sessionUpdate":"tool_call", "toolCallId":"read-1",
        "title":DIRECTION, "kind":"read", "rawInput":{"query":EXCERPT, (DIRECTION):CLAIM}})),
    );
    items.extend(t.on_update(
        &json!({"sessionUpdate":"tool_call_update", "toolCallId":"read-1",
        "status":"failed", "rawOutput":{"error":REASON, "details":[CLAIM,EXCERPT,DIRECTION]}}),
    ));
    items.extend(t.on_update(
        &json!({"sessionUpdate":"agent_thought_chunk", "content":{"type":"text", "text":EXCERPT}}),
    ));
    items.extend(t.on_update(&json!({"sessionUpdate":"plan", "entries":[{"content":CLAIM,"priority":"high","status":"pending"}]})));
    items.extend(t.on_update(&chunk("Public progress: done.")));
    items.extend(t.end_turn(json!({"kind":"result", "error":format!("failed: {PRIOR}" )})));
    let wire = assert_private(items);
    assert!(wire.contains("tool_call"));
    assert!(wire.contains("tool_result"));
    assert!(wire.contains("ordinary signed user request"));
    assert!(wire.contains("Public progress: done."));
    assert!(wire.contains("reasoning"));
    assert!(wire.contains("plan"));
    assert!(wire.contains(CONTINUITY_MASK));
}

#[test]
fn continuity_privacy_streamed_echo_crosses_size_and_subagent_boundaries() {
    let mut t = translator();
    let split = 8;
    let mut items = t.on_update(&chunk(&format!(
        "{}{}",
        "z".repeat(COALESCE_FLUSH_BYTES),
        &CLAIM[..split]
    )));
    items.extend(t.on_update(&chunk(&CLAIM[split..])));
    for text in [&EXCERPT[..9], &EXCERPT[9..]] {
        let mut update = chunk(text);
        update["_meta"] = json!({"claudeCode":{"parentToolUseId":"agent-1"}});
        items.extend(t.on_update(&update));
    }
    items.extend(t.close_turn());
    let wire = assert_private(items);
    assert!(!wire.contains(&CLAIM[..split]));
    assert!(!wire.contains(&CLAIM[split..]));
    assert!(wire.contains("parentToolId"));
}

#[test]
fn continuity_privacy_terminal_deltas_cannot_be_replaced_by_raw_native_output() {
    let mut t = translator();
    let mut items = t.on_update(&json!({"sessionUpdate":"tool_call", "toolCallId":"shell-1",
        "title":"Terminal", "rawInput":{"command":"read"},
        "_meta":{"terminal_info":{"terminal_id":"terminal-1"}}}));
    for data in [&DIRECTION[..18], &DIRECTION[18..]] {
        items.extend(t.on_update(
            &json!({"sessionUpdate":"tool_call_update", "toolCallId":"shell-1",
            "_meta":{"terminal_output_delta":{"terminal_id":"terminal-1", "data":data}}}),
        ));
    }
    items.extend(t.on_update(
        &json!({"sessionUpdate":"tool_call_update", "toolCallId":"shell-1", "status":"completed"}),
    ));
    assert!(t.take_stream_checks().is_empty());
    let wire = assert_private(items);
    assert!(wire.contains("streamed_deltas"));
    assert!(wire.contains("\"outputComplete\":false"));
}

#[test]
fn continuity_privacy_json_escaped_strings_keys_and_overlaps() {
    let mut t = translator();
    let private = "quoted \"claim\"\nwith\\escape";
    t.register_continuity_fragments(&[private.into(), "ABCDE".into(), "CDEFG".into()])
        .unwrap();
    let once = serde_json::to_string(private).unwrap();
    let twice = serde_json::to_string(&once).unwrap();
    let items =
        t.end_turn(json!({"kind":"result", "error":twice, (private):private, "overlap":"ABCDEFG"}));
    let wire = assert_private(items);
    assert!(!wire.contains("claim"));
    assert!(!wire.contains("escape"));
    assert!(!wire.contains("AB"));
    assert!(!wire.contains("FG"));
}

#[test]
fn continuity_privacy_registration_is_atomic_bounded_and_lasts_across_turns() {
    let mut t = translator();
    assert_eq!(super::MAX_CONTINUITY_FRAGMENT_BYTES, 32768);
    assert_eq!(
        t.register_continuity_fragments(&["x".repeat(32769)]),
        Err(CONTINUITY_PRIVATE_DELIVERY_UNAVAILABLE)
    );
    let too_many: Vec<_> = (0..800).map(|i| format!("extra-private-{i}")).collect();
    assert_eq!(
        t.register_continuity_fragments(&too_many),
        Err(CONTINUITY_PRIVATE_DELIVERY_UNAVAILABLE)
    );
    assert_eq!(t.continuity.patterns.len(), 5);
    t.begin_turn("ordinary", None, None, None, 0);
    t.on_update(&chunk(CLAIM));
    assert_private(t.close_turn());
    t.begin_turn("another", None, None, None, 0);
    t.on_update(&chunk(EXCERPT));
    assert_private(t.flush_all());
    let debug = format!("{:?}", t.continuity);
    assert!(!debug.contains(CLAIM));
}

#[test]
fn continuity_privacy_registration_precedes_output_and_replacement_re_registers() {
    // This proves the ordering requirement rather than pretending registration
    // can recall an event that has already been published.
    let mut before = TranscriptTranslator::new(false);
    before.on_update(&chunk(CLAIM));
    assert!(serde_json::to_string(&before.close_turn())
        .unwrap()
        .contains(CLAIM));
    let mut replacement = translator();
    replacement.on_update(&chunk(CLAIM));
    assert_private(replacement.close_turn());
}

#[test]
fn continuity_privacy_plain_session_keeps_ordinary_translation_and_recoverability() {
    let mut t = TranscriptTranslator::new(false);
    t.on_update(&chunk("Ordinary answer."));
    assert_eq!(
        t.close_turn(),
        vec![json!({"kind":"assistant_text", "text":"Ordinary answer."})]
    );
    let (_, records) = fit_item_recording(
        json!({"kind":"assistant_text", "text":DIRECTION}),
        512,
        32768,
    );
    assert!(records
        .iter()
        .any(|r| r.plaintext.contains("DIRECTION_CANARY")));
}

#[test]
fn continuity_privacy_oversized_tool_values_are_masked_before_preview_and_digest() {
    let mut t = translator();
    let input = format!("{}{}{}", "z".repeat(4090), CLAIM, "z".repeat(12000));
    let mut items = t.on_update(&json!({"sessionUpdate":"tool_call", "toolCallId":"big", "title":"Read", "rawInput":{"text":input}}));
    items.extend(t.on_update(&json!({"sessionUpdate":"tool_call_update", "toolCallId":"big", "status":"completed", "rawOutput":format!("{EXCERPT}{}", "z".repeat(16000))})));
    assert_private(items);
}

#[test]
fn continuity_privacy_limits_count_utf8_bytes_and_reject_without_partial_additions() {
    let mut t = translator();
    t.register_continuity_fragments(&["é".repeat(4096)])
        .unwrap();
    let before = t.continuity.patterns.clone();
    assert_eq!(
        // 16,385 characters but 32,770 bytes: over the byte bound.
        t.register_continuity_fragments(&["new-fragment".into(), "é".repeat(16385)]),
        Err(CONTINUITY_PRIVATE_DELIVERY_UNAVAILABLE)
    );
    assert_eq!(t.continuity.patterns, before);
    t.register_continuity_fragments(&[CLAIM.into(), String::new()])
        .unwrap();
    assert_eq!(t.continuity.patterns, before);
}

#[test]
fn continuity_privacy_structured_scalar_and_buffer_debug_do_not_escape() {
    let mut t = translator();
    t.register_continuity_fragments(&["78235614".into()])
        .unwrap();
    let private_prefix = &CLAIM[..12];
    t.on_update(&chunk(private_prefix));
    assert!(!format!("{t:?}").contains(private_prefix));
    let items = t.end_turn(json!({"kind":"result", "nested":[{"claim":78235614}]}));
    let wire = assert_private(items);
    assert!(!wire.contains("78235614"));
    assert!(!wire.contains(private_prefix));
}

/// Ledger 395: the admitted block is registered whole, so registration must
/// accept every block the dynamic bound admits and every runtime-sized
/// fragment. A literal 8192 refused the 11,324-byte canonical seat block after
/// `prepare` had admitted it.
#[test]
fn continuity_registration_accepts_every_block_and_briefing_the_bounds_admit() {
    use crate::continuity_bootstrap::{MAX_BOOTSTRAP_BYTES, MAX_RUNTIME_BRIEFING_BYTES};
    let mut t = translator();
    assert_eq!(
        t.register_continuity_fragments(&["b".repeat(MAX_BOOTSTRAP_BYTES)]),
        Ok(())
    );
    assert_eq!(
        t.register_continuity_fragments(&["r".repeat(MAX_RUNTIME_BRIEFING_BYTES)]),
        Ok(())
    );
    // The measured canonical block size registers too.
    assert_eq!(
        t.register_continuity_fragments(&["c".repeat(11324)]),
        Ok(())
    );
}
