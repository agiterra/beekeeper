//! The wire shape of `bee sessions close`, and the genesis it names.
//!
//! The event is asserted against the builder rather than against a hand-written
//! copy of the tags: the point of routing through
//! [`build_coding_session_closure`] is that there is one shape, so the test
//! that matters is that this command's payload is the payload the desktop
//! dialog's is.

use super::*;

fn genesis(id: &str, session_ref: &str) -> Value {
    json!({
        "id": id,
        "kind": KIND_CODING_SESSION_GENESIS,
        "content": json!({ "sessionRef": session_ref, "v": 1 }).to_string(),
        "tags": [["csg-session", session_ref]],
    })
}

const SESSION: &str = "cc5cb114-0000-4000-8000-000000000001";
const GENESIS_ID: &str = "aa11bb22cc33dd44ee55ff6600112233445566778899aabbccddeeff00112233";

#[test]
fn the_genesis_is_selected_by_its_content_session_ref() {
    let other = "11111111-2222-4333-8444-555555555555";
    let events = vec![genesis("dead", other), genesis(GENESIS_ID, SESSION)];
    assert_eq!(find_genesis(&events, SESSION), Some(GENESIS_ID));
    assert_eq!(find_genesis(&events, "not-a-session"), None);
}

#[test]
fn a_csg_session_tag_alone_never_selects_a_genesis() {
    // NIP-CSG: the tag exists for the relay's uniqueness probe; consumers must
    // not select by it. A genesis whose content names another session is
    // another session's, whatever its tag says.
    let events = vec![json!({
        "id": GENESIS_ID,
        "kind": KIND_CODING_SESSION_GENESIS,
        "content": json!({ "sessionRef": "11111111-2222-4333-8444-555555555555", "v": 1 })
            .to_string(),
        "tags": [["csg-session", SESSION]],
    })];
    assert_eq!(find_genesis(&events, SESSION), None);
}

#[test]
fn a_non_genesis_event_is_never_mistaken_for_one() {
    let events = vec![json!({
        "id": GENESIS_ID,
        "kind": 44230,
        "content": json!({ "sessionRef": SESSION, "v": 1 }).to_string(),
        "tags": [["d", SESSION]],
    })];
    assert_eq!(find_genesis(&events, SESSION), None);
}

#[test]
fn every_action_this_command_accepts_round_trips_to_the_payload() {
    assert_eq!(
        parse_action("closed").expect("closed"),
        CodingSessionClosureAction::Closed
    );
    assert_eq!(
        parse_action("archived").expect("archived"),
        CodingSessionClosureAction::Archived
    );
    assert_eq!(
        parse_action("open").expect("open"),
        CodingSessionClosureAction::Open
    );
    let refusal = parse_action("deleted").expect_err("a deletion is not a closure");
    assert!(
        refusal.to_string().contains("closed, archived or open"),
        "the refusal names the accepted words: {refusal}"
    );
}

#[test]
fn the_published_event_is_the_dialog_s_closure_exactly() {
    let channel = Uuid::parse_str("6620be79-0000-4000-8000-000000000002").expect("channel");
    let payload =
        CodingSessionClosurePayload::new(CodingSessionClosureAction::Closed, GENESIS_ID, SESSION);
    payload.validate().expect("the payload the relay validates");
    let builder = build_coding_session_closure(channel, &payload).expect("builder");
    let event = builder.build(nostr::Keys::generate().public_key());

    assert_eq!(event.kind.as_u16(), 44230);
    let tags: Vec<Vec<String>> = event.tags.iter().map(|tag| tag.clone().to_vec()).collect();
    assert_eq!(
        tags,
        vec![
            vec!["h".to_string(), channel.to_string()],
            vec!["d".to_string(), SESSION.to_string()],
            vec!["cscl-v".to_string(), "cscl1-1".to_string()],
            vec!["cscl-genesis".to_string(), GENESIS_ID.to_string()],
        ]
    );
    let content: Value = serde_json::from_str(&event.content).expect("content is JSON");
    assert_eq!(
        content,
        json!({
            "action": "closed",
            "genesisRef": GENESIS_ID,
            "sessionRef": SESSION,
            "v": 1,
        })
    );
}

#[test]
fn a_reopen_carries_the_same_shape_with_the_open_action() {
    let channel = Uuid::parse_str("6620be79-0000-4000-8000-000000000002").expect("channel");
    let payload =
        CodingSessionClosurePayload::new(CodingSessionClosureAction::Open, GENESIS_ID, SESSION);
    let builder = build_coding_session_closure(channel, &payload).expect("builder");
    let event = builder.build(nostr::Keys::generate().public_key());
    let content: Value = serde_json::from_str(&event.content).expect("content is JSON");
    assert_eq!(content.get("action").and_then(Value::as_str), Some("open"));
}
