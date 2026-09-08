//! Strict native restore: a promise made before a restart is kept against the
//! generation that was promised, or refused by name.
//!
//! Every test here follows the same shape, because it is the shape of the bug.
//! One provider creates an execution and registers a CI continuation against
//! it, then **dies** — dropped, nothing flushed, no actor asked to stop, which
//! is what a `kill -9` leaves behind. A second provider opens the same state
//! directory and has to answer the promise. Before this lane it always
//! answered `turn_dropped/NO_LIVE_EXECUTION`: the record survived, the adapter
//! did not, and the honest-but-useless disposition was the only one available.
//!
//! What the restore adds is a third option between "deliver into a process
//! that is not there" and "give up": reopen the *same* generation on the
//! adapter's own `session/load`, with the seat's own key, and deliver into it.
//! The tests below pin both halves — that it happens when it can, and that
//! when it cannot the failure is named, durable, and creates nothing.
//!
//! Two invariants are load-bearing enough to assert everywhere:
//!
//! - **The generation never moves.** A restore is not a resume. `generation`,
//!   `generationCommandId`, `agentRef` and the transcript sequence all
//!   continue, so the sender watching the target it registered against is
//!   watching the target the turn arrives on.
//! - **`session/new` is never sent.** Each fake adapter here would happily
//!   answer it — that is deliberate, so the absence proves strict mode rather
//!   than an adapter that could not have fallen through anyway. The fakes
//!   record every method they receive to a log the test reads.

use super::*;

use crate::ci_continuation_store::{ReadyResult, RecordState};
use crate::native_restore::{
    NATIVE_RESTORE_REJECTED, NATIVE_RESTORE_UNSUPPORTED, NO_RESUME_CURSOR, SESSION_RESTORED_NATIVE,
};
use crate::session::testing::{load_rejecting_agent, restorable_agent, unreattachable_agent};
use buzz_core::ci_result::{
    correlation_id, CiConclusion, CiPhase, CiResult, CiResultIdentity, CI_RESULT_SCHEMA,
};
use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CODING_SESSION_COMMAND_SCHEMA,
};

const RESTORE_RELAY_SELF: &str = "1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a";

fn identity() -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: "136".into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    }
}

fn digest() -> String {
    correlation_id(&identity()).expect("valid identity digest")
}

fn verified_result() -> ReadyResult {
    let result = CiResult {
        schema: CI_RESULT_SCHEMA.into(),
        identity: identity(),
        conclusion: CiConclusion::Success,
        evidence_url: None,
        summary: Some("gate output".into()),
    };
    ReadyResult {
        result_event_id: "fe".repeat(32),
        result_signer: RESTORE_RELAY_SELF.to_owned(),
        result_canonical_json: serde_json::to_string(&result).expect("encode result"),
        observed_at: now_secs(),
    }
}

/// The listener report a verified relay-signed result produces.
fn ready_report() -> crate::ci_result_listener::CiListenerEvent {
    let ready = verified_result();
    crate::ci_result_listener::CiListenerEvent::CiResultReady {
        digest: digest(),
        event_id: ready.result_event_id,
        signer: ready.result_signer,
        canonical_json: ready.result_canonical_json,
        observed_at: ready.observed_at,
    }
}

/// A signed `thread.turn.continue_on_ci`, as the CLI publishes it.
fn registration(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    expires_at: u64,
) -> Event {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnContinueOnCi {
            identity: identity(),
            continuation: "open a PR with the fix".to_owned(),
            expires_at,
        },
    };
    payload.validate().expect("the registration must be valid");
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_COMMAND as u16),
        serde_json::to_string(&payload).expect("encode registration"),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(test_operator_keys())
    .expect("sign registration")
}

/// A create inside the CI project, seated or not.
fn create_in_ci_project(
    provider: &Provider,
    channel_id: Uuid,
    command_id: &str,
    seat: Option<&str>,
) -> Event {
    let source = match seat {
        Some(actor) => seated_create_event(provider, channel_id, command_id, actor, "lead"),
        None => create_event(provider, channel_id, command_id),
    };
    let mut content: serde_json::Value =
        serde_json::from_str(&source.content).expect("create JSON");
    content["action"]["projectRef"] = identity().project.into();
    signed_lifecycle_event(channel_id, content.to_string())
}

/// Everything a restarted provider needs to find the state the dead one left.
struct Killed {
    projects: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    target: CodingSessionTarget,
    /// The transcript sequence the dead provider had reached, so a test can
    /// prove the restore continued the numbering rather than restarting it.
    next_seq: u64,
    /// The whole durable record as it stood at the crash, so a test can prove
    /// field by field what the restore did and did not touch.
    record: crate::state::SessionRecord,
}

/// One provider's whole life before a `kill -9`: create, register, and die.
///
/// The adapter is the ordinary cooperative fake, so the ACP cursor the record
/// stores is a real one from a real `session/new`. Nothing is flushed and no
/// actor is asked to stop — that is the state under test.
async fn register_and_die(dir: &Path, seat: Option<&str>, ready: bool) -> Killed {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir, channel_id, &cwd);
    let state_dir = dir.join("state");
    if let Some(actor) = seat {
        write_actor_seats(dir, "create-1", actor);
    }
    let mut provider = provider(&state_dir, Some(&projects));
    let create = create_in_ci_project(&provider, channel_id, "create-1", seat);
    provider
        .handle_command_event(channel_id, &create)
        .await
        .expect("create");
    let record = provider
        .state()
        .sessions()
        .next()
        .expect("the create minted a session")
        .clone();
    let target = record.target("instance-1");
    provider
        .handle_command_event(
            channel_id,
            &registration(channel_id, "cic-1", &target, now_secs() + 3_600),
        )
        .await
        .expect("register");
    if ready {
        provider
            .ci_continuations
            .mark_ready(&digest(), &verified_result())
            .expect("the verified result is durable before the crash");
    }
    let record = provider
        .state()
        .session(&target.session_id)
        .expect("record")
        .clone();
    let next_seq = record.next_seq;
    drop(provider);
    Killed {
        projects,
        state_dir,
        target,
        next_seq,
        record,
    }
}

/// A provider over `killed`'s state directory, running `script` as its adapter.
///
/// A different adapter build than the one before the restart, which is both
/// realistic and useful: the method log this one writes can only contain calls
/// made *after* the restart.
fn restarted_with(dir: &Path, killed: &Killed, name: &str, script: &str) -> Provider {
    let agent = fake_agent(dir, name, script);
    Provider::new(config_of(
        Keys::generate(),
        &killed.state_dir,
        Some(&killed.projects),
        agent,
    ))
    .expect("provider")
}

/// Every ACP method the fake adapter was asked for, in order.
fn methods(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// The most recent generation metadata published for `session_id`.
fn latest_metadata(sink: &CollectingSink, session_id: &str) -> serde_json::Value {
    sink.contents_of(KIND_CODING_SESSION_METADATA)
        .into_iter()
        .rfind(|metadata| metadata["session"]["sessionId"] == session_id)
        .expect("the restored generation published metadata")
}

/// Every transcript status row published, in sequence order.
fn statuses(sink: &CollectingSink) -> Vec<String> {
    transcript_items_in_sequence(sink)
        .into_iter()
        .filter_map(|row| row["item"]["status"].as_str().map(str::to_owned))
        .collect()
}

// -------------------------------------------------------------------------
// The turn is delivered to the generation that was promised
// -------------------------------------------------------------------------

/// (a) A *waiting* registration whose provider was killed. The desktop
/// re-stages the seat under the generation's command id — this test writes it
/// by hand, which is exactly what
/// `restage_actor_seats_for_provider` does on the host — and the result then
/// arrives. One turn starts, against the original target, as the original
/// agent.
#[tokio::test]
async fn a_waiting_registration_restores_the_generation_and_delivers_one_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let actor = "cd".repeat(32);
    let killed = register_and_die(dir.path(), Some(&actor), false).await;
    // The host's re-stage, stood in for: custody under the *generation's*
    // command id, which for a never-resumed execution is the create's.
    write_actor_seats(dir.path(), "create-1", &actor);
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted
        .handle_ci_listener_event(ready_report())
        .await
        .expect("the verified result arrives");
    pump_until_turn_started(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");

    // Exactly one turn, and it started — nothing was dropped or refused.
    let stages = receipt_stages(&sink, "cic-1");
    assert_eq!(
        stages
            .iter()
            .filter(|stage| *stage == "turn_started")
            .count(),
        1,
        "{stages:?}"
    );
    assert!(
        !stages
            .iter()
            .any(|stage| stage == "turn_dropped" || stage == "turn_refused"),
        "{stages:?}"
    );
    // Against the generation that was registered, not a successor.
    let record = restarted
        .state()
        .session(&killed.target.session_id)
        .expect("the record survived");
    assert_eq!(record.generation, killed.target.generation);
    assert_eq!(record.generation_command_id.as_deref(), Some("create-1"));
    let metadata = latest_metadata(&sink, &killed.target.session_id);
    assert_eq!(metadata["session"]["generation"], killed.target.generation);
    assert_eq!(metadata["agentRef"], actor);
    // The restore said so, in the transcript, and the numbering continued.
    let restored = transcript_items_in_sequence(&sink)
        .into_iter()
        .find(|row| row["item"]["status"] == SESSION_RESTORED_NATIVE)
        .expect("the restore published a status row");
    assert_eq!(restored["eventSeq"], killed.next_seq);
    assert!(
        record.next_seq > killed.next_seq,
        "the sequence restarted: {} -> {}",
        killed.next_seq,
        record.next_seq
    );
    // And the conversation was reattached, never replaced.
    let asked = methods(&log);
    assert!(asked.contains(&"session/load".to_owned()), "{asked:?}");
    assert!(!asked.contains(&"session/new".to_owned()), "{asked:?}");

    // Field by field: the *only* thing a restore may change about the durable
    // record is its transcript sequence (which advances because the restore
    // published a row) and, if the adapter had reported one, the effective
    // model. Everything else has to be what it was, because everything else
    // describes the generation rather than the process behind it. Stated as an
    // equality against the pre-crash record so a field added later cannot
    // quietly start moving.
    let mut before = killed.record.clone();
    let mut after = record.clone();
    assert!(after.next_seq > before.next_seq, "{before:?} -> {after:?}");
    before.next_seq = 0;
    after.next_seq = 0;
    // The turn this delivery started is in flight; the crash's record had none.
    after.open_turn = None;
    assert_eq!(before, after);

    // The one-shot custody was spent by the restore, not left at rest.
    let seats = std::fs::read_to_string(dir.path().join(config::ACTOR_SEATS_FILE_NAME))
        .expect("read seats");
    assert!(
        !seats.contains(TEST_SEAT_NSEC),
        "the restore left the seat's key at rest: {seats}"
    );
}

/// (b) The same, from the other crash window: the verified result was already
/// durable when the provider died, so the first tick after the restart is what
/// delivers it.
#[tokio::test]
async fn a_ready_registration_restores_the_generation_and_delivers_one_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let actor = "cd".repeat(32);
    let killed = register_and_die(dir.path(), Some(&actor), true).await;
    write_actor_seats(dir.path(), "create-1", &actor);
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_until_turn_started(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    assert!(
        receipt_stages(&sink, "cic-1").contains(&"turn_started".to_owned()),
        "{:?}",
        receipt_stages(&sink, "cic-1")
    );
    let record = restarted
        .state()
        .session(&killed.target.session_id)
        .expect("record");
    assert_eq!(record.generation, killed.target.generation);
    assert!(statuses(&sink).contains(&SESSION_RESTORED_NATIVE.to_owned()));
    assert!(!methods(&log).contains(&"session/new".to_owned()));
}

/// (h) An execution nobody is seated in — a human's own session — needs no
/// custody at all. It restores from its cursor alone.
#[tokio::test]
async fn an_unseated_execution_restores_with_the_cursor_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_until_turn_started(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    assert!(
        receipt_stages(&sink, "cic-1").contains(&"turn_started".to_owned()),
        "{:?}",
        receipt_stages(&sink, "cic-1")
    );
    let asked = methods(&log);
    assert!(asked.contains(&"session/load".to_owned()), "{asked:?}");
    assert!(!asked.contains(&"session/new".to_owned()), "{asked:?}");
    let metadata = latest_metadata(&sink, &killed.target.session_id);
    assert!(metadata["agentRef"].is_null(), "{metadata}");
}

/// (j) The restore does not weaken the at-most-once fence: a second delivery
/// pass over the same registration produces no second turn.
#[tokio::test]
async fn a_second_delivery_after_a_restore_is_still_fenced_to_one_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_until_turn_started(&mut restarted).await;
    // The listener replays the same verified result, and the tick runs again.
    restarted
        .handle_ci_listener_event(ready_report())
        .await
        .expect("replayed result");
    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let started = receipt_stages(&sink, "cic-1")
        .into_iter()
        .filter(|stage| stage == "turn_started")
        .count();
    assert_eq!(started, 1, "{:?}", receipt_stages(&sink, "cic-1"));
    // One reattachment, not two: the second pass found a live handle.
    assert_eq!(
        methods(&log)
            .into_iter()
            .filter(|method| method == "session/load")
            .count(),
        1
    );
}

// -------------------------------------------------------------------------
// Custody that has not arrived yet defers; custody that never arrives refuses
// -------------------------------------------------------------------------

/// (c) The seat has not been re-staged yet. Nothing is refused, nothing is
/// created, and no turn runs — and when the custody does land, the very next
/// pass delivers exactly one turn.
#[tokio::test]
async fn a_seat_that_has_not_been_restaged_defers_and_then_delivers_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let actor = "cd".repeat(32);
    let killed = register_and_die(dir.path(), Some(&actor), true).await;
    // Deliberately not re-staged yet: `write_actor_seats` from the first
    // provider's create was consumed at spawn.
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    {
        let sink = CollectingSink::new();
        restarted.flush(&sink).await.expect("flush");
        let stages = receipt_stages(&sink, "cic-1");
        assert!(
            !stages
                .iter()
                .any(|stage| stage.starts_with("turn_") && stage != "turn_queued"),
            "a deferral answered the registration: {stages:?}"
        );
        assert!(
            methods(&log).is_empty(),
            "an adapter was spawned without the seat's key: {:?}",
            methods(&log)
        );
        let stored = restarted
            .ci_continuations
            .record("cic-1")
            .expect("the promise is still owed");
        assert!(matches!(stored.state, RecordState::Ready(_)));
        assert_eq!(
            stored.last_obstacle.as_deref(),
            Some(crate::payload::ACTOR_UNAVAILABLE)
        );
        assert_eq!(stored.attempts, 1);
    }

    // The desktop re-stages custody, and the backoff window passes.
    write_actor_seats(dir.path(), "create-1", &actor);
    restarted
        .ci_continuations
        .note_attempt("cic-1", 0)
        .expect("clear the backoff");
    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_until_turn_started(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let started = receipt_stages(&sink, "cic-1")
        .into_iter()
        .filter(|stage| stage == "turn_started")
        .count();
    assert_eq!(started, 1, "{:?}", receipt_stages(&sink, "cic-1"));
    let record = restarted
        .state()
        .session(&killed.target.session_id)
        .expect("record");
    assert_eq!(record.generation, killed.target.generation);
}

/// (d) Custody never arrives, and the window closes. The terminal answer names
/// custody rather than CI, and no conversation was ever created.
#[tokio::test]
async fn a_seat_that_never_arrives_expires_naming_custody() {
    let dir = tempfile::tempdir().expect("tempdir");
    let actor = "cd".repeat(32);
    let killed = register_and_die(dir.path(), Some(&actor), true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("defer");
    // The registration's window closes with the obstacle still in place.
    restarted
        .ci_continuations
        .expire_now("cic-1")
        .expect("expire");
    restarted.run_ci_continuation_tick().await.expect("expire");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("the registration was answered");
    assert_eq!(refusal["error"]["code"], crate::payload::ACTOR_UNAVAILABLE);
    let message = refusal["error"]["message"].as_str().expect("message");
    assert!(message.contains("key"), "{message}");
    // Nothing was created in its place.
    assert!(methods(&log).is_empty(), "{:?}", methods(&log));
    assert_eq!(restarted.state().sessions().count(), 1);
    assert_eq!(restarted.sessions.live_count(), 0);
}

// -------------------------------------------------------------------------
// Obstacles that refuse immediately, and create nothing
// -------------------------------------------------------------------------

/// (e) The adapter offers `session/load` and rejects this session. Before
/// strict mode this fell through to `session/new` and the generation carried
/// on with a conversation that remembered nothing.
#[tokio::test]
async fn an_adapter_that_rejects_the_reattachment_refuses_and_starts_no_conversation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "load-rejecting-agent",
        &load_rejecting_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("the registration was answered");
    assert_eq!(refusal["error"]["code"], NATIVE_RESTORE_REJECTED);
    let asked = methods(&log);
    assert!(asked.contains(&"session/load".to_owned()), "{asked:?}");
    assert!(
        !asked.contains(&"session/new".to_owned()),
        "the strict open fell through to a new conversation: {asked:?}"
    );
    // The record still names the conversation it always named.
    let record = restarted
        .state()
        .session(&killed.target.session_id)
        .expect("record");
    assert_eq!(record.resume_cursor.as_deref(), Some("acp-session-1"));
    assert_eq!(record.generation, killed.target.generation);
    assert_eq!(restarted.sessions.live_count(), 0);
}

/// The runtime has no reattachment at all. A different code from a rejection,
/// because the remedies differ: this one is about which adapter is installed.
#[tokio::test]
async fn an_adapter_with_no_reattachment_refuses_as_unsupported() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "unreattachable-agent",
        &unreattachable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("the registration was answered");
    assert_eq!(refusal["error"]["code"], NATIVE_RESTORE_UNSUPPORTED);
    assert!(
        !methods(&log).contains(&"session/new".to_owned()),
        "{:?}",
        methods(&log)
    );
}

/// (f) No stored cursor, so there is nothing to reattach *to*. Refused before
/// the adapter is asked anything — the fact is about this provider's own
/// record.
#[tokio::test]
async fn an_execution_with_no_cursor_refuses_before_it_spawns() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    let log = dir.path().join("restore-methods.log");
    let mut restarted = restarted_with(
        dir.path(),
        &killed,
        "restorable-agent",
        &restorable_agent(&log.to_string_lossy()),
    );
    restarted.recover().expect("recover");
    restarted
        .state
        .update_session(&killed.target.session_id, |record| {
            record.resume_cursor = None;
        })
        .expect("forget the cursor");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("the registration was answered");
    assert_eq!(refusal["error"]["code"], NO_RESUME_CURSOR);
    assert!(
        methods(&log).is_empty(),
        "an adapter was spawned for a restore that could not succeed: {:?}",
        methods(&log)
    );
}

/// (g) The runtime the generation ran on is no longer installed on this host.
/// Nothing else can be tried, and nothing else is.
#[tokio::test]
async fn an_execution_whose_runtime_is_gone_refuses_as_provider_unavailable() {
    let dir = tempfile::tempdir().expect("tempdir");
    let killed = register_and_die(dir.path(), None, true).await;
    // The host comes back offering a *different* runtime instance, so the
    // record's `providerInstanceRef` resolves to no descriptor at all.
    let agent = fake_agent(dir.path(), "other-agent", GOOD_AGENT);
    let mut runtime = claude_runtime(agent);
    runtime.instance_ref = "codex-primary".into();
    let mut restarted = Provider::new(config_of_runtimes(
        Keys::generate(),
        &killed.state_dir,
        Some(&killed.projects),
        vec![runtime],
    ))
    .expect("provider");
    restarted.recover().expect("recover");

    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("the registration was answered");
    assert_eq!(
        refusal["error"]["code"],
        crate::payload::PROVIDER_UNAVAILABLE
    );
    assert_eq!(restarted.sessions.live_count(), 0);
}

// -------------------------------------------------------------------------
// The seat-request ledger the host reads
// -------------------------------------------------------------------------

/// (i) The file the desktop's re-stage reads follows the record set: a seated
/// create adds a row under the create's command id, a resume moves it to the
/// resume's, and a stop removes it.
#[tokio::test]
async fn the_seat_request_ledger_follows_create_resume_and_stop() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let actor = "cd".repeat(32);
    write_actor_seats(dir.path(), "create-1", &actor);
    let mut provider = provider(&state_dir, Some(&projects));

    let create = seated_create_event(&provider, channel_id, "create-1", &actor, "lead");
    provider
        .handle_command_event(channel_id, &create)
        .await
        .expect("create");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");

    let rows = seat_requests(&state_dir);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].command_id, "create-1");
    assert_eq!(rows[0].actor, actor);
    assert_eq!(rows[0].role, "lead");
    assert_eq!(rows[0].session_id, target.session_id);
    assert_eq!(rows[0].generation, 1);

    // A resume moves the generation, and the key custody is filed under moves
    // with it.
    provider.sessions.shutdown(&target.session_id);
    write_actor_seats(dir.path(), "resume-1", &actor);
    let resume =
        lifecycle_target_event(&provider, channel_id, "resume-1", "session.resume", &target);
    provider
        .handle_command_event(channel_id, &resume)
        .await
        .expect("resume");
    let rows = seat_requests(&state_dir);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(rows[0].command_id, "resume-1");
    assert_eq!(rows[0].generation, 2);

    // A stop retires the generation, so it stops asking for a key.
    let stopped = CodingSessionTarget {
        generation: 2,
        ..target.clone()
    };
    let stop = lifecycle_target_event(&provider, channel_id, "stop-1", "session.stop", &stopped);
    provider
        .handle_command_event(channel_id, &stop)
        .await
        .expect("stop");
    assert!(seat_requests(&state_dir).is_empty());
}

/// The rows a provider's state directory currently advertises.
fn seat_requests(state_dir: &Path) -> Vec<crate::seat_requests::SeatRequest> {
    let body = std::fs::read_to_string(crate::seat_requests::seat_requests_path(state_dir))
        .expect("the provider wrote its seat requests");
    serde_json::from_str::<crate::seat_requests::SeatRequestsFile>(&body)
        .expect("parse seat requests")
        .requests
}
