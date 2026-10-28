//! The shared twin carries no model or tool text, whatever the item holds.
use super::*;
use serde_json::json;

/// A value the model derived from a private claim, its paraphrase, and a file
/// name built from it: none is a registered fragment, so only the absence of
/// a field to carry them keeps them out of the shared copy.
const DERIVED: &str = "C4B-1f2e3d";
const PARAPHRASE: &str = "the bench now reads one-f two-e three-d";
const FILE: &str = "notes/C4B-1f2e3d.txt";

fn private_items() -> Vec<Value> {
    vec![
        json!({"kind":"assistant_text","text":format!("The code is {DERIVED}; {PARAPHRASE}.")}),
        json!({"kind":"assistant_text","text":"More prose in the same turn."}),
        json!({"kind":"reasoning","text":PARAPHRASE}),
        json!({"kind":"plan","entries":[{"content":format!("write {FILE}"),"status":"completed","priority":"high"},{"content":PARAPHRASE,"status":"pending"}]}),
        json!({"kind":"tool_call","tool":{"toolName":format!("echo {DERIVED}"),"toolKind":"execute","toolId":"toolu_01AbC","input":{"command":format!("echo {DERIVED}")}}}),
        json!({"kind":"tool_call","tool":{"toolName":format!("Write {FILE}"),"toolKind":"edit","toolId":"toolu_02","input":{"file_path":FILE,"content":DERIVED},"edit":{"paths":[FILE],"changes":[{"path":FILE,"newText":DERIVED}]}}}),
        json!({"kind":"tool_result","toolId":"toolu_02","toolName":format!("Write {FILE}"),"toolKind":"edit","input":{"file_path":FILE},"edit":{"paths":[FILE]},"content":format!("wrote {DERIVED}"),"isError":false}),
        json!({"kind":"tool_result","toolId":format!("id with {DERIVED}"),"toolKind":"made-up","content":PARAPHRASE,"isError":true,"exitCode":1}),
        json!({"kind":"result","subtype":"success","isError":false,"durationMs":1200,"result":format!("Done: {DERIVED}"),"detail":PARAPHRASE,"costUsd":0.01,"costBasis":"adapter_estimate","usage":{"inputTokens":10,"note":DERIVED}}),
        json!({"kind":"context_window_updated","usage":{"used":5,"size":100,"label":DERIVED}}),
        json!({"kind":"elided","reason":"oversize","byteCount":9,"contentDigest":"ab".repeat(32)}),
        json!({"kind":"future_kind","text":DERIVED}),
    ]
}

#[test]
fn the_shared_twin_has_no_field_for_derived_substrings_paraphrases_or_file_names() {
    let mut state = TwinState::default();
    let twins: Vec<Value> = private_items()
        .iter()
        .filter_map(|item| state.twin(item))
        .collect();
    let shared = serde_json::to_string(&twins).unwrap();
    for private in [
        DERIVED,
        PARAPHRASE,
        FILE,
        "1f2e3d",
        "one-f",
        "Write ",
        "echo",
        "ab".repeat(32).as_str(),
    ] {
        assert!(
            !shared.contains(private),
            "{private} reached the twin: {shared}"
        );
    }
    // One prose notice per turn; reasoning, unknown kinds and nothing else.
    let kinds: Vec<&str> = twins.iter().map(|t| t["kind"].as_str().unwrap()).collect();
    assert_eq!(
        kinds,
        [
            "assistant_text",
            "plan",
            "tool_call",
            "tool_call",
            "tool_result",
            "tool_result",
            "result",
            "context_window_updated",
            "elided"
        ]
    );
    assert_eq!(twins[0]["text"], PRIVATE_PROSE_NOTICE);
    assert_eq!(
        twins[2]["tool"],
        json!({"toolKind":"execute","toolName":"Terminal","toolId":"toolu_01AbC"})
    );
    assert_eq!(
        twins[4],
        json!({"kind":"tool_result","toolKind":"edit","toolName":"Edit","toolId":"toolu_02","isError":false})
    );
    // An id that could be prose, and an unknown discriminant, are not repeated.
    assert_eq!(
        twins[5],
        json!({"kind":"tool_result","toolKind":"other","toolName":"Tool","isError":true})
    );
    assert_eq!(twins[6]["result"], PRIVATE_RESULT_NOTICE);
    assert_eq!(twins[6]["usage"], json!({"inputTokens":10}));
    assert_eq!(
        twins[1]["text"],
        "Plan: 1 of 2 steps completed (steps are private)."
    );
}

#[test]
fn the_twin_is_deterministic_and_signed_input_keeps_its_visibility() {
    let prompt = json!({"kind":"user_prompt","content":"@agent proceed","operatorPubkey":"22".repeat(32),"commandId":"c1"});
    let run = || {
        let mut state = TwinState::default();
        let mut out = vec![state.twin(&prompt)];
        out.extend(private_items().iter().map(|i| state.twin(i)));
        out.push(state.twin(&prompt));
        out.push(state.twin(&private_items()[0]));
        out
    };
    let first = run();
    assert_eq!(first, run());
    assert_eq!(first[0].as_ref(), Some(&prompt));
    // A new turn announces its prose again, once.
    assert_eq!(
        first.last().unwrap().as_ref().unwrap()["text"],
        PRIVATE_PROSE_NOTICE
    );
}

#[test]
fn a_split_covers_its_generation_and_every_later_one() {
    let split = PrivateOutput {
        channel: Uuid::nil(),
        owner: "22".repeat(32),
        from_generation: 2,
    };
    assert!(!split.covers(1));
    assert!(split.covers(2));
    assert!(split.covers(3));
}

#[test]
fn only_a_relay_signed_dm_record_naming_exactly_the_three_participants_binds() {
    let relay = nostr::Keys::generate();
    let impostor = nostr::Keys::generate();
    let channel = Uuid::new_v4();
    let mut expected = vec!["11".repeat(32), "22".repeat(32), "33".repeat(32)];
    expected.sort();
    let record = |keys: &nostr::Keys, t: &str, members: &[String], at: u64| {
        let mut tags = vec![
            nostr::Tag::parse(["d", &channel.to_string()]).unwrap(),
            nostr::Tag::parse(["t", t]).unwrap(),
        ];
        tags.extend(members.iter().map(|p| nostr::Tag::parse(["p", p]).unwrap()));
        nostr::EventBuilder::new(nostr::Kind::Custom(39000), "")
            .tags(tags)
            .custom_created_at(nostr::Timestamp::from(at))
            .sign_with_keys(keys)
            .unwrap()
    };
    let verdict = |events: &[nostr::Event]| {
        participants_verdict(events, &relay.public_key(), channel, &expected)
    };
    assert_eq!(verdict(&[record(&relay, "dm", &expected, 10)]), Some(true));
    // A teammate added later is a different participant set.
    let mut widened = expected.clone();
    widened.push("44".repeat(32));
    assert_eq!(
        verdict(&[
            record(&relay, "dm", &expected, 10),
            record(&relay, "dm", &widened, 11)
        ]),
        Some(false)
    );
    assert_eq!(
        verdict(&[record(&relay, "stream", &expected, 10)]),
        Some(false)
    );
    // Not the relay's record: not evidence of membership at all.
    assert_eq!(verdict(&[record(&impostor, "dm", &expected, 10)]), None);
    assert_eq!(verdict(&[]), None);
}
