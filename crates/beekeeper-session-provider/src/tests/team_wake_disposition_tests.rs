//! Ledger control run 7: a verifier's `Verdict::Disposition` is the
//! assigner's cue that a held-open report has been ruled on, so it must
//! reach the wake queue exactly as a report does. Before this file, both
//! `Provider::on_team_transaction` and `Provider::discover_team_wake_partition_for`
//! only ever produced a wake candidate for `CodingSessionTeamTransactionBody::
//! Report`; a disposition fell to `pending_completion::settlement_fact`,
//! which only recognises an *approving* disposition that asks the assignee
//! for nothing, and even that classification never mints a wake — it only
//! asks the fold to re-read the channel. A disposition carrying
//! `required_action` matched neither arm and produced nothing at all,
//! exactly the defect a live verifier hit and worked around by messaging the
//! lead by hand.

use super::*;

use beekeeper_core::coding_session_team_transaction::{
    CodingSessionTeamDispositionDecision, CodingSessionTeamRefutationDecision,
    CodingSessionTeamTransactionBody, CodingSessionTeamVerdict,
};
use beekeeper_sdk::coding_session_team_transaction::{
    build_coding_session_team_transaction, coding_session_team_transaction_payload,
};

fn id(byte: &str) -> String {
    byte.repeat(32)
}

fn signed_disposition(
    channel_id: Uuid,
    session_ref: &str,
    genesis_ref: &str,
    decision: CodingSessionTeamDispositionDecision,
    required_action: Option<String>,
    signer: &Keys,
) -> nostr::Event {
    let payload = coding_session_team_transaction_payload(
        session_ref.to_owned(),
        genesis_ref.to_owned(),
        None,
        None,
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Disposition {
            assignment_ref: id("cd"),
            report_ref: id("ef"),
            refutation_ref: None,
            decision,
            summary: "Ruled on the report".into(),
            findings: Vec::new(),
            required_action,
        }),
    );
    build_coding_session_team_transaction(&channel_id.to_string(), payload)
        .expect("the disposition builds")
        .sign_with_keys(signer)
        .expect("the disposition signs")
}

fn signed_refutation(
    channel_id: Uuid,
    session_ref: &str,
    genesis_ref: &str,
    signer: &Keys,
) -> nostr::Event {
    let payload = coding_session_team_transaction_payload(
        session_ref.to_owned(),
        genesis_ref.to_owned(),
        None,
        None,
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            assignment_ref: id("cd"),
            report_ref: id("ef"),
            decision: CodingSessionTeamRefutationDecision::NotRefuted,
            summary: "No refutation found".into(),
            findings: Vec::new(),
            required_action: None,
        }),
    );
    build_coding_session_team_transaction(&channel_id.to_string(), payload)
        .expect("the refutation builds")
        .sign_with_keys(signer)
        .expect("the refutation signs")
}

fn bare_provider(state_dir: &std::path::Path) -> Provider {
    Provider::new(config_of(
        Keys::generate(),
        state_dir,
        None,
        "missing-agent".into(),
    ))
    .expect("provider")
}

/// (a) An approving disposition wakes the assignment author exactly once:
/// one admitted intent appears in the channel's wake queue.
#[test]
fn an_approving_disposition_wakes_the_assignment_author_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = bare_provider(&dir.path().join("state"));
    let channel_id = Uuid::new_v4();
    let session_ref = Uuid::new_v4().to_string();
    let genesis_ref = "ab".repeat(32);
    let verifier = Keys::generate();
    let disposition = signed_disposition(
        channel_id,
        &session_ref,
        &genesis_ref,
        CodingSessionTeamDispositionDecision::Approve,
        None,
        &verifier,
    );

    provider
        .on_team_transaction(channel_id, &disposition)
        .expect("on_team_transaction");

    let (resolved, admitted, in_flight, _terminals) =
        provider.team_wakes.channel_counts(channel_id);
    assert_eq!(
        resolved + admitted + in_flight,
        1,
        "exactly one wake intent is queued for the approving disposition"
    );
}

/// (b) A disposition carrying `required_action` — the exact shape that
/// previously matched neither `team_report_candidate` nor `settlement_fact`
/// and produced no wake at all — also wakes once.
#[test]
fn a_disposition_with_required_action_wakes_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = bare_provider(&dir.path().join("state"));
    let channel_id = Uuid::new_v4();
    let session_ref = Uuid::new_v4().to_string();
    let genesis_ref = "ab".repeat(32);
    let verifier = Keys::generate();
    let disposition = signed_disposition(
        channel_id,
        &session_ref,
        &genesis_ref,
        CodingSessionTeamDispositionDecision::ChangesRequested,
        Some("Address the open residual before landing.".into()),
        &verifier,
    );

    provider
        .on_team_transaction(channel_id, &disposition)
        .expect("on_team_transaction");

    let (resolved, admitted, in_flight, _terminals) =
        provider.team_wakes.channel_counts(channel_id);
    assert_eq!(
        resolved + admitted + in_flight,
        1,
        "a disposition requiring action is no longer silently dropped"
    );
}

/// (c) A refutation remains the verifier's own silent next step: it never
/// reaches the wake queue.
#[test]
fn a_refutation_wakes_nobody() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = bare_provider(&dir.path().join("state"));
    let channel_id = Uuid::new_v4();
    let session_ref = Uuid::new_v4().to_string();
    let genesis_ref = "ab".repeat(32);
    let verifier = Keys::generate();
    let refutation = signed_refutation(channel_id, &session_ref, &genesis_ref, &verifier);

    provider
        .on_team_transaction(channel_id, &refutation)
        .expect("on_team_transaction");

    let (resolved, admitted, in_flight, _terminals) =
        provider.team_wakes.channel_counts(channel_id);
    assert_eq!(
        resolved + admitted + in_flight,
        0,
        "a refutation mints no wake intent"
    );
}

/// (d) Replaying the identical disposition event — the relay redelivering
/// it, or the offline-catchup scan seeing what the live path already saw —
/// is a no-op: still exactly one queued intent, never two.
#[test]
fn replaying_the_same_disposition_wakes_nobody_twice() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = bare_provider(&dir.path().join("state"));
    let channel_id = Uuid::new_v4();
    let session_ref = Uuid::new_v4().to_string();
    let genesis_ref = "ab".repeat(32);
    let verifier = Keys::generate();
    let disposition = signed_disposition(
        channel_id,
        &session_ref,
        &genesis_ref,
        CodingSessionTeamDispositionDecision::Approve,
        None,
        &verifier,
    );

    provider
        .on_team_transaction(channel_id, &disposition)
        .expect("first delivery");
    provider
        .on_team_transaction(channel_id, &disposition)
        .expect("replayed delivery");

    let (resolved, admitted, in_flight, _terminals) =
        provider.team_wakes.channel_counts(channel_id);
    assert_eq!(
        resolved + admitted + in_flight,
        1,
        "the second delivery of the same disposition id is a no-op"
    );

    // The same dedup must survive retirement into the permanent ledger, not
    // just admission: once an intent resolves, the id moves to `resolved`
    // and a later replay must still be recognised there.
    provider
        .team_wakes
        .pending_for_channel(channel_id)
        .expect("promote admitted intent to in-flight")
        .expect("an intent is pending");
    provider
        .team_wakes
        .retire_in_flight(channel_id)
        .expect("retire in-flight intent");
    provider
        .on_team_transaction(channel_id, &disposition)
        .expect("delivery after retirement");
    let (resolved, admitted, in_flight, _terminals) =
        provider.team_wakes.channel_counts(channel_id);
    assert_eq!(
        resolved + admitted + in_flight,
        1,
        "a disposition already resolved is still recognised as a duplicate"
    );
}

/// `WakeSource::Disposition` mints the exact identifier-only text a disposition
/// wake carries: the disposition id, the report id, the settled assignment
/// id, and whether the ruling approves or requires action — so the lead
/// needs no lookup to act on it.
#[test]
fn disposition_wake_text_names_ids_and_whether_action_is_required() {
    let source = team_wake::WakeSource::Disposition {
        operation_id: "11".repeat(32),
        report_ref: "22".repeat(32),
        assignment_ref: "33".repeat(32),
        decision: CodingSessionTeamDispositionDecision::ChangesRequested,
        required_action: Some("Fix the failing test.".into()),
        author_pubkey: "44".repeat(32),
        created_at: 1,
    };
    let text = team_wake::wake_text(&source).expect("wake text encodes");
    let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(value["operationId"], "11".repeat(32));
    assert_eq!(value["reportRef"], "22".repeat(32));
    assert_eq!(value["assignmentRef"], "33".repeat(32));
    assert_eq!(value["decision"], "changes-requested");
    assert_eq!(value["requiredAction"], "Fix the failing test.");
    assert!(
        team_wake::is_team_wake_pointer(&text),
        "the disposition pointer is recognised as this provider's own wake text"
    );
}
