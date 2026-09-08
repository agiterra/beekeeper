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
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
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
}

/// A bounded store refuses rather than displacing a promise somebody is
/// already waiting on.
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
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
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
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
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
            .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
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

/// The ledger order at `TurnStarted` is the thing that degrades well:
/// operation, then command, then the durable promise.
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
