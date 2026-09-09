//! A hired seat's brief used to arrive unattributed (COMMS-MAP finding 1).
//!
//! The create's `initial_turn` was delivered with `framing: None` and
//! `operator_pubkey = founder`, so the signed `user_prompt` named the founder
//! as the person who wrote a brief the *lead* wrote, and the model reading it
//! got no `[Context]` block, no sender and no reply address. The only trace of
//! the lead was a 16-byte `"[From the lead] "` prefix added in TypeScript.
//!
//! Kind 44221 now carries `hireRef` on a create and `requestedBy` on a hire.
//! These tests pin all three outcomes of reading them, because the middle one
//! is the honest part: **the relay does not verify `requestedBy` against the
//! hire's signer** (POLICY.md §5), so a claim that disagrees with its signer is
//! disputed — disclosed out loud, and never rendered as attribution.

use super::*;

use crate::commands::{hire_attribution, HireAttribution};
use buzz_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command;

/// The umbrella every fixture in this file shares.
const UMBRELLA: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/// A signed kind:44221 `session.hire`, with whatever `requestedBy` the caller
/// wants to put on the wire beside whichever key actually signs it.
fn hire_event(
    channel_id: Uuid,
    genesis_ref: &str,
    requested_by: Option<&str>,
    signer: &Keys,
) -> Event {
    let mut action = serde_json::json!({
        "type": "session.hire",
        "sessionRef": UMBRELLA,
        "genesisRef": genesis_ref,
        "role": "builder",
        "providerInstanceRef": null,
        "model": null,
        "brief": "land lane B2",
    });
    if let (Some(object), Some(requested_by)) = (action.as_object_mut(), requested_by) {
        object.insert("requestedBy".into(), serde_json::json!(requested_by));
    }
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": "hire-1",
        "action": action,
    })
    .to_string();
    signed_lifecycle_event_by(channel_id, content, signer)
}

/// A seated-shaped create that answers a hire and carries the brief as its
/// first turn.
fn create_answering_hire(
    provider: &Provider,
    channel_id: Uuid,
    command_id: &str,
    genesis_ref: &str,
    hire_ref: Option<&str>,
) -> Event {
    let mut action = serde_json::json!({
        "type": "session.create",
        "projectRef": null,
        "repoRef": null,
        "sessionRef": UMBRELLA,
        "genesisRef": genesis_ref,
        "providerInstanceRef": "claude-primary",
        "providerAuthorityPubkey": provider.config.pubkey_hex(),
        "model": null,
        "title": "Ship it",
        "initialTurn": "land lane B2",
    });
    if let (Some(object), Some(hire_ref)) = (action.as_object_mut(), hire_ref) {
        object.insert("hireRef".into(), serde_json::json!(hire_ref));
    }
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": command_id,
        "action": action,
    })
    .to_string();
    signed_lifecycle_event(channel_id, content)
}

/// The requester's own seat, so a resolved requester has a role and a reply
/// address exactly as a live sibling would.
fn seat_the_requester(
    provider: &mut Provider,
    channel_id: Uuid,
    cwd: &Path,
    genesis_ref: &str,
    requester: &str,
) {
    let mut seat = governed_record(channel_id, cwd, genesis_ref);
    seat.actor = Some(requester.to_owned());
    seat.role = Some("lead".into());
    provider
        .state
        .insert_session(seat)
        .expect("insert requester seat");
}

/// The `user_prompt` item the create's first turn published.
fn first_prompt_item(sink: &CollectingSink) -> serde_json::Value {
    transcript_items_in_sequence(sink)
        .into_iter()
        .find(|item| item["item"]["kind"] == "user_prompt")
        .expect("user_prompt item")
}

/// Three answers, never two. Collapsing "unclaimed" into either of the others
/// is exactly how an unattributed seat comes to look attributed.
#[test]
fn a_hires_requester_claim_is_compared_with_its_signer() {
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);
    let requester = Keys::generate();
    let requester_hex = requester.public_key().to_hex();
    let stranger_hex = "cd".repeat(32);

    let attributed = hire_event(channel_id, &genesis_ref, Some(&requester_hex), &requester);
    let payload =
        decode_coding_session_lifecycle_command(&attributed.content).expect("decode attributed");
    assert_eq!(
        hire_attribution(&payload, &attributed.pubkey.to_hex()),
        HireAttribution::Attributed(requester_hex.clone())
    );

    let disputed = hire_event(channel_id, &genesis_ref, Some(&stranger_hex), &requester);
    let payload =
        decode_coding_session_lifecycle_command(&disputed.content).expect("decode disputed");
    assert_eq!(
        hire_attribution(&payload, &disputed.pubkey.to_hex()),
        HireAttribution::Disputed {
            claimed: stranger_hex,
            signer: requester_hex,
        }
    );

    let unclaimed = hire_event(channel_id, &genesis_ref, None, &requester);
    let payload =
        decode_coding_session_lifecycle_command(&unclaimed.content).expect("decode unclaimed");
    assert_eq!(
        hire_attribution(&payload, &unclaimed.pubkey.to_hex()),
        HireAttribution::Unclaimed
    );
}

/// The lane's own finding, end to end: the brief is the lead's, and the signed
/// transcript says so.
#[tokio::test]
async fn a_hired_seats_brief_is_attributed_to_the_seat_that_asked_for_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));

    let genesis = genesis_event(channel_id, UMBRELLA);
    let genesis_ref = genesis.id.to_hex();
    let requester = Keys::generate();
    let requester_hex = requester.public_key().to_hex();
    seat_the_requester(
        &mut provider,
        channel_id,
        &cwd,
        &genesis_ref,
        &requester_hex,
    );
    let hire = hire_event(channel_id, &genesis_ref, Some(&requester_hex), &requester);
    let create = create_answering_hire(
        &provider,
        channel_id,
        "create-hired",
        &genesis_ref,
        Some(&hire.id.to_hex()),
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![genesis.clone(), hire.clone()])
            .await;
    // A relay publishes its identity and a provider witnesses it; without one
    // no receipt verifies, so a genesis-bearing create is refused by name
    // rather than admitted on the assumption that nothing has been handed
    // over. Doing it here is what production does at startup.
    provider.set_rest_client(relay.rest_client());
    provider.witness_relay_identity().await;

    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    pump_until_turn_finished(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");

    let prompt = first_prompt_item(&sink);
    assert_eq!(
        prompt["item"]["operatorPubkey"], requester_hex,
        "the brief is the requesting seat's, not the founder's: {prompt}"
    );
    assert_eq!(
        prompt["item"]["senderRole"], "lead",
        "the requester's own seat supplies the role: {prompt}"
    );
    // The signed transcript keeps the brief unframed — the `[Context]` block is
    // addressing metadata for the model, not something the lead wrote.
    assert_eq!(prompt["item"]["content"], "land lane B2");

    relay.shutdown().await;
    server.abort();
}

/// A `requestedBy` that disagrees with the hire's signer is a claim anybody
/// admitted for a hire could have made. It is disclosed, and the brief falls
/// back to exactly the unattributed delivery it had before this field existed.
#[tokio::test]
async fn a_disputed_requester_is_disclosed_and_never_rendered_as_attribution() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));

    let genesis = genesis_event(channel_id, UMBRELLA);
    let genesis_ref = genesis.id.to_hex();
    let impersonated = Keys::generate();
    let impersonated_hex = impersonated.public_key().to_hex();
    seat_the_requester(
        &mut provider,
        channel_id,
        &cwd,
        &genesis_ref,
        &impersonated_hex,
    );
    // Signed by somebody else entirely, claiming the seated lead asked for it.
    let hire = hire_event(
        channel_id,
        &genesis_ref,
        Some(&impersonated_hex),
        &Keys::generate(),
    );
    let create = create_answering_hire(
        &provider,
        channel_id,
        "create-disputed",
        &genesis_ref,
        Some(&hire.id.to_hex()),
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![genesis.clone(), hire.clone()])
            .await;
    // A relay publishes its identity and a provider witnesses it; without one
    // no receipt verifies, so a genesis-bearing create is refused by name
    // rather than admitted on the assumption that nothing has been handed
    // over. Doing it here is what production does at startup.
    provider.set_rest_client(relay.rest_client());
    provider.witness_relay_identity().await;

    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    pump_until_turn_finished(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");

    let prompt = first_prompt_item(&sink);
    assert_eq!(
        prompt["item"]["operatorPubkey"],
        test_operator_keys().public_key().to_hex(),
        "a disputed claim falls back to the founder, exactly as before: {prompt}"
    );
    assert!(
        prompt["item"].get("senderRole").is_none(),
        "an unverified claim never becomes a signed role: {prompt}"
    );

    let statuses: Vec<String> = transcript_items_in_sequence(&sink)
        .into_iter()
        .filter(|item| item["item"]["kind"] == "status")
        .filter_map(|item| item["item"]["status"].as_str().map(str::to_owned))
        .collect();
    assert!(
        statuses
            .iter()
            .any(|status| status == "hire_requester_disputed"),
        "the dispute is disclosed on the wire, not swallowed: {statuses:?}"
    );

    relay.shutdown().await;
    server.abort();
}

/// A create that answers no hire is byte-for-byte the delivery it always was.
#[tokio::test]
async fn a_create_with_no_hire_ref_is_delivered_exactly_as_before() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));

    let genesis = genesis_event(channel_id, UMBRELLA);
    let create = create_answering_hire(
        &provider,
        channel_id,
        "create-plain",
        &genesis.id.to_hex(),
        None,
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![genesis.clone()]).await;
    // A relay publishes its identity and a provider witnesses it; without one
    // no receipt verifies, so a genesis-bearing create is refused by name
    // rather than admitted on the assumption that nothing has been handed
    // over. Doing it here is what production does at startup.
    provider.set_rest_client(relay.rest_client());
    provider.witness_relay_identity().await;

    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    pump_until_turn_finished(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");

    let prompt = first_prompt_item(&sink);
    assert_eq!(
        prompt["item"]["operatorPubkey"],
        test_operator_keys().public_key().to_hex()
    );
    assert!(prompt["item"].get("senderRole").is_none(), "{prompt}");

    relay.shutdown().await;
    server.abort();
}
