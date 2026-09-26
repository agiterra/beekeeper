//! A durable CI promise meets the handover fence.
//!
//! `thread.turn.continue_on_ci` is the one turn nobody sends: a registration
//! made hours earlier, held durably, and materialized into a prompt by the
//! provider itself when a verified CI result arrives. That makes it the path
//! most likely to walk around an admission rule — it does not re-decode a
//! signed command, and it starts a turn on a target whose operator is
//! somewhere else entirely.
//!
//! It does not walk around this one, and these tests say why: the CI path
//! re-runs [`crate::commands::decide_turn_command`] at **delivery**, not at
//! registration, so a claim accepted while CI was running is in force by the
//! time the result lands. The registration then ends the way §3 requires —
//! `HANDOVER_FENCED`, durably, once — instead of waking an agent on a session
//! somebody else has taken over.
//!
//! `ci_continuation.rs` is not this lane's file and is not edited; that the
//! fence reaches it at all is the property under test.

use super::*;

use crate::ci_continuation_store::ReadyResult;
use crate::payload::HANDOVER_FENCED;
use buzz_core::ci_result::{
    correlation_id, CiConclusion, CiPhase, CiResult, CiResultIdentity, CI_RESULT_SCHEMA,
};
use buzz_core::coding_session_authority_claim::{ClaimState, CurrentClaim};
use buzz_core::coding_session_command::{
    CodingSessionAction, CodingSessionCommandPayload, CODING_SESSION_COMMAND_SCHEMA,
};

fn identity() -> CiResultIdentity {
    CiResultIdentity {
        project: format!("30621:{}:beekeeper", "11".repeat(32)),
        repository: format!("30617:{}:beekeeper", "11".repeat(32)),
        commit: "ab".repeat(20),
        check: "main-validation".into(),
        run: "204".into(),
        attempt: 1,
        workflow: "6f1a2f1e-0b3c-4a5d-8e9f-0a1b2c3d4e5f".into(),
        phase: CiPhase::Build,
    }
}

fn digest() -> String {
    correlation_id(&identity()).expect("valid identity digest")
}

/// A verified relay-signed result, as the listener would hand it over.
fn verified_result(signer: &str) -> ReadyResult {
    let result = CiResult {
        schema: CI_RESULT_SCHEMA.into(),
        identity: identity(),
        conclusion: CiConclusion::Success,
        evidence_url: None,
        summary: Some("gate output".into()),
    };
    ReadyResult {
        result_event_id: "fe".repeat(32),
        result_signer: signer.to_owned(),
        result_canonical_json: serde_json::to_string(&result).expect("encode result"),
        observed_at: now_secs(),
    }
}

/// A signed `thread.turn.continue_on_ci`, as the CLI publishes it.
fn registration(channel_id: Uuid, command_id: &str, target: &CodingSessionTarget) -> Event {
    let payload = CodingSessionCommandPayload {
        schema: CODING_SESSION_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.to_owned(),
        target: target.clone(),
        action: CodingSessionAction::ThreadTurnContinueOnCi {
            identity: identity(),
            continuation: "open a PR with the fix".to_owned(),
            expires_at: now_secs() + 3_600,
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

/// A create inside the CI project, so the registration's project matches.
fn create_in_ci_project(provider: &Provider, channel_id: Uuid, command_id: &str) -> Event {
    let source = create_event(provider, channel_id, command_id);
    let mut content: serde_json::Value =
        serde_json::from_str(&source.content).expect("create JSON");
    content["action"]["projectRef"] = identity().project.into();
    signed_lifecycle_event(channel_id, content.to_string())
}

/// A registered, ready CI continuation on a live execution, plus the claim
/// that fences it.
///
/// The claim is written onto the record directly rather than folded out of a
/// chain: which links produce which [`ClaimState`] is settled in
/// `handover_fence_tests.rs` and in `buzz-core`, and what this file is about
/// is whether the CI path reads the answer at all.
async fn fenced_ready_continuation(
    dir: &Path,
    ready: bool,
) -> (Provider, Uuid, CodingSessionTarget, String) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_project_checkout(dir, channel_id, &cwd, &identity().project);
    let mut provider = provider(&dir.join("state"), Some(&projects));

    let create = create_in_ci_project(&provider, channel_id, "create-ci");
    provider
        .handle_command_event(channel_id, &create)
        .await
        .expect("create");
    let record = provider.state().sessions().next().expect("session").clone();
    let target = record.target(&provider.config.instance_id);

    provider
        .handle_command_event(channel_id, &registration(channel_id, "cic-1", &target))
        .await
        .expect("register");
    // A caller that will drive the *listener* leaves the record waiting, so
    // the result it delivers is the one that moves it; a caller that tests
    // admission directly needs it already `Ready`.
    if ready {
        let signer = test_operator_keys().public_key().to_hex();
        provider
            .ci_continuations
            .mark_ready(&digest(), &verified_result(&signer))
            .expect("the verified result is durable");
    }

    // Meanwhile, somebody took the umbrella over on another machine.
    let claimant = "bb".repeat(32);
    provider
        .state
        .update_session(&target.session_id, |record| {
            record.handover = ClaimState::Active(CurrentClaim {
                claimant: claimant.clone(),
                body_pubkey: "dd".repeat(32),
                accepted_event_id: "11".repeat(32),
                seq: 1,
            });
        })
        .expect("fence the record");

    (provider, channel_id, target, claimant)
}

/// The admission wrapper the CI path calls before an actor is permitted to
/// start. It runs `decide_turn_command`, so it inherits the fence — and the
/// refusal it records is the one an operator reads.
#[tokio::test]
async fn a_ready_ci_registration_on_a_fenced_record_is_refused_by_name() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, target, claimant) =
        fenced_ready_continuation(dir.path(), true).await;

    let admitted = provider
        .admit_ci_turn_start(&target.session_id, "cic-1")
        .expect("admission decides");
    assert!(!admitted, "a fenced record permits no CI turn");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    // The registration's own `continuation_registered` acknowledgement stands;
    // what must appear exactly once is the *outcome*.
    let outcomes: Vec<serde_json::Value> = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .filter(|receipt| {
            receipt["commandId"] == "cic-1" && receipt["status"] != "continuation_registered"
        })
        .collect();
    assert_eq!(
        outcomes.len(),
        1,
        "exactly one answer, recorded once: {outcomes:?}"
    );
    let receipts = outcomes;
    assert_eq!(receipts[0]["status"], "turn_refused");
    assert_eq!(receipts[0]["error"]["code"], HANDOVER_FENCED);
    assert!(
        receipts[0]["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&claimant)),
        "the refusal names who holds the session: {}",
        receipts[0]
    );
    assert!(
        provider.state().is_command_refused("cic-1"),
        "the refusal is in the durable ledger, so a redelivery cannot republish it"
    );
}

/// The same answer through the whole live path: the verified result arrives on
/// the listener, delivery re-decides, and the promise ends visibly rather than
/// prompting an agent about a session it no longer holds.
#[tokio::test]
async fn a_verified_ci_result_for_a_fenced_record_starts_no_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, target, _claimant) =
        fenced_ready_continuation(dir.path(), false).await;
    let signer = test_operator_keys().public_key().to_hex();
    let ready = verified_result(&signer);

    provider
        .handle_ci_listener_event(crate::ci_result_listener::CiListenerEvent::CiResultReady {
            digest: digest(),
            event_id: ready.result_event_id,
            signer: ready.result_signer,
            canonical_json: ready.result_canonical_json,
            observed_at: ready.observed_at,
        })
        .await
        .expect("the verified result arrives");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let stages = receipt_stages(&sink, "cic-1");
    assert!(
        !stages.iter().any(|stage| stage == "turn_started"),
        "no turn may start on a fenced record: {stages:?}"
    );
    assert!(
        sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .filter(|receipt| receipt["commandId"] == "cic-1")
            .any(|receipt| receipt["error"]["code"] == HANDOVER_FENCED),
        "and the promise ends with the fence's own code"
    );
    assert!(
        provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .open_turn
            .is_none(),
        "nothing was left in flight"
    );
}
