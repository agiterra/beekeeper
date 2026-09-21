//! Releasing a held wake is a **re-admission**, and nothing is held forever.
//!
//! Astra's Wave 2 re-check, R4. The landed release rebuilt a
//! `TurnDecision::Start` from the durable record and handed it straight to
//! `apply_turn_decision`, so every check in the ordinary decision path —
//! consumed/in-flight, the freshness horizon, the target's generation, the
//! signer's authority — was skipped for exactly the commands that had been
//! waiting longest. These cases drive the release pass and assert on what the
//! provider publishes.

use super::*;
use crate::deferred_turns::{defer, forget_mirror, held, DeferredTurn, MAX_DEFERRED_TURNS};

/// One held wake, as `on_turn` would have written it: the whole signed
/// command, plus the facts the floor and the seat lookup need.
fn deferred(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    created_at: u64,
    content: &str,
    operator_pubkey: &str,
) -> DeferredTurn {
    DeferredTurn {
        command_id: command_id.to_owned(),
        channel_id,
        created_at,
        operator_pubkey: operator_pubkey.to_owned(),
        event_id: "cd".repeat(32),
        target: target.clone(),
        text: "the prompt".to_owned(),
        content: content.to_owned(),
        attachments: Vec::new(),
        deliver: buzz_core::coding_session_command::CodingSessionDelivery::Boundary,
        operation_key: None,
        assignment_ref: "ef".repeat(32),
        reason: "input_not_established".to_owned(),
        deferred_at: "2026-09-21T00:00:00Z".to_owned(),
    }
}

/// The receipts published for one command, in order.
fn receipts_for(sink: &CollectingSink, command_id: &str) -> Vec<serde_json::Value> {
    sink.contents_of(buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .filter(|receipt| receipt["commandId"] == command_id)
        .collect()
}

/// R4's safety case: a wake deferred against generation N must not be
/// delivered to the execution that is now generation N+1. The release goes
/// through admission, which refuses a stale generation; before this lane the
/// rebuilt decision never asked, and delivery selects the actor by session id,
/// so the old wake reached the new generation.
#[tokio::test]
async fn a_wake_deferred_against_an_old_generation_is_not_delivered_to_the_new_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    forget_mirror();
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&state, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let session_id = record.session_id.clone();
    let stale_target = provider.target_for(&record);
    assert_eq!(stale_target.generation, 1);

    // The wake, held against generation 1.
    let wake = turn_event(channel_id, "wake-1", &stale_target);
    defer(
        &state,
        deferred(
            channel_id,
            "wake-1",
            &stale_target,
            wake.created_at.as_secs(),
            &wake.content,
            &provider.pubkey_hex.clone(),
        ),
    )
    .expect("held");

    // The execution moves on: same session id, next generation. Inserted
    // directly — what is under test is whether the release compares
    // generations at all, not how a resume increments one.
    let mut resumed = record.clone();
    resumed.generation = 2;
    provider
        .state
        .insert_session(resumed)
        .expect("the execution moved on");
    assert_eq!(
        provider
            .state()
            .session(&session_id)
            .expect("session")
            .generation,
        2
    );

    provider.release_deferred_turns().await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let answers = receipts_for(&sink, "wake-1");
    assert!(
        !answers
            .iter()
            .any(|receipt| receipt["status"] == "turn_queued"),
        "a generation-1 wake must never be queued on generation 2: {answers:?}"
    );
    assert!(
        held(&state).is_empty(),
        "and it stops being held rather than clamping the floor forever"
    );
}

/// A signer with no authority over the session is refused by admission on
/// release, exactly as it would be on arrival — the release path used to ask
/// nobody.
#[tokio::test]
async fn a_wake_from_a_signer_without_authority_is_refused_on_release() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    forget_mirror();
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&state, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let target = provider.target_for(&record);

    // A stranger's key: never the session's operator, and holding no grant.
    let stranger = Keys::generate().public_key().to_hex();
    let wake = turn_event(channel_id, "wake-1", &target);
    defer(
        &state,
        deferred(
            channel_id,
            "wake-1",
            &target,
            wake.created_at.as_secs(),
            &wake.content,
            &stranger,
        ),
    )
    .expect("held");

    provider.release_deferred_turns().await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let answers = receipts_for(&sink, "wake-1");
    assert!(
        !answers
            .iter()
            .any(|receipt| receipt["status"] == "turn_queued"),
        "an unauthorized signer's held wake must not be delivered: {answers:?}"
    );
    assert!(
        held(&state).is_empty(),
        "and it is discharged rather than retried forever"
    );
}

/// R4's liveness case: a wake whose input never becomes establishable is
/// **expired** once it passes the command horizon — with a receipt a person
/// can see — and it stops standing in front of that seat's later work.
#[tokio::test]
async fn an_undecidable_wake_expires_and_stops_blocking_the_seats_later_work() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    forget_mirror();
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&state, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let mut record = provider.state().sessions().next().expect("session").clone();
    // A commit-bound role, so an assignment pointer really does reach the
    // fence and really does stay undecided with no relay to ask.
    record.role = Some("verifier".to_owned());
    record.actor = Some("ef".repeat(32));
    // Both umbrella refs, so the fence gets as far as needing the relay —
    // which is unavailable here, so the answer is *undecided* rather than a
    // durable refusal. That is the state this case is about.
    record.session_ref = Some(Uuid::new_v4().to_string());
    record.genesis_ref = Some("ab".repeat(32));
    provider
        .state
        .insert_session(record.clone())
        .expect("seat record");
    let target = provider.target_for(&record);
    let founder = record
        .founder_pubkey
        .clone()
        .expect("the create recorded its founder");
    let horizon = provider.config.command_horizon.as_secs();

    // The old one: deferred longer ago than any command is valid for.
    let ancient = now_secs().saturating_sub(horizon + 60);
    defer(
        &state,
        deferred(
            channel_id,
            "wake-old",
            &target,
            ancient,
            &turn_event(channel_id, "wake-old", &target).content,
            &founder,
        ),
    )
    .expect("held");
    // And a fresh one behind it, on the same seat — an assignment pointer,
    // which with no relay to verify against stays undecided and therefore
    // still owed.
    let fresh = command_event(
        channel_id,
        "wake-new",
        &target,
        serde_json::json!({
            "type": "thread.turn.start",
            "text": serde_json::json!({"operationId": "ef".repeat(32), "type": "assignment"})
                .to_string(),
        }),
    );
    defer(
        &state,
        deferred(
            channel_id,
            "wake-new",
            &target,
            fresh.created_at.as_secs(),
            &fresh.content,
            &founder,
        ),
    )
    .expect("held");
    assert_eq!(held(&state).len(), 2);
    assert_eq!(
        provider.watermark_ceiling(channel_id),
        Some(ancient),
        "the stale wake is clamping the floor"
    );

    provider.release_deferred_turns().await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let expiry = receipts_for(&sink, "wake-old");
    let refusal = expiry
        .iter()
        .find(|receipt| {
            receipt["error"]["code"] == crate::verification_input::VERIFICATION_INPUT_EXPIRED
        })
        .unwrap_or_else(|| panic!("an expiry a person can see; got {expiry:?}"));
    assert!(
        refusal["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("command horizon")),
        "{refusal}"
    );
    let remaining = held(&state);
    assert_eq!(
        remaining
            .iter()
            .map(|turn| turn.command_id.as_str())
            .collect::<Vec<_>>(),
        vec!["wake-new"],
        "the stale wake is gone and the live one is still owed"
    );
    assert_ne!(
        provider.watermark_ceiling(channel_id),
        Some(ancient),
        "and the floor is no longer pinned to it"
    );
}

/// A wake this provider cannot hold durably is **answered**, not forgotten:
/// the one outcome a lead could not see is "owed by nobody".
///
/// The write fails because the state directory's own parent is a regular
/// file — `ENOTDIR`, which no privilege overrides, so this says the same
/// thing when CI runs as root.
#[tokio::test]
async fn a_wake_that_cannot_be_held_is_refused_rather_than_lost() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    forget_mirror();
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&state, Some(&projects));
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("handle");
    pump_available(&mut provider).await;
    let record = provider.state().sessions().next().expect("session").clone();
    let target = provider.target_for(&record);

    let not_a_directory = dir.path().join("not-a-directory");
    std::fs::write(&not_a_directory, "a regular file\n").expect("write");
    let unwritable = not_a_directory.join("state");
    let wake = turn_event(channel_id, "wake-1", &target);
    let error = defer(
        &unwritable,
        deferred(
            channel_id,
            "wake-1",
            &target,
            wake.created_at.as_secs(),
            &wake.content,
            &provider.pubkey_hex.clone(),
        ),
    )
    .expect_err("a store that cannot be written holds nothing");
    assert!(
        !held(&unwritable)
            .iter()
            .any(|turn| turn.command_id == "wake-1"),
        "and the mirror must not claim it did: {error}"
    );

    // The provider's own answer for that failure: a refusal, published.
    provider
        .hold_undecided_turn(deferred(
            channel_id,
            "wake-2",
            &target,
            wake.created_at.as_secs(),
            &wake.content,
            &provider.pubkey_hex.clone(),
        ))
        .await
        .expect("the failure is handled, not propagated");
    // With a writable store this one is simply held.
    assert!(
        held(&state).iter().any(|turn| turn.command_id == "wake-2"),
        "a writable store holds it"
    );
}

/// Nothing owed is evicted to make room: a full store refuses the newcomer
/// and keeps every wake it already owes an answer for.
#[test]
fn a_full_store_refuses_the_new_deferral_rather_than_dropping_an_old_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    forget_mirror();
    let channel_id = Uuid::new_v4();
    let target = CodingSessionTarget {
        driver: "claude".to_owned(),
        instance_id: "instance".to_owned(),
        session_id: "seat-1".to_owned(),
        generation: 1,
    };
    for index in 0..MAX_DEFERRED_TURNS {
        defer(
            &state,
            deferred(
                channel_id,
                &format!("wake-{index}"),
                &target,
                1_700_000_000 + index as u64,
                "{}",
                &"ab".repeat(32),
            ),
        )
        .expect("held");
    }
    let error = defer(
        &state,
        deferred(
            channel_id,
            "wake-one-too-many",
            &target,
            1_800_000_000,
            "{}",
            &"ab".repeat(32),
        ),
    )
    .expect_err("a full store refuses");
    assert!(error.contains("nothing owed is dropped"), "{error}");
    let remaining = held(&state);
    assert_eq!(remaining.len(), MAX_DEFERRED_TURNS);
    assert!(
        remaining.iter().any(|turn| turn.command_id == "wake-0"),
        "the oldest owed wake is still owed"
    );
    assert!(
        !remaining
            .iter()
            .any(|turn| turn.command_id == "wake-one-too-many"),
        "and the refused one was not admitted"
    );
}
