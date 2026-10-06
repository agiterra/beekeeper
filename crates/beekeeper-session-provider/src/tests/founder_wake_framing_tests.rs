//! A founder-run provider's wake pointers used to arrive naked (COMMS-MAP
//! finding 3).
//!
//! `turn_framing` returned `None` for every founder-signed command, so a lead
//! whose provider runs under the founder's own key received exactly the 102
//! bytes of `{"operationId":"…","type":"…"}` — no scope, no sender, no reply
//! address — while the identical pointer from a peer seat or a granted
//! operator arrived inside a `[Context]` block. Same fact, two shapes,
//! decided by an accident of who happened to hold the provider's key.
//!
//! These tests pin the fix and its two edges: prose from the founder is still
//! unframed, and the byte-exact operation fence still keys on the bare
//! pointer, not on the delivered envelope.

use super::*;

use crate::team_wake;

/// The exact identifier-only report pointer `team_wake::wake_text` mints.
fn report_pointer() -> String {
    serde_json::json!({"operationId": "ab".repeat(32), "type": "assignment_report"}).to_string()
}

/// The rendered envelope with its `From:` line removed — everything a wake's
/// recipient is told apart from who sent it.
fn without_the_from_line(rendered: &str) -> String {
    rendered
        .lines()
        .filter(|line| !line.starts_with("From: "))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One provider, one governed session, and the founder that record names.
fn framing_fixture(dir: &tempfile::TempDir) -> (Provider, Uuid, String, String) {
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    let founder = test_operator_keys().public_key().to_hex();
    let addressed = governed_record(channel_id, &cwd, &"ab".repeat(32));
    let addressed_id = addressed.session_id.clone();
    provider
        .state
        .insert_session(addressed)
        .expect("insert addressed");
    (provider, channel_id, addressed_id, founder)
}

/// The whole finding: one pointer, two signers, one envelope.
///
/// The peer here holds no seat in this umbrella, so both sides get the same
/// honest "no live seat" reply line — which is exactly what makes the
/// comparison meaningful: the two envelopes may differ *only* by who sent the
/// wake.
#[test]
fn a_founder_provider_wake_is_framed_exactly_like_a_peer_wake() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (provider, channel_id, addressed_id, founder) = framing_fixture(&dir);
    let peer = "cd".repeat(32);
    let pointer = report_pointer();

    let founder_frame = provider
        .turn_framing(
            &addressed_id,
            &founder,
            CodingSessionDelivery::Boundary,
            channel_id,
            &pointer,
        )
        .expect("a team-wake pointer from a founder-run provider is framed");
    let peer_frame = provider
        .turn_framing(
            &addressed_id,
            &peer,
            CodingSessionDelivery::Boundary,
            channel_id,
            &pointer,
        )
        .expect("a peer's team-wake pointer is framed");

    let founder_rendered = founder_frame.render(&pointer);
    let peer_rendered = peer_frame.render(&pointer);
    assert_eq!(
        without_the_from_line(&founder_rendered),
        without_the_from_line(&peer_rendered),
        "the same pointer must produce the same envelope regardless of who ran the provider"
    );
    assert!(
        founder_rendered.contains(&format!("From: {founder} (provider)")),
        "{founder_rendered}"
    );
    assert!(
        peer_rendered.contains(&format!("From: {peer} (operator)")),
        "{peer_rendered}"
    );
    assert!(
        founder_rendered.contains("Reply: "),
        "a framed wake always carries a reply address or says it has none: {founder_rendered}"
    );
}

/// Prose the founder typed is still their own words, delivered bare.
#[test]
fn a_founder_typed_prose_turn_is_still_unframed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (provider, channel_id, addressed_id, founder) = framing_fixture(&dir);

    assert_eq!(
        provider.turn_framing(
            &addressed_id,
            &founder,
            CodingSessionDelivery::Boundary,
            channel_id,
            "ship it",
        ),
        None,
        "the founder's own words are never framed as somebody else's message"
    );
    // JSON that is not one of the two exact wake shapes is prose too.
    assert_eq!(
        provider.turn_framing(
            &addressed_id,
            &founder,
            CodingSessionDelivery::Boundary,
            channel_id,
            r#"{"operationId":"not-an-event-id","type":"assignment_report"}"#,
        ),
        None,
        "only the two shapes wake_text mints are wakes"
    );
}

/// The fence key is computed from the bare pointer, and framing never touches
/// it. A fence that keyed on the delivered envelope would stop de-duplicating
/// the two producers' byte-identical pointers the moment one of them was
/// framed.
#[test]
fn the_operation_fence_still_keys_on_the_bare_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (provider, channel_id, addressed_id, founder) = framing_fixture(&dir);
    let pointer = report_pointer();
    let target = provider
        .state
        .session(&addressed_id)
        .map(|record| record.target(&provider.config.instance_id))
        .expect("target");

    let bare = team_wake::operation_fence_key(&target, &pointer).expect("the pointer is fenced");
    let framed = provider
        .turn_framing(
            &addressed_id,
            &founder,
            CodingSessionDelivery::Boundary,
            channel_id,
            &pointer,
        )
        .expect("framed")
        .render(&pointer);
    // Keyed on the fact the bare pointer names (ledger 266), which only the
    // bare pointer carries in a parseable form.
    let fact_id = serde_json::from_str::<serde_json::Value>(&pointer).expect("pointer json")
        ["operationId"]
        .as_str()
        .expect("operation id")
        .to_owned();
    assert!(bare.ends_with(&format!("fact:{fact_id}")), "{bare}");
    assert_eq!(
        team_wake::operation_fence_key(&target, &framed),
        None,
        "the envelope is not the pointer, and the fence must keep reading the pointer"
    );
}
