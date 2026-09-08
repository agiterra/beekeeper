//! CI-managed continuation: admission, delivery, refusal, expiry, recovery.
//!
//! The guarantee under test, stated once: for one exact target and one CI
//! correlation digest this provider admits **at most one** continuation turn,
//! durably, across restarts, duplicate result events, reconnect replays and any
//! number of registration command ids. Nothing here is a new fence — the turn
//! is rebuilt as an ordinary `thread.turn.start` whose operation pointer is the
//! compact `{"operationId":…,"type":"ci_result"}`, and the existing operation
//! ledger is what makes it once.
//!
//! The listener is exercised on its own socket in
//! `crate::ci_result_listener::tests`; these tests feed the provider the same
//! reports the run loop's `select!` arm delivers, so the durable consequences
//! are the subject rather than the transport.

use super::*;

use crate::ci_continuation_store::{CiContinuationStore, ReadyResult, RecordState};
use crate::ci_result_listener::CiListenerEvent;
use buzz_core::ci_result::{
    correlation_id, CiConclusion, CiPhase, CiResult, CiResultIdentity, CI_RESULT_SCHEMA,
};
use buzz_core::coding_session_command::{
    ci_continuation_pointer, CodingSessionAction, CodingSessionCommandPayload,
    CODING_SESSION_COMMAND_SCHEMA,
};

const RELAY_SELF: &str = "1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a";

fn ci_identity(run: &str) -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: run.into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    }
}

fn digest_of(identity: &CiResultIdentity) -> String {
    correlation_id(identity).expect("valid identity digest")
}

/// A signed `thread.turn.continue_on_ci`, as the CLI publishes it.
fn continuation_event(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    identity: &CiResultIdentity,
    continuation: &str,
    expires_at: u64,
) -> Event {
    continuation_event_signed(
        channel_id,
        command_id,
        target,
        identity,
        continuation,
        expires_at,
        test_operator_keys(),
    )
}

fn continuation_event_signed(
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    identity: &CiResultIdentity,
    continuation: &str,
    expires_at: u64,
    keys: &Keys,
) -> Event {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnContinueOnCi {
            identity: identity.clone(),
            continuation: continuation.to_owned(),
            expires_at,
        },
    };
    payload.validate().expect("the registration must be valid");
    let content = serde_json::to_string(&payload).expect("encode registration");
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_CODING_SESSION_COMMAND as u16),
        content,
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(keys)
    .expect("sign registration")
}

/// The listener report a verified relay-signed result produces.
fn ready_report(identity: &CiResultIdentity, conclusion: CiConclusion) -> CiListenerEvent {
    let result = CiResult {
        schema: CI_RESULT_SCHEMA.into(),
        identity: identity.clone(),
        conclusion,
        evidence_url: Some("https://ci.agiterra.org/runs/136".into()),
        summary: Some("gate output".into()),
    };
    CiListenerEvent::CiResultReady {
        digest: digest_of(identity),
        event_id: "fe".repeat(32),
        signer: RELAY_SELF.to_owned(),
        canonical_json: serde_json::to_string(&result).expect("encode result"),
        observed_at: now_secs(),
    }
}

fn answered_report(identity: &CiResultIdentity) -> CiListenerEvent {
    CiListenerEvent::CiResultsAnswered {
        digests: vec![digest_of(identity)],
    }
}

fn ci_create_event(provider: &Provider, channel_id: Uuid, command_id: &str) -> Event {
    let original = create_event(provider, channel_id, command_id);
    let mut content: serde_json::Value =
        serde_json::from_str(&original.content).expect("create JSON");
    content["action"]["projectRef"] = ci_identity("136").project.into();
    signed_lifecycle_event(channel_id, content.to_string())
}

/// A provider with one live execution, and the target that addresses it.
async fn provider_with_session(
    dir: &Path,
) -> (Provider, Uuid, CodingSessionTarget, std::path::PathBuf) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir, channel_id, &cwd);
    let state_dir = dir.join("state");
    let mut provider = provider(&state_dir, Some(&projects));
    provider
        .handle_command_event(
            channel_id,
            &ci_create_event(&provider, channel_id, "create-1"),
        )
        .await
        .expect("handle create");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");
    (provider, channel_id, target, projects)
}

/// The receipt error code published for one command, if any.
fn refusal_code(sink: &CollectingSink, command_id: &str) -> Option<String> {
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id && receipt["status"] == "turn_refused")
        .and_then(|receipt| receipt["error"]["code"].as_str().map(str::to_owned))
}

/// Drain session reports until a turn starts, answering the exact text it was
/// started with.
///
/// The delivered prompt is the only thing the woken agent sees, so it is read
/// from the report the execution itself made rather than inferred from a
/// receipt.
async fn started_turn_text(provider: &mut Provider) -> String {
    loop {
        let event = tokio::time::timeout(Duration::from_secs(20), provider.next_session_event())
            .await
            .expect("a session event within the timeout")
            .expect("channel open");
        let started = match &event {
            session::SessionEvent::TurnStarted { text, .. } => Some(text.clone()),
            _ => None,
        };
        provider.handle_session_event(event).expect("record");
        if let Some(text) = started {
            return text;
        }
    }
}

/// Every record in one of the provider's append-only ledgers.
fn ledger_lines(state_dir: &Path, file: &str) -> Vec<serde_json::Value> {
    let Ok(body) = std::fs::read_to_string(state_dir.join(file)) else {
        return Vec::new();
    };
    body.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| serde_json::from_str(line).expect("ledger line is json"))
        .collect()
}

// -------------------------------------------------------------------------
// Admission
// -------------------------------------------------------------------------

/// The whole handshake §2 promises: a registration is durable *before* it is
/// acknowledged, spends no turn, and claims no mailbox.
#[tokio::test]
async fn a_registration_is_durable_before_it_is_acknowledged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");

    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "open a PR with the fix",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle registration");

    let stored = CiContinuationStore::open(&state_dir).expect("reopen store");
    let record = stored.record("cic-1").expect("the record is on disk");
    assert_eq!(record.state, RecordState::Waiting);
    assert_eq!(record.correlation_id, digest_of(&identity));
    assert_eq!(record.continuation, "open a PR with the fix");
    assert_eq!(record.target, target);

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "cic-1"),
        vec!["continuation_registered".to_owned()],
        "a registration is acknowledged once and claims no mailbox turn"
    );
    // No turn was spent and no operation was claimed.
    assert!(ledger_lines(&state_dir, "commands.jsonl")
        .iter()
        .all(|row| row["commandId"] != "cic-1"));
    assert!(ledger_lines(&state_dir, "operations.jsonl").is_empty());
}

/// The recovery §2 documents: a caller whose acknowledgement was lost re-runs
/// the identical command and is answered, without a second registration.
#[tokio::test]
async fn an_exact_retry_is_answered_again_and_registers_nothing_new() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    let expires_at = now_secs() + 3_600;
    let event = continuation_event(channel_id, "cic-1", &target, &identity, "again", expires_at);

    provider
        .handle_command_event(channel_id, &event)
        .await
        .expect("first");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    provider
        .handle_command_event(channel_id, &event)
        .await
        .expect("retry");
    let second = CollectingSink::new();
    provider.flush(&second).await.expect("flush");

    assert_eq!(
        receipt_stages(&second, "cic-1"),
        vec!["continuation_registered".to_owned()],
        "the retry is answered rather than met with silence"
    );
    assert_eq!(
        provider.ci_continuations.records().count(),
        1,
        "and it registers nothing new"
    );
}

/// The same id carrying different bytes is two intents naming one
/// registration. The first durable record wins, out loud.
#[tokio::test]
async fn a_command_id_reused_for_different_bytes_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    let expires_at = now_secs() + 3_600;

    provider
        .handle_command_event(
            channel_id,
            &continuation_event(channel_id, "cic-1", &target, &identity, "first", expires_at),
        )
        .await
        .expect("first");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "a completely different continuation",
                expires_at,
            ),
        )
        .await
        .expect("second");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("COMMAND_ID_CONFLICT")
    );
    assert_eq!(
        provider
            .ci_continuations
            .record("cic-1")
            .expect("record")
            .continuation,
        "first",
        "the first durable record wins"
    );
    let conflict = sink
        .all()
        .into_iter()
        .find(|event| {
            serde_json::from_str::<serde_json::Value>(&event.content)
                .ok()
                .is_some_and(|body| {
                    body["commandId"] == "cic-1" && body["error"]["code"] == "COMMAND_ID_CONFLICT"
                })
        })
        .expect("conflict event");
    conflict.verify().expect("valid signature");
    let payload: LifecycleReceipt = serde_json::from_str(&conflict.content).expect("receipt");
    let expected =
        build_coding_session_turn_receipt(channel_id, "cic-1", payload.status, &conflict.content)
            .expect("strict envelope")
            .sign_with_keys(&provider.config.keys)
            .expect("sign expected");
    assert_eq!(
        conflict.tags, expected.tags,
        "outbox fence never replaces the wire command or semantic key"
    );
    assert!(!provider.state.is_command_refused("cic-1"));
    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("resolve original");
    let text = started_turn_text(&mut provider).await;
    let prompt: serde_json::Value = serde_json::from_str(&text).expect("materialized prompt");
    assert_eq!(prompt["continuation"], "first");
    assert!(provider.state.is_command_consumed("cic-1"));
}

/// A bounded store refuses rather than displacing a promise somebody is
/// already waiting on.
#[tokio::test]
async fn an_ordinary_start_cannot_replace_a_registered_ci_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider.handle_command_event(channel_id, &command_event(channel_id, "cic-expiry", &target,
        serde_json::json!({"type": "thread.turn.start", "text": "unrelated replacement", "deliver": "boundary"})))
        .await.expect("conflicting ordinary start");
    assert!(!provider.in_flight.contains_key("cic-expiry"));
    assert!(!provider.state.is_command_refused("cic-expiry"));
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("resolve original");
    let text = started_turn_text(&mut provider).await;
    let prompt: serde_json::Value = serde_json::from_str(&text).expect("materialized original");
    assert_eq!(prompt["continuation"], "carry on");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("COMMAND_ID_CONFLICT")
    );
}

#[tokio::test]
async fn a_ci_registration_cannot_repurpose_an_ordinary_in_flight_command() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    provider.handle_command_event(channel_id, &command_event(channel_id, "cic-expiry", &target,
        serde_json::json!({"type": "thread.turn.start", "text": "ordinary original", "deliver": "boundary"})))
        .await.expect("ordinary start");
    assert!(provider.in_flight.contains_key("cic-expiry"));
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    assert!(provider.ci_continuations.record("cic-expiry").is_none());
    assert_eq!(started_turn_text(&mut provider).await, "ordinary original");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(!receipt_stages(&sink, "cic-expiry").contains(&"continuation_registered".to_owned()));
}

#[tokio::test]
async fn a_full_store_refuses_the_registration_before_acknowledging_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let expires_at = now_secs() + 3_600;
    let cap = crate::ci_continuation_store::MAX_REGISTRATIONS_PER_CHANNEL;

    for index in 0..cap {
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(
                    channel_id,
                    &format!("cic-{index}"),
                    &target,
                    &ci_identity(&index.to_string()),
                    "wait",
                    expires_at,
                ),
            )
            .await
            .expect("handle");
    }
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-overflow",
                &target,
                &ci_identity("overflow"),
                "wait",
                expires_at,
            ),
        )
        .await
        .expect("handle");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-overflow").as_deref(),
        Some("CI_CONTINUATION_STORE_FULL")
    );
    assert!(provider.ci_continuations.record("cic-overflow").is_none());
    assert_eq!(provider.ci_continuations.pending_count(), cap);
}

/// An expiry already in the past could never deliver anything, and one past
/// the provider's command horizon is a window it does not offer. Both are
/// refused with the code that names what was actually wrong.
#[tokio::test]
async fn an_unusable_expiry_is_refused_with_the_code_that_names_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;

    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-past",
                &target,
                &ci_identity("136"),
                "wait",
                now_secs() - 1,
            ),
        )
        .await
        .expect("handle");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-far",
                &target,
                &ci_identity("137"),
                "wait",
                now_secs() + 86_400 * 30,
            ),
        )
        .await
        .expect("handle");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-past").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
    assert_eq!(
        refusal_code(&sink, "cic-far").as_deref(),
        Some("CI_CONTINUATION_HORIZON")
    );
    assert_eq!(provider.ci_continuations.pending_count(), 0);
}

/// Registering promises a turn, so only somebody who may steer now may
/// register one.
#[tokio::test]
async fn a_signer_who_may_not_steer_cannot_register_a_continuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let stranger = Keys::generate();

    provider
        .handle_command_event(
            channel_id,
            &continuation_event_signed(
                channel_id,
                "cic-1",
                &target,
                &ci_identity("136"),
                "wait",
                now_secs() + 3_600,
                &stranger,
            ),
        )
        .await
        .expect("handle");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("UNAUTHORIZED_OPERATOR")
    );
    assert_eq!(provider.ci_continuations.pending_count(), 0);
}

// -------------------------------------------------------------------------
// Delivery
// -------------------------------------------------------------------------

async fn register_and_resolve(
    provider: &mut Provider,
    channel_id: Uuid,
    target: &CodingSessionTarget,
    command_id: &str,
    identity: &CiResultIdentity,
    conclusion: CiConclusion,
) -> String {
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                command_id,
                target,
                identity,
                "open a PR with the fix",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle registration");
    provider
        .handle_ci_listener_event(answered_report(identity))
        .await
        .expect("answered");
    provider
        .handle_ci_listener_event(ready_report(identity, conclusion))
        .await
        .expect("ready");
    started_turn_text(provider).await
}

/// The delivered turn carries the whole verified fact — result, evidence,
/// registration identity and the requested continuation — so the woken agent
/// fetches nothing to act on it.
#[tokio::test]
async fn a_verified_result_starts_one_turn_carrying_the_materialized_context() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    let text = register_and_resolve(
        &mut provider,
        channel_id,
        &target,
        "cic-1",
        &identity,
        CiConclusion::Success,
    )
    .await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "cic-1"),
        vec![
            "continuation_registered".to_owned(),
            "turn_queued".to_owned(),
            "turn_started".to_owned(),
        ],
        "one registration, then exactly one turn under the same commandId"
    );
    let prompt: serde_json::Value =
        serde_json::from_str(&text).expect("the turn text is the materialized result");
    assert_eq!(prompt["type"], "ci_result");
    assert_eq!(prompt["operationId"], digest_of(&identity));
    assert_eq!(prompt["registration"]["commandId"], "cic-1");
    assert_eq!(
        prompt["registration"]["signer"],
        test_operator_keys().public_key().to_hex()
    );
    assert_eq!(prompt["result"]["signer"], RELAY_SELF);
    assert_eq!(prompt["result"]["conclusion"], "success");
    assert_eq!(
        prompt["result"]["evidenceUrl"],
        "https://ci.agiterra.org/runs/136"
    );
    assert_eq!(
        prompt["result"]["identity"],
        serde_json::to_value(&identity).expect("identity json")
    );
    assert_eq!(prompt["continuation"], "open a PR with the fix");
}

/// A failure is a result. Nothing about the delivery depends on the verdict —
/// an agent that only ever heard about green builds could not act on a red one.
#[tokio::test]
async fn a_failed_and_a_cancelled_run_deliver_their_conclusions_unchanged() {
    for (run, conclusion, expected) in [
        ("137", CiConclusion::Failure, "failure"),
        ("138", CiConclusion::Cancelled, "cancelled"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
        let identity = ci_identity(run);
        let text = register_and_resolve(
            &mut provider,
            channel_id,
            &target,
            "cic-1",
            &identity,
            conclusion,
        )
        .await;
        let prompt: serde_json::Value = serde_json::from_str(&text).expect("materialized result");
        assert_eq!(prompt["result"]["conclusion"], expected);
    }
}

/// A result already stored when the registration arrives is found by the
/// listener's first REQ, not by a special catch-up path.
#[tokio::test]
async fn a_result_stored_before_the_registration_still_delivers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle");
    // The first subscription's replay: the answer and the stored row arrive
    // together, before anything live could have happened.
    provider
        .handle_ci_listener_event(answered_report(&identity))
        .await
        .expect("answered");
    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready");
    pump_until_turn_started(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(receipt_stages(&sink, "cic-1").contains(&"turn_started".to_owned()));
}

/// Two producers, one run, two command ids. The operation ledger is the fence:
/// one turn, and the loser is refused without spending one.
#[tokio::test]
async fn two_command_ids_for_one_run_start_exactly_one_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");
    let expires_at = now_secs() + 3_600;

    for command_id in ["cic-a", "cic-b"] {
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(
                    channel_id, command_id, &target, &identity, "carry on", expires_at,
                ),
            )
            .await
            .expect("handle");
    }
    provider
        .handle_ci_listener_event(answered_report(&identity))
        .await
        .expect("answered");
    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready");
    pump_until_turn_started(&mut provider).await;
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let started: Vec<String> = ["cic-a", "cic-b"]
        .into_iter()
        .filter(|id| receipt_stages(&sink, id).contains(&"turn_started".to_owned()))
        .map(str::to_owned)
        .collect();
    assert_eq!(
        started.len(),
        1,
        "exactly one turn ran; started: {started:?}"
    );
    let loser = if started[0] == "cic-a" {
        "cic-b"
    } else {
        "cic-a"
    };
    assert_eq!(
        refusal_code(&sink, loser).as_deref(),
        Some("DUPLICATE_OPERATION"),
        "the second command id is refused and spends no turn"
    );

    // The durable half: one operation row, keyed by the compact pointer.
    let operations = ledger_lines(&state_dir, "operations.jsonl");
    assert_eq!(operations.len(), 1);
    let expected_key = format!(
        "{}\u{0}{}",
        buzz_core::coding_session_command::coding_session_target_key(&target),
        ci_continuation_pointer(&digest_of(&identity)),
    );
    assert_eq!(operations[0]["key"], expected_key);
}

/// A duplicate result is the same fact arriving twice, and a fact does not
/// become two turns by being repeated.
#[tokio::test]
async fn a_duplicate_result_after_the_turn_started_changes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    let _ = register_and_resolve(
        &mut provider,
        channel_id,
        &target,
        "cic-1",
        &identity,
        CiConclusion::Success,
    )
    .await;
    assert!(
        provider.ci_continuations.record("cic-1").is_none(),
        "the record is retired once the turn's command ledger entry is durable"
    );

    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("duplicate");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "cic-1"),
        vec![
            "continuation_registered".to_owned(),
            "turn_queued".to_owned(),
            "turn_started".to_owned(),
        ],
        "no second turn and no second receipt"
    );
}

/// A model turn is not undone by a contradiction that arrives after it. What
/// the provider owes is the truth in its log, not a second answer to a command
/// it already answered.
#[tokio::test]
async fn a_conflicting_result_after_the_turn_started_neither_refuses_nor_repeats_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    let _ = register_and_resolve(
        &mut provider,
        channel_id,
        &target,
        "cic-1",
        &identity,
        CiConclusion::Success,
    )
    .await;

    provider
        .handle_ci_listener_event(CiListenerEvent::CiResultConflict {
            digest: digest_of(&identity),
        })
        .await
        .expect("conflict");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(refusal_code(&sink, "cic-1"), None);
    assert_eq!(
        receipt_stages(&sink, "cic-1"),
        vec![
            "continuation_registered".to_owned(),
            "turn_queued".to_owned(),
            "turn_started".to_owned(),
        ]
    );
}

/// Before the turn is admitted, a contradiction still can — and must — stop it.
#[tokio::test]
async fn a_conflicting_result_before_the_turn_starts_refuses_the_registration() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle");
    provider
        .handle_ci_listener_event(CiListenerEvent::CiResultConflict {
            digest: digest_of(&identity),
        })
        .await
        .expect("conflict");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("CI_RESULT_CONFLICT")
    );
    assert!(!receipt_stages(&sink, "cic-1").contains(&"turn_queued".to_owned()));
    assert!(matches!(
        provider.ci_continuations.record("cic-1").map(|r| &r.state),
        Some(RecordState::Terminal { .. })
    ));
}

// -------------------------------------------------------------------------
// Authority, generation, closure — re-evaluated at delivery
// -------------------------------------------------------------------------

#[tokio::test]
async fn a_conflict_after_queueing_denies_the_actor_before_its_prompt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("queue");
    assert!(provider.in_flight.contains_key("cic-expiry"));
    provider
        .handle_ci_listener_event(CiListenerEvent::CiResultConflict {
            digest: digest_of(&ci_identity("136")),
        })
        .await
        .expect("conflict before admission");
    pump_available(&mut provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_RESULT_CONFLICT")
    );
    assert!(!receipt_stages(&sink, "cic-expiry").contains(&"turn_started".to_owned()));
    assert!(!provider.state.is_command_consumed("cic-expiry"));
}

#[tokio::test]
async fn queued_ci_turns_recheck_authority_generation_closure_and_project_at_start() {
    for (change, expected) in [
        ("revoked", "UNAUTHORIZED_OPERATOR"),
        ("generation", "STALE_GENERATION"),
        ("closed", "SESSION_CLOSED"),
        ("project", "CI_CONTINUATION_PROJECT_MISMATCH"),
    ] {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
        register_expiry_fixture(&mut provider, channel_id, &target).await;
        provider
            .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
            .await
            .expect("queue");
        assert!(provider.in_flight.contains_key("cic-expiry"));
        provider
            .state
            .update_session(&target.session_id, |record| match change {
                "revoked" => {
                    record.founder_pubkey = Some(Keys::generate().public_key().to_hex());
                    record.granted_operators.clear();
                }
                "generation" => record.generation += 1,
                "closed" => record.closed = true,
                "project" => record.project_ref = None,
                _ => unreachable!(),
            })
            .expect("state changes while queued");
        pump_available(&mut provider).await;
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            refusal_code(&sink, "cic-expiry").as_deref(),
            Some(expected),
            "{change}"
        );
        assert!(
            !receipt_stages(&sink, "cic-expiry").contains(&"turn_started".to_owned()),
            "{change}"
        );
        assert!(
            !provider.state.is_command_consumed("cic-expiry"),
            "{change}"
        );
    }
}

#[tokio::test]
async fn unknown_and_other_projects_cannot_register_a_result_subscription() {
    for project in [None, Some("30621:other:private-project".to_owned())] {
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
        provider
            .state
            .update_session(&target.session_id, |record| record.project_ref = project)
            .expect("different project");
        register_expiry_fixture(&mut provider, channel_id, &target).await;
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        assert_eq!(
            refusal_code(&sink, "cic-expiry").as_deref(),
            Some("CI_CONTINUATION_PROJECT_MISMATCH")
        );
        assert!(provider.ci_continuations.pending_identities().is_empty());
        assert!(provider.ci_continuations.record("cic-expiry").is_none());
    }
}

/// Steering authority is checked *now*, not when the registration was made,
/// and a regrant does not resurrect a refused promise.
#[tokio::test]
async fn a_revoked_signer_is_refused_at_delivery_and_no_regrant_resurrects_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle");

    // The signer stops being able to steer while CI runs: the founder is
    // replaced, which is what a revocation leaves behind for this decision.
    let usurper = Keys::generate().public_key().to_hex();
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.founder_pubkey = Some(usurper.clone());
            record.granted_operators.clear();
        })
        .expect("revoke");

    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("UNAUTHORIZED_OPERATOR")
    );

    // Regranted, and the result reported again: the refusal is durable, so
    // nothing comes back.
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.founder_pubkey = Some(test_operator_keys().public_key().to_hex());
        })
        .expect("regrant");
    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready again");
    provider.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut provider).await;

    let after = CollectingSink::new();
    provider.flush(&after).await.expect("flush");
    assert!(
        !receipt_stages(&after, "cic-1").contains(&"turn_queued".to_owned()),
        "a regrant does not resurrect a refused continuation"
    );
}

/// A generation that moved is never retargeted. The registration named an
/// exact generation and that is the only one it can address.
#[tokio::test]
async fn a_moved_generation_refuses_the_continuation_rather_than_retargeting_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle");
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.generation += 1;
        })
        .expect("resume");

    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("STALE_GENERATION")
    );
}

/// A stopped execution takes its promises with it, visibly.
#[tokio::test]
async fn a_closed_execution_refuses_the_continuation() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("handle");
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.closed = true;
        })
        .expect("close");

    provider
        .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
        .await
        .expect("ready");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("SESSION_CLOSED")
    );
}

// -------------------------------------------------------------------------
// Expiry — the two observations, never a guess
// -------------------------------------------------------------------------

async fn register_expiry_fixture(
    provider: &mut Provider,
    channel_id: Uuid,
    target: &CodingSessionTarget,
) {
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-expiry",
                target,
                &ci_identity("136"),
                "carry on",
                now_secs() + 3_600,
            ),
        )
        .await
        .expect("register");
}

/// Results can arrive between expiry ticks. Admission must enforce the
/// deadline itself instead of relying on a later pass to remove the promise.
#[tokio::test]
async fn a_result_arriving_after_expiry_never_reaches_the_mailbox() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire");
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("late result");
    provider.run_ci_continuation_tick().await.expect("tick");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
    assert!(!provider.in_flight.contains_key("cic-expiry"));
    assert!(!receipt_stages(&sink, "cic-expiry").contains(&"turn_queued".to_owned()));
}

/// Recovery may find a verified result whose admission window elapsed while
/// the process was down. It must expire before ordinary turn admission.
#[tokio::test]
async fn an_expired_ready_record_is_refused_after_recovery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut running, channel_id, target, projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut running, channel_id, &target).await;
    let CiListenerEvent::CiResultReady {
        digest,
        event_id,
        signer,
        canonical_json,
        observed_at,
    } = ready_report(&ci_identity("136"), CiConclusion::Success)
    else {
        panic!("ready fixture");
    };
    running
        .ci_continuations
        .mark_ready(
            &digest,
            &ReadyResult {
                result_event_id: event_id,
                result_signer: signer,
                result_canonical_json: canonical_json,
                observed_at,
            },
        )
        .expect("persist result before mailbox");
    running
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire while down");
    drop(running);

    let mut restarted = provider(&dir.path().join("state"), Some(&projects));
    restarted.recover().expect("recover");
    restarted.run_ci_continuation_tick().await.expect("tick");
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
    assert!(!restarted.in_flight.contains_key("cic-expiry"));
    assert!(!receipt_stages(&sink, "cic-expiry").contains(&"turn_queued".to_owned()));
}

/// A queued turn must still meet the deadline at its execution boundary.
/// Expiry before the provider grants that start prevents the adapter prompt.
#[tokio::test]
async fn a_continuation_queued_before_expiry_is_denied_at_the_execution_boundary() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("result before expiry");
    assert!(provider.in_flight.contains_key("cic-expiry"));
    provider
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire after admission");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
    assert!(!receipt_stages(&sink, "cic-expiry").contains(&"turn_started".to_owned()));
    assert!(!provider.state.is_command_consumed("cic-expiry"));
}

/// The relay never answered, so the honest fact is that the window closed
/// before this provider could look.
#[tokio::test]
async fn an_expiry_with_no_relay_answer_reports_the_registration_as_expired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 1,
            ),
        )
        .await
        .expect("handle");
    provider
        .ci_continuations
        .set_expiry_for_test("cic-1", now_secs() - 1)
        .expect("expire");

    provider.run_ci_continuation_tick().await.expect("tick");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("refusal");
    assert!(receipt["error"]["message"]
        .as_str()
        .expect("message")
        .contains("before this provider could check the relay"));
}

/// The relay answered and held nothing this provider's identity could read, so
/// "not finished" and "not permitted to read" are indistinguishable — and the
/// receipt says exactly that rather than picking one.
#[tokio::test]
async fn an_expiry_after_an_empty_relay_answer_reports_unavailable_or_hidden() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let identity = ci_identity("136");
    provider
        .handle_command_event(
            channel_id,
            &continuation_event(
                channel_id,
                "cic-1",
                &target,
                &identity,
                "carry on",
                now_secs() + 1,
            ),
        )
        .await
        .expect("handle");
    provider
        .handle_ci_listener_event(answered_report(&identity))
        .await
        .expect("answered");
    provider
        .ci_continuations
        .set_expiry_for_test("cic-1", now_secs() - 1)
        .expect("expire");

    provider.run_ci_continuation_tick().await.expect("tick");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-1").as_deref(),
        Some("CI_RESULT_UNAVAILABLE_OR_HIDDEN")
    );
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "cic-1" && receipt["status"] == "turn_refused")
        .expect("refusal");
    let message = receipt["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("private to it, or CI never reported"),
        "{message}"
    );
}

// -------------------------------------------------------------------------
// Crash boundaries
// -------------------------------------------------------------------------

/// A provider killed while a registration is waiting comes back still owing
/// the turn: the record survives and the identity is still watched.
#[tokio::test]
async fn a_restart_before_the_result_keeps_the_registration_and_delivers_later() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");
    let target = {
        let mut provider = provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(
                channel_id,
                &ci_create_event(&provider, channel_id, "create-1"),
            )
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(
                    channel_id,
                    "cic-1",
                    &target,
                    &identity,
                    "carry on",
                    now_secs() + 3_600,
                ),
            )
            .await
            .expect("register");
        // Killed here: nothing flushed, no actor asked to stop.
        target
    };

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    let record = restarted
        .ci_continuations
        .record("cic-1")
        .expect("the promise survives the restart");
    assert_eq!(record.state, RecordState::Waiting);
    assert_eq!(
        restarted.ci_continuations.pending_identities().len(),
        1,
        "and the identity is still watched"
    );
    assert_eq!(record.target, target);
}

/// A crash between "the result is durable" and "the turn is in a mailbox"
/// costs the mailbox, not the turn: the next tick delivers it.
#[tokio::test]
async fn a_restart_after_the_result_is_ready_still_delivers_the_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");
    {
        let mut provider = provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(
                channel_id,
                &ci_create_event(&provider, channel_id, "create-1"),
            )
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(
                    channel_id,
                    "cic-1",
                    &target,
                    &identity,
                    "carry on",
                    now_secs() + 3_600,
                ),
            )
            .await
            .expect("register");
        // The verified result is persisted, and the process dies before the
        // turn reaches any mailbox.
        let result = CiResult {
            schema: CI_RESULT_SCHEMA.into(),
            identity: identity.clone(),
            conclusion: CiConclusion::Success,
            evidence_url: None,
            summary: None,
        };
        provider
            .ci_continuations
            .mark_ready(
                &digest_of(&identity),
                &ReadyResult {
                    result_event_id: "fe".repeat(32),
                    result_signer: RELAY_SELF.to_owned(),
                    result_canonical_json: serde_json::to_string(&result).expect("encode"),
                    observed_at: now_secs(),
                },
            )
            .expect("ready");
    }

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    // A session record with no live actor after a restart answers the turn
    // terminally rather than running it — the honest outcome, and the one the
    // command ledger records exactly once.
    restarted.run_ci_continuation_tick().await.expect("tick");
    pump_available(&mut restarted).await;

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let stages = receipt_stages(&sink, "cic-1");
    assert!(
        !stages.is_empty(),
        "a ready continuation is answered after a restart, not lost"
    );
    assert!(
        restarted.ci_continuations.pending_count() <= 1,
        "and it never multiplies"
    );
}

/// Startup reconciliation: a registration whose command id already reached the
/// ledgers is dropped, because those ledgers are the fence.
#[tokio::test]
async fn recovery_drops_a_registration_whose_command_is_already_answered() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");
    {
        let mut provider = provider(&state_dir, Some(&projects));
        provider
            .handle_command_event(
                channel_id,
                &ci_create_event(&provider, channel_id, "create-1"),
            )
            .await
            .expect("create");
        let target = provider
            .state()
            .sessions()
            .next()
            .expect("session")
            .target("instance-1");
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(
                    channel_id,
                    "cic-1",
                    &target,
                    &identity,
                    "carry on",
                    now_secs() + 3_600,
                ),
            )
            .await
            .expect("register");
        // The turn ran; the crash happened before the record was retired.
        provider
            .state
            .consume_command("cic-1", now_secs())
            .expect("consume");
    }

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    assert!(
        restarted.ci_continuations.record("cic-1").is_none(),
        "the command ledger is the fence; the record has nothing left to promise"
    );
}

#[tokio::test]
async fn simultaneous_ci_start_permissions_reserve_the_shared_turn_allowance() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, first, _projects) = provider_with_session(dir.path()).await;
    provider
        .handle_command_event(
            channel_id,
            &ci_create_event(&provider, channel_id, "create-2"),
        )
        .await
        .expect("second actor");
    let second = provider
        .state
        .sessions()
        .find(|record| record.command_id == "create-2")
        .expect("second session")
        .target("instance-1");
    provider
        .state
        .claim_umbrella_founder("ci-budget", &Keys::generate().public_key().to_hex())
        .expect("umbrella founder");
    for target in [&first, &second] {
        provider
            .state
            .update_session(&target.session_id, |record| {
                record.session_ref = Some("ci-budget".into())
            })
            .expect("same umbrella");
    }
    provider.config.turn_budget = 1;
    for (id, target, run) in [
        ("ci-budget-1", &first, "136"),
        ("ci-budget-2", &second, "137"),
    ] {
        let identity = ci_identity(run);
        provider
            .handle_command_event(
                channel_id,
                &continuation_event(channel_id, id, target, &identity, "go", now_secs() + 3_600),
            )
            .await
            .expect("register");
        provider
            .handle_ci_listener_event(ready_report(&identity, CiConclusion::Success))
            .await
            .expect("queue");
    }
    assert!(provider
        .admit_ci_turn_start(&first.session_id, "ci-budget-1")
        .expect("first start permission"));
    assert_eq!(
        provider.state.turns_used("ci-budget"),
        0,
        "permission alone is not reported as a spent turn"
    );
    assert!(!provider
        .admit_ci_turn_start(&second.session_id, "ci-budget-2")
        .expect("second refused"));
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "ci-budget-2").as_deref(),
        Some("BUDGET_EXHAUSTED")
    );
    provider
        .handle_session_event(SessionEvent::TurnStarted {
            session_id: first.session_id,
            turn_id: "budget-turn".into(),
            command_id: "ci-budget-1".into(),
            text: "go".into(),
        })
        .expect("started report");
    assert_eq!(provider.state.turns_used("ci-budget"), 1);
    assert!(!provider.in_flight.contains_key("ci-budget-1"));
}

/// The actor cannot receive start permission until both durable fences exist.
#[tokio::test]
async fn a_ci_start_claim_is_durable_before_prompt_permission_and_survives_a_crash() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut running, channel_id, target, projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut running, channel_id, &target).await;
    running
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("queue");
    // Simulate the boundary handler's durable half, then lose the process
    // before its yes can reach the actor. The execution can be lost; its
    // command and operation must never be re-admitted on recovery.
    assert!(running
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect("claim"));
    assert!(running.state.is_command_consumed("cic-expiry"));
    // The record survives the claim in `claimed` rather than vanishing, so a
    // crash in this window can be told apart from a delivered turn. It is
    // removed at `TurnStarted`, or answered at boot — see
    // `a_claim_lost_to_a_crash_is_reported_once_and_never_re_admitted`.
    assert!(matches!(
        running
            .ci_continuations
            .record("cic-expiry")
            .map(|record| &record.state),
        Some(RecordState::Claimed { .. })
    ));
    let operations = ledger_lines(&dir.path().join("state"), "operations.jsonl");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["commandId"], "cic-expiry");
    drop(running);
    let mut restarted = provider(&dir.path().join("state"), Some(&projects));
    restarted.recover().expect("recover");
    assert!(restarted.state.is_command_consumed("cic-expiry"));
    assert!(!restarted
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect("retry denied"));
    assert!(restarted.ci_continuations.pending_identities().is_empty());
}

#[tokio::test]
async fn the_delivered_turn_writes_the_operation_ledger_before_the_command_ledger() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let state_dir = dir.path().join("state");
    let identity = ci_identity("136");
    let _ = register_and_resolve(
        &mut provider,
        channel_id,
        &target,
        "cic-1",
        &identity,
        CiConclusion::Success,
    )
    .await;

    let operations = ledger_lines(&state_dir, "operations.jsonl");
    assert_eq!(operations.len(), 1);
    assert_eq!(operations[0]["commandId"], "cic-1");
    assert!(ledger_lines(&state_dir, "commands.jsonl")
        .iter()
        .any(|row| row["commandId"] == "cic-1"));
    assert!(
        provider.ci_continuations.record("cic-1").is_none(),
        "and the record is retired only after both"
    );
}

/// A verified result for a digest nothing is waiting on is not a fact about
/// this provider at all.
#[tokio::test]
async fn a_result_for_an_unwatched_digest_wakes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, _target, _projects) = provider_with_session(dir.path()).await;
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("999"), CiConclusion::Success))
        .await
        .expect("stray result");
    pump_available(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .iter()
            .all(|receipt| receipt["commandId"] == "create-1"),
        "a stray result produces no receipt of its own"
    );
}

// -------------------------------------------------------------------------
// Failure-safe dispositions
//
// Every test below injects a one-shot I/O failure at a named boundary. The
// subject is not that the provider survives — it is *what the operator is left
// holding* when one half of a two-write disposition fails. The rule these pin:
// the visible answer is queued into the crash-safe outbox first, and the
// durable "already answered" fence is written second, because a lost fence
// costs at most a duplicate of an identical receipt while a lost receipt costs
// the operator any answer at all, permanently.
// -------------------------------------------------------------------------

/// A refusal-ledger failure must not swallow the refusal.
///
/// The receipt is already in the outbox when the ledger write fails, so the
/// operator is told; the command stays unrefused, so the next pass re-derives
/// the same disposition; and the outbox's semantic key makes that replay add
/// nothing rather than answer twice.
#[tokio::test]
async fn a_refusal_receipt_survives_a_failing_refusal_ledger_append() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    let state_dir = dir.path().join("state");
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire");

    provider.state.fault_plan().fail_next_refusal_append = true;
    provider.run_ci_continuation_tick().await.expect("tick");

    assert!(
        !provider.state.is_command_refused("cic-expiry"),
        "the fence is not claimed when its write failed"
    );
    assert!(
        ledger_lines(&state_dir, "refusals.jsonl")
            .iter()
            .all(|row| row["commandId"] != "cic-expiry"),
        "and nothing reached the refusal ledger on disk"
    );
    assert!(
        provider
            .ci_continuations
            .record("cic-expiry")
            .expect("the record is still there")
            .state
            .is_pending(),
        "an unfenced refusal leaves the registration answerable"
    );

    // The replay re-derives the identical refusal. This time the ledger takes
    // it, and the queued receipt is fenced rather than duplicated.
    provider.run_ci_continuation_tick().await.expect("replay");
    assert!(provider.state.is_command_refused("cic-expiry"));

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "cic-expiry"),
        vec![
            "continuation_registered".to_owned(),
            "turn_refused".to_owned()
        ],
        "exactly one refusal reaches the relay"
    );
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_CONTINUATION_EXPIRED")
    );
}

/// The other half of the same order: if the *visible* half fails, nothing is
/// recorded at all, so the command is still answerable on the next pass.
#[tokio::test]
async fn a_failing_outbox_enqueue_leaves_the_command_unrefused_and_unconsumed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir.path()).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire");

    provider.outbox.fail_next_enqueue();
    provider.run_ci_continuation_tick().await.expect("tick");

    assert!(!provider.state.is_command_refused("cic-expiry"));
    assert!(!provider.state.is_command_consumed("cic-expiry"));
    assert!(
        provider
            .ci_continuations
            .record("cic-expiry")
            .expect("the record survives an unanswered pass")
            .state
            .is_pending(),
        "nothing is terminal until the operator has been told"
    );

    provider.run_ci_continuation_tick().await.expect("retry");
    assert!(provider.state.is_command_refused("cic-expiry"));
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        refusal_code(&sink, "cic-expiry").as_deref(),
        Some("CI_CONTINUATION_EXPIRED"),
        "the disposition survives the failed attempt"
    );
}

/// The actor's overflow path takes the same order, and is proven the same way:
/// the drop is visible to the transcript reader and to the operator even when
/// the refusal ledger refuses the write that follows.
#[tokio::test]
async fn a_dropped_turn_publishes_its_receipt_even_when_the_refusal_ledger_fails() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, target, _projects) = provider_with_session(dir.path()).await;

    provider.state.fault_plan().fail_next_refusal_append = true;
    let error = provider
        .handle_session_event(SessionEvent::TurnDropped {
            session_id: target.session_id.clone(),
            command_id: "turn-overflow".into(),
        })
        .expect_err("the ledger write fails after the receipt is queued");
    assert!(
        error.to_string().contains("injected refusal-ledger append"),
        "{error}"
    );
    assert!(!provider.state.is_command_refused("turn-overflow"));

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert_eq!(
        receipt_stages(&sink, "turn-overflow"),
        vec!["turn_dropped".to_owned()],
        "the operator is told the turn was dropped"
    );
    assert!(
        transcript_items_in_sequence(&sink)
            .iter()
            .any(|item| item["item"]["status"] == "turn_dropped:queue_full"),
        "and so is a reader of the transcript"
    );
}

/// A registration that has been delivered and is holding its mailbox slot,
/// one step before the actor asks for start permission.
async fn delivered_ci_turn_in_flight(dir: &Path) -> (Provider, CodingSessionTarget) {
    let (mut provider, channel_id, target, _projects) = provider_with_session(dir).await;
    register_expiry_fixture(&mut provider, channel_id, &target).await;
    provider
        .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
        .await
        .expect("queue the delivery");
    assert!(
        provider.in_flight.contains_key("cic-expiry"),
        "the delivered turn holds mailbox custody until its start is decided"
    );
    (provider, target)
}

/// Start permission is durable in the operation and command ledgers before the
/// record is touched, so a store failure at that point cannot honestly deny the
/// actor: the turn is already consumed. It is logged, the actor is permitted,
/// and the record still reads `claimed` so this process will not re-deliver it.
#[tokio::test]
async fn a_failed_claim_write_still_permits_the_actor_and_the_record_reads_claimed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, target) = delivered_ci_turn_in_flight(dir.path()).await;

    provider.ci_continuations.fail_next_write();
    assert!(
        provider
            .admit_ci_turn_start(&target.session_id, "cic-expiry")
            .expect("the claim is durable, so the answer is yes"),
        "denying a turn whose ledgers already consumed it would be a false answer"
    );
    assert!(provider.state.is_command_consumed("cic-expiry"));
    assert!(matches!(
        provider
            .ci_continuations
            .record("cic-expiry")
            .map(|record| &record.state),
        Some(RecordState::Claimed { .. })
    ));
    assert!(
        provider
            .ci_continuations
            .ready_for_delivery(now_secs())
            .is_empty(),
        "a claimed record is not offered to another delivery pass"
    );
}

/// The invariant the wrapper exists for: no denial, however it fails, may
/// leave the command occupying its target's mailbox slot.
#[tokio::test]
async fn every_denial_path_releases_the_mailbox_slot_even_when_its_write_fails() {
    // The expiry branch, with the refusal ledger refusing.
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, target) = delivered_ci_turn_in_flight(dir.path()).await;
    provider
        .ci_continuations
        .set_expiry_for_test("cic-expiry", now_secs() - 1)
        .expect("expire");
    provider.state.fault_plan().fail_next_refusal_append = true;
    let error = provider
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect_err("the refusal ledger fails");
    assert!(
        error.to_string().contains("injected refusal-ledger append"),
        "{error}"
    );
    assert!(
        !provider.in_flight.contains_key("cic-expiry"),
        "an expiry that could not be fenced still releases the slot"
    );

    // The project-mismatch branch, with the refusal ledger refusing.
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, target) = delivered_ci_turn_in_flight(dir.path()).await;
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.project_ref = Some(format!("30621:{}:elsewhere", "22".repeat(32)));
        })
        .expect("retarget the execution's project");
    provider.state.fault_plan().fail_next_refusal_append = true;
    provider
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect_err("the refusal ledger fails");
    assert!(
        !provider.in_flight.contains_key("cic-expiry"),
        "a mismatch that could not be fenced still releases the slot"
    );

    // The branch that writes nothing at all: the record is simply gone.
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, target) = delivered_ci_turn_in_flight(dir.path()).await;
    provider
        .ci_continuations
        .remove("cic-expiry")
        .expect("drop the record");
    assert!(!provider
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect("denied"));
    assert!(!provider.in_flight.contains_key("cic-expiry"));
}

/// The loss the `claimed` state exists to make visible.
///
/// A crash between the durable claim and the adapter prompt cannot be undone —
/// the operation and command ledgers are first-writer-wins, so nothing will
/// ever spend that claim. Before this state existed the record was removed at
/// claim time and the loss was indistinguishable from success: the sender
/// waited forever on a turn that had already been thrown away. Now startup
/// says so, exactly once.
#[tokio::test]
async fn a_claim_lost_to_a_crash_is_reported_once_and_never_re_admitted() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let projects = {
        let (mut running, channel_id, target, projects) = provider_with_session(dir.path()).await;
        register_expiry_fixture(&mut running, channel_id, &target).await;
        running
            .handle_ci_listener_event(ready_report(&ci_identity("136"), CiConclusion::Success))
            .await
            .expect("queue");
        assert!(running
            .admit_ci_turn_start(&target.session_id, "cic-expiry")
            .expect("claim"));
        // The process dies here: claimed, never started.
        (projects, target)
    };
    let (projects, target) = projects;

    let mut restarted = provider(&state_dir, Some(&projects));
    restarted.recover().expect("recover");
    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");

    let dropped: Vec<serde_json::Value> = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .filter(|receipt| {
            receipt["commandId"] == "cic-expiry" && receipt["status"] == "turn_dropped"
        })
        .collect();
    assert_eq!(dropped.len(), 1, "the lost delivery is reported once");
    assert_eq!(
        dropped[0]["error"]["code"],
        crate::ci_continuation::LOST_AFTER_CLAIM
    );
    assert!(restarted.ci_continuations.record("cic-expiry").is_none());
    assert!(
        restarted.state.is_command_consumed("cic-expiry"),
        "the claim stays spent — the report is an answer, not a release"
    );
    assert!(
        !restarted
            .admit_ci_turn_start(&target.session_id, "cic-expiry")
            .expect("no second admission"),
        "a reported loss must not become a second delivery"
    );

    // A second restart has nothing left to say.
    drop(restarted);
    let mut again = provider(&state_dir, Some(&projects));
    again.recover().expect("recover");
    let second = CollectingSink::new();
    again.flush(&second).await.expect("flush");
    assert!(
        second
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .iter()
            .all(|receipt| receipt["error"]["code"] != crate::ci_continuation::LOST_AFTER_CLAIM),
        "the drop is not republished on every subsequent boot"
    );
}

/// And the ordinary path, so the state above cannot silently become permanent:
/// the turn starts, the record is retired, and nothing is ever reported lost.
#[tokio::test]
async fn a_claimed_record_is_retired_when_the_turn_actually_starts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, target) = delivered_ci_turn_in_flight(dir.path()).await;
    assert!(provider
        .admit_ci_turn_start(&target.session_id, "cic-expiry")
        .expect("claim"));
    assert!(matches!(
        provider
            .ci_continuations
            .record("cic-expiry")
            .map(|record| &record.state),
        Some(RecordState::Claimed { .. })
    ));

    provider
        .handle_session_event(SessionEvent::TurnStarted {
            session_id: target.session_id.clone(),
            turn_id: "claimed-turn".into(),
            command_id: "cic-expiry".into(),
            text: "carry on".into(),
        })
        .expect("started report");

    assert!(
        provider.ci_continuations.record("cic-expiry").is_none(),
        "the promise is kept, so the record has nothing left to say"
    );
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let stages = receipt_stages(&sink, "cic-expiry");
    assert!(stages.contains(&"turn_started".to_owned()), "{stages:?}");
    assert!(!stages.contains(&"turn_dropped".to_owned()), "{stages:?}");
}
