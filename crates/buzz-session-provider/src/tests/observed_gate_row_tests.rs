//! The provider publishes a gate row nobody asked it for.
//!
//! Brian's 2026-09-02 ruling, end to end: a seat runs a gate, the provider
//! watches the two transcript frames it was already publishing, and a signed
//! kind 44246 row goes on the wire under the **provider's** key with
//! `source: "observed"`. The seat is not consulted, cannot suppress it, and
//! cannot sign it.
//!
//! Live-run finding 26 is what this closes: a seat's prose "`cargo test -p
//! buzz-cli` green" against a verifier reproducing red on the same patch. One
//! of the two was wrong, and nothing on the wire could say which.

use super::*;

use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationGateOutcome,
    CodingSessionObservationSource,
};
use buzz_sdk::coding_session_observation::parse_coding_session_observation;

fn tool_call(tool_id: &str, command: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "input": { "command": command },
        },
    })
}

fn tool_result(tool_id: &str, is_error: bool, content: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_result",
        "toolId": tool_id,
        "toolName": "Bash",
        "content": content,
        "isError": is_error,
    })
}

/// One provider holding one governed session, plus that session's id.
fn observed_fixture(dir: &tempfile::TempDir) -> (Provider, Uuid, String) {
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    let record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).expect("insert");
    (provider, channel_id, session_id)
}

fn feed(provider: &mut Provider, session_id: &str, items: Vec<serde_json::Value>) {
    provider
        .handle_session_event(session::SessionEvent::TranscriptItems {
            session_id: session_id.to_owned(),
            turn_id: "turn-1".to_owned(),
            items,
        })
        .expect("transcript items");
}

#[tokio::test]
async fn a_seats_failing_cargo_test_publishes_an_observed_gate_row_it_never_asked_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, session_id) = observed_fixture(&dir);

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo test -p buzz-cli subcommand_"),
            tool_result(
                "t1",
                true,
                "test result: FAILED. 0 passed; 2 failed; 0 ignored",
            ),
        ],
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let observations: Vec<Event> = sink
        .all()
        .into_iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_OBSERVATION)
        .collect();
    assert_eq!(observations.len(), 1, "exactly one gate row for one gate");

    let event = &observations[0];
    assert_eq!(
        event.pubkey.to_hex(),
        provider.config.keys.public_key().to_hex(),
        "the mechanism that watched is the author; the seat cannot sign this"
    );
    let payload = parse_coding_session_observation(event).expect("a valid 44246 envelope");
    assert_eq!(payload.source, CodingSessionObservationSource::Observed);
    assert_eq!(payload.session_ref, "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
    assert_eq!(payload.genesis_ref, "ab".repeat(32));
    assert_eq!(
        payload.assignment_ref, None,
        "the provider watched a command, not an assignment"
    );
    let CodingSessionObservationBody::Gate(gate) = payload.body else {
        panic!("a gate observation carries a gate body");
    };
    assert_eq!(gate.rows.len(), 1);
    assert_eq!(gate.rows[0].gate, "cargo test");
    assert_eq!(
        gate.rows[0].outcome,
        CodingSessionObservationGateOutcome::Failed
    );
    assert_eq!(gate.rows[0].command, "cargo test -p buzz-cli subcommand_");
    assert_eq!(
        gate.rows[0].summary.as_deref(),
        Some("test result: FAILED. 0 passed; 2 failed; 0 ignored")
    );
    // The envelope is filed under the channel the session publishes into.
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert_eq!(tags[0], ["h".to_owned(), channel_id.to_string()]);
}

#[tokio::test]
async fn ordinary_work_publishes_no_gate_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir);

    feed(
        &mut provider,
        &session_id,
        vec![
            serde_json::json!({ "kind": "assistant_text", "text": "cargo test passed, honest" }),
            tool_call("t1", "git status --porcelain"),
            tool_result("t1", false, ""),
        ],
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        sink.all()
            .iter()
            .all(|event| u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_OBSERVATION),
        "prose claiming a gate is not a gate row, and `git status` is not a gate"
    );
}

#[tokio::test]
async fn a_session_with_no_umbrella_publishes_nothing_rather_than_inventing_a_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    // A solo session: no umbrella, so no `sessionRef` and no `genesisRef` for a
    // 44246 to be scoped to.
    record.session_ref = None;
    record.genesis_ref = None;
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).expect("insert");

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo fmt --check"),
            tool_result("t1", false, ""),
        ],
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        sink.all()
            .iter()
            .all(|event| u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_OBSERVATION),
        "a row filed under an umbrella this session is not part of would be a lie"
    );
    // The gate still ran, and the transcript still says so.
    assert!(!sink.contents_of(KIND_CODING_SESSION_TRANSCRIPT).is_empty());
}

#[tokio::test]
async fn a_lost_mailbox_forgets_the_half_paired_call_rather_than_resolving_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir);

    feed(
        &mut provider,
        &session_id,
        vec![tool_call("t1", "cargo clippy --all-targets -- -D warnings")],
    );
    provider
        .handle_session_event(session::SessionEvent::Exited {
            session_id: session_id.clone(),
            reason: session::ExitReason::Requested,
        })
        .expect("exit");
    assert!(
        !provider.gate_observers.contains_key(&session_id),
        "a call whose result this process will never see is not a gate anybody ran"
    );
}
