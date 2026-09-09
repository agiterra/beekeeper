//! What the seat-request ledger derives, and what it deliberately omits.

use super::*;

use buzz_core::coding_session_authority_claim::ClaimState;
use std::collections::BTreeSet;
use std::path::PathBuf;

use uuid::Uuid;

/// This provider's own authority pubkey, as the fence asks about it.
const TEST_PROVIDER: &str = "99";

/// A minimal open session record. Every test below varies only the fields it
/// is actually about.
fn record(session_id: &str, command_id: &str) -> SessionRecord {
    SessionRecord {
        session_id: session_id.to_owned(),
        generation: 1,
        channel_id: Uuid::nil(),
        command_id: command_id.to_owned(),
        generation_command_id: Some(command_id.to_owned()),
        provider_instance_ref: "claude-primary".into(),
        runtime: "claude".into(),
        driver: "claude".into(),
        cwd: PathBuf::from("/tmp/checkout"),
        project_ref: None,
        repo_ref: None,
        session_ref: None,
        genesis_ref: None,
        actor: None,
        role: None,
        pack_ref: None,
        founder_pubkey: None,
        granted_operators: BTreeSet::new(),
        granted_viewers: BTreeSet::new(),
        authority_seq: 0,
        model: None,
        routing: None,
        resume_cursor: Some("saved-acp-session".into()),
        title: None,
        created_at_ms: 0,
        next_seq: 1,
        next_lease_sequence: 1,
        bootstrap_transport: None,
        open_turn: None,
        closed: false,
        handover: ClaimState::NoClaim,
        retired: None,
    }
}

fn seated(session_id: &str, command_id: &str, actor: &str) -> SessionRecord {
    let mut record = record(session_id, command_id);
    record.actor = Some(actor.to_owned());
    record.role = Some("builder".into());
    record.project_ref = Some(format!("30621:{}:beekeeper", "11".repeat(32)));
    record
}

/// The key the desktop files custody under is the *generation's* command id,
/// not the create's: after a resume those differ, and staging under the create
/// would leave the restore path finding nothing.
#[test]
fn a_resumed_generation_asks_for_custody_under_its_own_command_id() {
    let mut seat = seated("session-1", "create-1", &"aa".repeat(32));
    seat.generation = 3;
    seat.generation_command_id = Some("resume-2".into());

    let rows = derive_seat_requests([&seat], TEST_PROVIDER);

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].command_id, "resume-2");
    assert_eq!(rows[0].generation, 3);
    assert_eq!(rows[0].session_id, "session-1");
}

/// A record written before generation commands were persisted names its
/// generation with the create's command id, and that is what custody keys on.
#[test]
fn a_record_without_a_generation_command_falls_back_to_the_create() {
    let mut seat = seated("session-1", "create-1", &"aa".repeat(32));
    seat.generation_command_id = None;

    let rows = derive_seat_requests([&seat], TEST_PROVIDER);

    assert_eq!(rows[0].command_id, "create-1");
}

/// Only open, seated generations are asked for. A stopped one must not have a
/// key re-staged for it, and an unseated one has no key at all.
#[test]
fn closed_and_unseated_generations_ask_for_nothing() {
    let unseated = record("session-human", "create-h");
    let mut stopped = seated("session-stopped", "create-s", &"bb".repeat(32));
    stopped.closed = true;
    let open = seated("session-open", "create-o", &"cc".repeat(32));

    let rows = derive_seat_requests([&unseated, &stopped, &open], TEST_PROVIDER);

    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].session_id, "session-open");
}

/// The exact key set the desktop reader parses, and nothing that could carry a
/// secret.
#[test]
fn a_row_carries_the_public_facts_only() {
    let seat = seated("session-1", "create-1", &"aa".repeat(32));
    let dir = tempfile::tempdir().expect("tempdir");

    write_seat_requests(dir.path(), [&seat], TEST_PROVIDER).expect("write");

    let body = std::fs::read_to_string(seat_requests_path(dir.path())).expect("read");
    let parsed: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(parsed["version"], 1);
    assert_eq!(
        parsed["requests"][0],
        serde_json::json!({
            "commandId": "create-1",
            "actor": "aa".repeat(32),
            "role": "builder",
            "projectRef": format!("30621:{}:beekeeper", "11".repeat(32)),
            "sessionId": "session-1",
            "generation": 1,
        }),
        "{body}"
    );
    // The custody file's own fields must never appear here.
    for forbidden in ["nsec", "authTag", "cwd", "packDir", "relayUrl"] {
        assert!(!body.contains(forbidden), "{forbidden} leaked into {body}");
    }
}

/// A rewrite replaces the whole set: a generation that stopped stops asking.
#[test]
fn a_rewrite_drops_the_rows_that_no_longer_apply() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = seated("session-1", "create-1", &"aa".repeat(32));
    let second = seated("session-2", "create-2", &"bb".repeat(32));

    write_seat_requests(dir.path(), [&first, &second], TEST_PROVIDER).expect("write both");
    write_seat_requests(dir.path(), [&second], TEST_PROVIDER).expect("rewrite");

    let file: SeatRequestsFile = serde_json::from_str(
        &std::fs::read_to_string(seat_requests_path(dir.path())).expect("read"),
    )
    .expect("parse");
    assert_eq!(file.requests.len(), 1);
    assert_eq!(file.requests[0].session_id, "session-2");
}

/// Nothing seated is still an answer, not an absent file: the desktop has to
/// be able to tell "no custody is needed" from "the provider never wrote".
#[test]
fn nothing_seated_writes_an_empty_request_list() {
    let dir = tempfile::tempdir().expect("tempdir");

    write_seat_requests(
        dir.path(),
        [&record("session-human", "create-h")],
        TEST_PROVIDER,
    )
    .expect("write");

    let file: SeatRequestsFile = serde_json::from_str(
        &std::fs::read_to_string(seat_requests_path(dir.path())).expect("read"),
    )
    .expect("parse");
    assert_eq!(file.version, SEAT_REQUESTS_VERSION);
    assert!(file.requests.is_empty(), "{file:?}");
}
