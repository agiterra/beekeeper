//! The prose-join vectors: what every reader of `assistant_text` and
//! `reasoning` items must turn a run of pieces into.
//!
//! `conformance/transcript-prose-join/fixtures/vectors.json` is the contract
//! (`conformance/transcript-prose-join/CONTRACT.md`, NIP-CST amendment 3
//! "Join key"). This file is the only thing that writes it, and it does so
//! from the **real** [`TranscriptTranslator`] with paragraph flushing on and
//! the [`COALESCE_FLUSH_BYTES`] size cut, plus hand-written attribution cases
//! the translator cannot produce on its own (adjacent pieces from a subagent,
//! a second signer, provider `messageId`s).
//!
//! By default the test regenerates the vectors in memory and **compares** them
//! with the checked-in file, so a translator change that moves a cut, adds a
//! field or reorders an item fails here before any reader silently disagrees.
//! `BUZZ_REGEN_PROSE_JOIN_VECTORS=1` rewrites the file instead; review the diff
//! and re-run every reader's binding test before committing it.
//!
//! Every expected message is checked twice: by the reference join below, and
//! by hand assertions per case written from what the case means (one message
//! for three paragraphs, the streamed bytes back exactly, which item is
//! arriving). The second check is what keeps the file from being a
//! self-fulfilling prophecy of the first.
//!
//! The reference join and the serializer live in the sibling
//! `transcript_prose_join_vectors_reference.rs`; the hand-written cases in
//! `transcript_prose_join_vectors_hand_written.rs`.

use std::path::PathBuf;

use buzz_core::coding_session_command::CodingSessionTarget;
use buzz_core::coding_session_payload::{
    result_item, ResultSubtype, TranscriptEnvelope, TurnCost, TurnUsageReport,
};

use super::*;

#[path = "transcript_prose_join_vectors_reference.rs"]
mod reference;
use reference::{canonical, content, group_key, item_kind, reference_join};

#[path = "transcript_prose_join_vectors_hand_written.rs"]
mod hand_written;
use hand_written::{
    distinct_message_ids, duplicate_delivery, generations_share_turn_id,
    own_prose_around_subagent_prose, reasoning_joins_like_prose, sequence_gap, two_signers,
};

const REGEN_ENV: &str = "BUZZ_REGEN_PROSE_JOIN_VECTORS";
const VECTOR_SCHEMA: &str = "buzz.conformance/transcript-prose-join@1";
/// Synthetic signers. Ids and signers in the vectors are deterministic labels,
/// not keys: a reader binds to them as opaque strings and never verifies them.
const SIGNER: &str = "5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";
const OTHER_SIGNER: &str = "0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b";
const BASE_TIMESTAMP_MS: i64 = 1_785_512_977_000;
/// Small enough that every `"\n\n"` straddles two chunks somewhere, as tokens
/// do in a live stream.
const CHUNK_BYTES: usize = 7;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/transcript-prose-join/fixtures/vectors.json")
}

// ---------------------------------------------------------------------------
// Producing envelopes
// ---------------------------------------------------------------------------

/// One exact target's published envelopes, numbered the way the provider
/// numbers them.
struct Target {
    vector: &'static str,
    signer: &'static str,
    target: CodingSessionTarget,
    next_seq: u64,
    envelopes: Vec<Value>,
}

impl Target {
    fn new(vector: &'static str, signer: &'static str, generation: u64) -> Self {
        Self {
            vector,
            signer,
            target: CodingSessionTarget {
                driver: "claude-agent-acp".to_owned(),
                instance_id: "vector-instance".to_owned(),
                session_id: "vector-session".to_owned(),
                generation,
            },
            next_seq: 1,
            envelopes: Vec::new(),
        }
    }

    /// Publish items in order, as the provider does: each fitted (redacted and
    /// bounded) on its way to the envelope. Returns the new event ids.
    fn publish(&mut self, turn_id: Option<&str>, items: Vec<Value>) -> Vec<String> {
        items
            .into_iter()
            .map(|item| {
                let fitted = fit_item(item.clone(), 512, 32 * 1024);
                assert_eq!(
                    fitted, item,
                    "a vector item must be published unchanged by fit_item"
                );
                self.push(turn_id, fitted)
            })
            .collect()
    }

    fn push(&mut self, turn_id: Option<&str>, item: Value) -> String {
        let seq = self.next_seq;
        self.next_seq += 1;
        let event_id = hex::encode(Sha256::digest(
            format!(
                "transcript-prose-join|{}|{}|{}|{}",
                self.vector, self.signer, self.target.generation, seq
            )
            .as_bytes(),
        ));
        let content = TranscriptEnvelope::new(
            &self.target,
            seq,
            BASE_TIMESTAMP_MS + i64::try_from(seq).expect("small seq") * 1000,
            turn_id,
            item,
        );
        self.envelopes.push(json!({
            "eventId": event_id,
            "signer": self.signer,
            "content": serde_json::to_value(content).expect("envelope serializes"),
        }));
        event_id
    }

    /// A sequence number reserved and lost (a producer crash): permitted by
    /// NIP-CST, and not a boundary.
    fn lose_seq(&mut self) {
        self.next_seq += 1;
    }
}

fn translator() -> TranscriptTranslator {
    TranscriptTranslator::new(true).with_paragraph_flush(true)
}

fn chunk(text: &str) -> Value {
    json!({ "sessionUpdate": "agent_message_chunk", "content": { "type": "text", "text": text } })
}

fn thought(text: &str) -> Value {
    json!({ "sessionUpdate": "agent_thought_chunk", "content": { "type": "text", "text": text } })
}

fn tool_call(id: &str) -> Value {
    json!({
        "sessionUpdate": "tool_call",
        "toolCallId": id,
        "title": "Run cargo test",
        "kind": "execute",
        "status": "pending",
        "rawInput": { "command": "cargo test" },
    })
}

fn tool_done(id: &str) -> Value {
    json!({
        "sessionUpdate": "tool_call_update",
        "toolCallId": id,
        "status": "completed",
        "content": [{ "type": "content", "content": { "type": "text", "text": "ok" } }],
    })
}

fn result() -> Value {
    result_item(
        ResultSubtype::Success,
        1234,
        "completed",
        TurnCost::default(),
        TurnUsageReport::default(),
    )
}

/// Stream `text` token-sized, publishing what each chunk flushes.
fn stream(at: &mut Target, translator: &mut TranscriptTranslator, turn: &str, text: &str) {
    let mut rest = text;
    while !rest.is_empty() {
        let mut cut = rest.len().min(CHUNK_BYTES);
        while !rest.is_char_boundary(cut) {
            cut += 1;
        }
        let (piece, tail) = rest.split_at(cut);
        let items = translator.on_update(&chunk(piece));
        at.publish(Some(turn), items);
        rest = tail;
    }
}

fn begin(at: &mut Target, translator: &mut TranscriptTranslator, turn: &str, prompt: &str) {
    let items = translator.begin_turn(prompt, Some(OTHER_SIGNER), Some("cmd"), None, 0);
    at.publish(Some(turn), items);
}

/// A paragraph of ordinary prose at least `bytes` long, with no blank line,
/// ending in the blank line that closes it.
fn paragraph(label: &str, bytes: usize) -> String {
    let mut text = format!("{label}:");
    while text.len() < bytes {
        text.push_str(" words of the answer");
    }
    text.push_str(".\n\n");
    text
}

fn prose(text: &str) -> Value {
    json!({ "kind": "assistant_text", "text": text })
}

fn subagent_prose(parent: &str, text: &str) -> Value {
    json!({ "kind": "assistant_text", "text": text, "parentToolId": parent })
}

fn with_message_id(mut item: Value, message_id: &str) -> Value {
    item["messageId"] = json!(message_id);
    item
}

fn tool_call_item(id: &str) -> Value {
    json!({ "kind": "tool_call", "tool": { "toolId": id, "toolName": "Bash", "input": { "command": "ls" } } })
}

/// One `sessionStatus[]` entry: the latest metadata (kind:44223) status a
/// reader holds for `at`'s exact target.
fn status(at: &Target, status: &str) -> Value {
    json!({
        "signer": at.signer,
        "target": serde_json::to_value(&at.target).expect("target serializes"),
        "status": status,
    })
}

/// One `sessionLease[]` entry: the reader's view, at its own `now`, of the
/// winning kind:24223 lease for `at`'s exact target — `live` (unexpired),
/// `released` (the provider's tombstone), or `lapsed` (a `live` lease past its
/// TTL: nothing renewed it).
fn lease(at: &Target, state: &str) -> Value {
    json!({
        "signer": at.signer,
        "target": serde_json::to_value(&at.target).expect("target serializes"),
        "lease": state,
    })
}

// ---------------------------------------------------------------------------
// The cases
// ---------------------------------------------------------------------------

struct Case {
    name: &'static str,
    source: &'static str,
    description: &'static str,
    input: Vec<Value>,
    /// The latest metadata status per exact target, where the case has one.
    session_status: Vec<Value>,
    /// The reader's lease per exact target, where it holds one.
    session_lease: Vec<Value>,
}

fn messages_of(case: &Case) -> Vec<Value> {
    reference_join(&case.input, &case.session_status, &case.session_lease).0
}

fn texts(messages: &[Value]) -> Vec<&str> {
    messages
        .iter()
        .map(|m| m["text"].as_str().unwrap_or(""))
        .collect()
}

fn pieces(case: &Case, kind: &str) -> usize {
    case.input.iter().filter(|e| item_kind(e) == kind).count()
}

fn three_paragraphs() -> Case {
    let name = "three-paragraphs-one-turn";
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "Explain the change.");
    let answer = [
        paragraph("first", MIN_PARAGRAPH_FLUSH_BYTES),
        paragraph("second", MIN_PARAGRAPH_FLUSH_BYTES),
        "third: and that is all.".to_owned(),
    ]
    .concat();
    stream(&mut at, &mut tr, "turn-1", &answer);
    let items = tr.end_turn(result());
    at.publish(Some("turn-1"), items);
    let case = Case {
        name,
        source: "translator",
        description: "Three paragraphs flushed one by one join back into one message whose text is exactly the streamed answer.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert_eq!(
        pieces(&case, "assistant_text"),
        3,
        "{name}: one piece per paragraph"
    );
    let messages = messages_of(&case);
    assert_eq!(texts(&messages), vec![answer.as_str()], "{name}");
    assert_eq!(
        messages[0]["arriving"], false,
        "{name}: the turn has a result"
    );
    case
}

fn fence_straddles_size_cut() -> Case {
    let name = "fence-straddles-size-cut";
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "Show the log.");
    let mut fence = String::from("```text\n");
    let mut line = 0;
    while fence.len() < COALESCE_FLUSH_BYTES + 512 {
        fence.push_str(&format!("log line {line:05}: nothing to see here\n"));
        line += 1;
    }
    fence.push_str("```\n\n");
    let answer = [
        paragraph("Here is the log", MIN_PARAGRAPH_FLUSH_BYTES),
        fence,
        "That is the whole log.".to_owned(),
    ]
    .concat();
    stream(&mut at, &mut tr, "turn-1", &answer);
    let items = tr.end_turn(result());
    at.publish(Some("turn-1"), items);
    let case = Case {
        name,
        source: "translator",
        description: "A code fence longer than the 24 KiB size cut is split inside the fence; the pieces still join into one message with every byte in place.",
        input: at.envelopes,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    let cut_inside_fence = case.input.iter().any(|e| {
        item_kind(e) == "assistant_text"
            && content(e)["item"]["text"]
                .as_str()
                .unwrap_or("")
                .matches("```")
                .count()
                == 1
    });
    assert!(
        cut_inside_fence,
        "{name}: some piece must end or start inside the fence"
    );
    let messages = messages_of(&case);
    assert_eq!(texts(&messages), vec![answer.as_str()], "{name}");
    case
}

/// Thinking, prose, a tool, prose — produced by the translator. Shared by
/// the in-order and the out-of-order case.
fn prose_tool_prose_input(name: &'static str) -> (Vec<Value>, String, String) {
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "Run the tests.");
    let items = tr.on_update(&thought("I should run the tests first."));
    at.publish(Some("turn-1"), items);
    let before = [
        paragraph("Running the tests", MIN_PARAGRAPH_FLUSH_BYTES),
        "Starting now.".to_owned(),
    ]
    .concat();
    stream(&mut at, &mut tr, "turn-1", &before);
    let items = tr.on_update(&tool_call("call-1"));
    at.publish(Some("turn-1"), items);
    let items = tr.on_update(&tool_done("call-1"));
    at.publish(Some("turn-1"), items);
    let after = [
        paragraph("All tests pass", MIN_PARAGRAPH_FLUSH_BYTES),
        "Nothing else changed.".to_owned(),
    ]
    .concat();
    stream(&mut at, &mut tr, "turn-1", &after);
    let items = tr.end_turn(result());
    at.publish(Some("turn-1"), items);
    (at.envelopes, before, after)
}

fn prose_tool_prose() -> Case {
    let name = "prose-tool-prose";
    let (input, before, after) = prose_tool_prose_input(name);
    let case = Case {
        name,
        source: "translator",
        description: "A tool call and its result between two runs of prose end the first message: a reasoning message and two prose messages, never one.",
        input,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    assert!(
        pieces(&case, "assistant_text") >= 4,
        "{name}: paragraphs on both sides"
    );
    let messages = messages_of(&case);
    assert_eq!(
        texts(&messages),
        vec![
            "I should run the tests first.",
            before.as_str(),
            after.as_str()
        ],
        "{name}"
    );
    assert_eq!(messages[0]["kind"], "reasoning", "{name}");
    case
}

fn out_of_order_arrival() -> Case {
    let name = "out-of-order-arrival";
    let (mut input, before, after) = prose_tool_prose_input(name);
    // Deterministic shuffle: the relay delivers by arrival, not by eventSeq.
    input.reverse();
    let len = input.len();
    input.swap(0, len / 2);
    let case = Case {
        name,
        source: "translator",
        description: "The prose-tool-prose stream delivered out of order: a reader sorts by eventSeq within the exact target before joining, and gets the same messages.",
        input,
        session_status: Vec::new(),
        session_lease: Vec::new(),
    };
    let messages = messages_of(&case);
    assert_eq!(
        texts(&messages)[1..],
        [before.as_str(), after.as_str()],
        "{name}"
    );
    case
}

fn open_turn() -> Case {
    let name = "open-turn-arriving";
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "First question.");
    stream(&mut at, &mut tr, "turn-1", "A short, finished answer.");
    let items = tr.end_turn(result());
    at.publish(Some("turn-1"), items);
    begin(&mut at, &mut tr, "turn-2", "Second question.");
    let so_far = [
        paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES),
        paragraph("two", MIN_PARAGRAPH_FLUSH_BYTES),
    ]
    .concat();
    // The third paragraph is still in the translator's buffer: unpublished.
    stream(
        &mut at,
        &mut tr,
        "turn-2",
        &format!("{so_far}three, still being writ"),
    );
    let case = Case {
        name,
        source: "translator",
        description: "A turn with no result yet, in a target whose latest metadata status is running and whose kind:24223 lease the reader holds live: its trailing joined message is arriving; the finished turn before it is not.",
        session_status: vec![status(&at, "running")],
        session_lease: vec![lease(&at, "live")],
        input: at.envelopes,
    };
    let messages = messages_of(&case);
    assert_eq!(
        texts(&messages),
        vec!["A short, finished answer.", so_far.as_str()],
        "{name}"
    );
    assert_eq!(messages[0]["arriving"], false, "{name}");
    assert_eq!(messages[1]["arriving"], true, "{name}");
    case
}

fn abandoned_generation() -> Case {
    let name = "abandoned-generation-not-arriving";
    let mut first = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut first, &mut tr, "turn-1", "Answer at length.");
    let so_far = [
        paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES),
        paragraph("two", MIN_PARAGRAPH_FLUSH_BYTES),
    ]
    .concat();
    // The provider dies mid-answer: generation 1 never publishes again, so its
    // open turn never gets a `result` or `interrupted` item.
    stream(
        &mut first,
        &mut tr,
        "turn-1",
        &format!("{so_far}three, cut off by the cra"),
    );
    let alone = reference_join(&first.envelopes, &[], &[lease(&first, "live")]).0;
    assert_eq!(
        alone[0]["arriving"], true,
        "{name}: without the resume, the open turn reads as arriving"
    );
    // The session resumes as generation 2 and takes a prompt.
    let mut second = Target::new(name, SIGNER, 2);
    let mut resumed = translator();
    begin(&mut second, &mut resumed, "turn-2", "Carry on.");
    let first_lease = lease(&first, "live");
    let mut input = first.envelopes;
    input.extend(second.envelopes);
    let case = Case {
        name,
        source: "translator",
        description: "Generation 1 dies mid-answer and never ends its turn; generation 2 of the same signer, driver, instance and session publishes a prompt. A higher generation with any item supersedes the lower one: its trailing message is not arriving, even while the reader still holds generation 1's last live lease.",
        session_lease: vec![first_lease],
        input,
        session_status: Vec::new(),
    };
    let messages = messages_of(&case);
    assert_eq!(texts(&messages), vec![so_far.as_str()], "{name}");
    assert_eq!(messages[0]["arriving"], false, "{name}: superseded");
    case
}

fn terminal_status() -> Case {
    let name = "terminal-status-not-arriving";
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "Answer at length.");
    let so_far = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    stream(
        &mut at,
        &mut tr,
        "turn-1",
        &format!("{so_far}two, never finis"),
    );
    let live = [lease(&at, "live")];
    let running = reference_join(&at.envelopes, &[status(&at, "running")], &live).0;
    assert_eq!(
        running[0]["arriving"], true,
        "{name}: running keeps it arriving"
    );
    for terminal in reference::TERMINAL_STATUSES {
        let ended = reference_join(&at.envelopes, &[status(&at, terminal)], &live).0;
        assert_eq!(ended[0]["arriving"], false, "{name}: {terminal}");
    }
    for turn_only in reference::TURN_ONLY_STATUSES {
        let open = reference_join(&at.envelopes, &[status(&at, turn_only)], &live).0;
        assert_eq!(
            open[0]["arriving"], true,
            "{name}: {turn_only} ends a turn, not the session"
        );
    }
    let case = Case {
        name,
        source: "translator",
        description: "An open turn with no result, in a target whose latest metadata status is stopped: the session has ended, so its trailing message is not arriving, even while the reader still holds a live lease. Only the session-terminal statuses (completed, stopped, disconnected) do the same. interrupted and failed do not: the producer publishes them at the end of an ordinary turn while the session goes on, and that turn's own result or interrupted item already ends its message.",
        session_status: vec![status(&at, "stopped")],
        session_lease: vec![lease(&at, "live")],
        input: at.envelopes,
    };
    let messages = messages_of(&case);
    assert_eq!(texts(&messages), vec![so_far.as_str()], "{name}");
    assert_eq!(messages[0]["arriving"], false, "{name}");
    case
}

fn lapsed_lease() -> Case {
    let name = "lapsed-lease-not-arriving";
    let mut at = Target::new(name, SIGNER, 1);
    let mut tr = translator();
    begin(&mut at, &mut tr, "turn-1", "Answer at length.");
    let so_far = paragraph("one", MIN_PARAGRAPH_FLUSH_BYTES);
    // The provider's machine sleeps mid-answer. Nothing republishes a status:
    // the latest metadata stays `running`, no generation 2 ever appears, and
    // the lease the provider renewed every 60 seconds runs out.
    stream(
        &mut at,
        &mut tr,
        "turn-1",
        &format!("{so_far}two, the lid clo"),
    );
    let running = [status(&at, "running")];
    let live = reference_join(&at.envelopes, &running, &[lease(&at, "live")]).0;
    assert_eq!(
        live[0]["arriving"], true,
        "{name}: a live lease keeps it arriving"
    );
    for gone in ["lapsed", "released"] {
        let ended = reference_join(&at.envelopes, &running, &[lease(&at, gone)]).0;
        assert_eq!(ended[0]["arriving"], false, "{name}: {gone}");
    }
    let none = reference_join(&at.envelopes, &running, &[]).0;
    assert_eq!(none[0]["arriving"], false, "{name}: no lease held");
    let case = Case {
        name,
        source: "translator",
        description: "An open turn with no result, latest metadata status running, no higher generation — and the reader's kind:24223 lease for the target has lapsed: the provider is not proven reachable (its machine slept, lost power or lost the network), so its trailing message is not arriving. A released lease, or no lease the reader holds at all, does the same; only an unexpired live lease keeps it arriving.",
        session_status: running.to_vec(),
        session_lease: vec![lease(&at, "lapsed")],
        input: at.envelopes,
    };
    let messages = messages_of(&case);
    assert_eq!(texts(&messages), vec![so_far.as_str()], "{name}");
    assert_eq!(messages[0]["arriving"], false, "{name}");
    case
}

fn cases() -> Vec<Case> {
    vec![
        three_paragraphs(),
        fence_straddles_size_cut(),
        prose_tool_prose(),
        own_prose_around_subagent_prose(),
        generations_share_turn_id(),
        distinct_message_ids(),
        out_of_order_arrival(),
        open_turn(),
        abandoned_generation(),
        terminal_status(),
        lapsed_lease(),
        two_signers(),
        reasoning_joins_like_prose(),
        sequence_gap(),
        duplicate_delivery(),
    ]
}

fn vectors() -> Value {
    let vectors: Vec<Value> = cases()
        .into_iter()
        .map(|case| {
            let (messages, brief) =
                reference_join(&case.input, &case.session_status, &case.session_lease);
            json!({
                "name": case.name,
                "source": case.source,
                "description": case.description,
                "input": case.input,
                "sessionStatus": case.session_status,
                "sessionLease": case.session_lease,
                "expectedMessages": messages,
                "expectedBrief": brief,
            })
        })
        .collect();
    json!({
        "schema": VECTOR_SCHEMA,
        "contract": "conformance/transcript-prose-join/CONTRACT.md",
        "generator": {
            "test": "crates/buzz-session-provider/src/transcript_prose_join_vectors_tests.rs",
            "regenerate": format!("{REGEN_ENV}=1 cargo test -p buzz-session-provider transcript_prose_join"),
            "translator": "TranscriptTranslator::new(true).with_paragraph_flush(true)",
            "minParagraphFlushBytes": MIN_PARAGRAPH_FLUSH_BYTES,
            "coalesceFlushBytes": COALESCE_FLUSH_BYTES,
        },
        "vectors": vectors,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// The checked-in vectors are exactly what the translator and the join
/// produce today. Fails on any drift; regenerate only on purpose.
#[test]
fn transcript_prose_join_vectors_match_the_translator() {
    let generated = vectors();
    let path = fixture_path();
    if std::env::var(REGEN_ENV).as_deref() == Ok("1") {
        std::fs::create_dir_all(path.parent().expect("fixture dir")).expect("create fixture dir");
        std::fs::write(&path, canonical(&generated)).expect("write vectors");
        return;
    }
    let on_disk = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}; run with {REGEN_ENV}=1", path.display()));
    let parsed: Value = serde_json::from_str(&on_disk).expect("vectors.json parses");
    let empty = Vec::new();
    let disk_vectors = parsed["vectors"].as_array().unwrap_or(&empty);
    let new_vectors = generated["vectors"].as_array().unwrap_or(&empty);
    for (disk, new) in disk_vectors.iter().zip(new_vectors) {
        assert_eq!(
            disk, new,
            "vector {} drifted from the translator; if intended, run with {REGEN_ENV}=1 and re-run every reader's binding test",
            new["name"]
        );
    }
    assert_eq!(
        parsed, generated,
        "vectors.json drifted; run with {REGEN_ENV}=1"
    );
    assert_eq!(
        on_disk,
        canonical(&generated),
        "vectors.json is not in canonical form"
    );
}

/// Every required case is present, and the file binds the contract's shape:
/// each expected message has exactly the fields a reader compares.
#[test]
fn transcript_prose_join_vectors_cover_every_required_case() {
    let generated = vectors();
    let names: Vec<&str> = generated["vectors"]
        .as_array()
        .map(|vectors| vectors.iter().filter_map(|v| v["name"].as_str()).collect())
        .unwrap_or_default();
    for required in [
        "three-paragraphs-one-turn",
        "fence-straddles-size-cut",
        "prose-tool-prose",
        "own-prose-around-subagent-prose",
        "two-generations-share-turn-id",
        "distinct-message-ids",
        "out-of-order-arrival",
        "open-turn-arriving",
        "abandoned-generation-not-arriving",
        "terminal-status-not-arriving",
        "lapsed-lease-not-arriving",
        "duplicate-delivery-is-one-piece",
    ] {
        assert!(names.contains(&required), "missing vector {required}");
    }
    for vector in generated["vectors"].as_array().into_iter().flatten() {
        for message in vector["expectedMessages"].as_array().into_iter().flatten() {
            let mut keys: Vec<&str> = message
                .as_object()
                .map(|object| object.keys().map(String::as_str).collect())
                .unwrap_or_default();
            keys.sort_unstable();
            assert_eq!(
                keys,
                vec![
                    "arriving",
                    "firstEventId",
                    "kind",
                    "lastEventId",
                    "parentToolId",
                    "signer",
                    "target",
                    "text",
                    "turnId",
                ],
                "{}",
                vector["name"]
            );
        }
    }
}

/// Joining moves no byte: per exact target, the joined texts concatenate to
/// the distinct pieces' texts (one per event id) in eventSeq order.
#[test]
fn transcript_prose_join_moves_no_byte() {
    for case in cases() {
        let (messages, _) = reference_join(&case.input, &case.session_status, &case.session_lease);
        let mut sorted: Vec<&Value> = case.input.iter().collect();
        sorted.sort_by_key(|e| (group_key(e), content(e)["eventSeq"].as_u64()));
        // One event, one piece: a repeated delivery contributes its bytes once.
        sorted.dedup_by(|a, b| group_key(a) == group_key(b) && a["eventId"] == b["eventId"]);
        let pieces: String = sorted
            .iter()
            .filter(|e| matches!(item_kind(e), "assistant_text" | "reasoning"))
            .filter_map(|e| content(e)["item"]["text"].as_str())
            .collect();
        let joined: String = texts(&messages).concat();
        assert_eq!(joined, pieces, "{}", case.name);
    }
}
