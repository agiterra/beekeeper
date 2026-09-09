//! The handover fence: who may act on a session that has been handed over.
//!
//! Every test here is about one sentence from `docs/HANDOVER_IMPL.md` §3: once
//! an accepted `takeover` or `transfer` names a claimant and an execution
//! body, a turn, a resume or a provider-minted wake is admitted **only** on
//! that body and **only** from that claimant. Everyone else — including the
//! founder, including the machine that was running the work five minutes
//! ago — is refused by name.
//!
//! The failure this prevents is not an authorization leak. It is two live
//! executions of one task: A's machine comes back from an outage, replays the
//! turn it had queued, and now two agents are working the same branch with
//! nothing on the wire saying which one is real.
//!
//! Three shapes recur below and are worth naming once:
//!
//! - **The chain is built out of real events.** Each link is a signed 44228
//!   and each is applied through a relay-signed 40099 acceptance receipt
//!   verified against a witnessed relay identity, exactly as production folds
//!   them. Nothing here writes `record.handover` by hand, because the thing
//!   under test is the fold as much as the fence.
//! - **A refusal is durable before it is published.** Each fenced turn is
//!   checked in the refusal ledger as well as in the receipt, because a
//!   refusal that is published but not recorded republishes on redelivery.
//! - **The regrant regression is the point.** Revoking the claimant voids the
//!   claim; granting that same pubkey `grant-operator` again must leave it
//!   voided. A build that folded `Option<claim>` instead of three states
//!   passes every other test on this page and fails that one.

use super::*;

use crate::payload::{HANDOVER_FENCED, SESSION_RETIRED};
use buzz_core::coding_session_authority_claim::ClaimState;
use buzz_core::coding_session_authority_transition::{
    CodingSessionAuthorityTransitionPayload, CodingSessionAuthorityTransitionType,
};

/// A signed `takeover`: `claimant` claims the umbrella for execution body
/// `body`.
///
/// Signed by the claimant, which is what the relay requires of a self-claim;
/// [`authority::verify_accepted_transition`] does not bind a claim link's
/// signer to the session owner, so this is the shape that reaches the fold.
fn takeover_event(
    channel_id: Uuid,
    genesis_ref: &str,
    prev_accepted: Option<String>,
    seq: u32,
    claimant: &Keys,
    body_pubkey: &str,
) -> Event {
    let payload = CodingSessionAuthorityTransitionPayload::new_takeover(
        genesis_ref.to_owned(),
        prev_accepted,
        seq,
        claimant.public_key().to_hex(),
        body_pubkey.to_owned(),
    );
    buzz_sdk::builders::build_coding_session_authority_transition(channel_id, &payload)
        .expect("takeover builder")
        .sign_with_keys(claimant)
        .expect("sign takeover")
}

/// The relay-signed 40099 for a claim link, carrying its `bodyPubkey`.
///
/// Deliberately built here rather than imported from another test module: the
/// receipt is the artifact this lane depends on most, and a local copy makes
/// the exact bytes the provider must accept visible on this page.
fn claim_receipt(relay_keys: &Keys, channel_id: Uuid, transition: &Event) -> Event {
    let payload =
        buzz_core::coding_session_authority_transition::decode_coding_session_authority_transition(
            &transition.content,
        )
        .expect("transition payload");
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": authority::ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": payload.genesis_ref,
            "acceptedEventId": transition.id.to_hex(),
            "seq": payload.seq,
            "transitionType": payload.transition_type,
            "granteePubkey": payload.grantee_pubkey,
            "bodyPubkey": payload.body_pubkey,
        })
        .to_string(),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(relay_keys)
    .expect("sign claim receipt")
}

/// Everything a fence test needs: a provider, a governed record, a relay
/// serving the chain's events, and the keys that matter.
struct Umbrella {
    provider: Provider,
    relay: HarnessRelay,
    control: RecordingTestRelay,
    server: tokio::task::JoinHandle<()>,
    channel_id: Uuid,
    genesis_ref: String,
    target: CodingSessionTarget,
}

impl Umbrella {
    /// Store a signed transition on the relay and apply its accepted receipt,
    /// exactly as the live path does: the receipt arrives on the channel
    /// subscription, and the transition it names is resolved by explicit id.
    async fn apply(&mut self, transition: &Event, relay_keys: &Keys) {
        self.control
            .events
            .lock()
            .expect("relay events")
            .push(transition.clone());
        let receipt = claim_receipt(relay_keys, self.channel_id, transition);
        self.provider
            .handle_relay_event(&mut self.relay, self.channel_id, &receipt)
            .await
            .expect("apply accepted link");
    }

    fn claim(&self) -> ClaimState {
        self.provider
            .state()
            .session(&self.target.session_id)
            .expect("record")
            .handover
            .clone()
    }

    fn scope(&self) -> team_wake::WakeScope {
        team_wake::WakeScope {
            channel_ref: self.channel_id,
            session_ref: UMBRELLA_SESSION_REF.into(),
            genesis_ref: self.genesis_ref.clone(),
        }
    }

    async fn shutdown(self) {
        self.relay.shutdown().await;
        self.server.abort();
    }
}

/// The `sessionRef` [`governed_record`] mints. Named so the wake scope and the
/// record cannot drift apart silently.
const UMBRELLA_SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

/// One governed session on a provider whose keys the caller chose — because
/// half these tests need to know this provider's authority pubkey (the body a
/// claim either names or does not) *before* the chain is built.
async fn umbrella(
    dir: &Path,
    relay_keys: &Keys,
    provider_keys: Keys,
    channel_id: Uuid,
) -> Umbrella {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.join("state");
    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider =
        Provider::new(config_of(provider_keys.clone(), &state_dir, None, agent)).expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let genesis_ref = "ab".repeat(32);
    let record = governed_record(channel_id, &cwd, &genesis_ref);
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");
    let (relay, control, server) = spawn_recording_test_relay(&provider_keys, Vec::new()).await;
    provider.set_rest_client(relay.rest_client());
    Umbrella {
        provider,
        relay,
        control,
        server,
        channel_id,
        genesis_ref,
        target,
    }
}

/// The founder's `grant-operator` for `grantee` at `seq` — the standing a
/// claimant must already hold before the relay will accept its `takeover`.
fn grant_for(
    channel_id: Uuid,
    genesis_ref: &str,
    prev_accepted: Option<String>,
    seq: u32,
    grantee_hex: &str,
) -> Event {
    authority_transition_event(
        channel_id,
        genesis_ref,
        prev_accepted,
        seq,
        grantee_hex,
        CodingSessionAuthorityTransitionType::GrantOperator,
    )
}

/// Send `turn` and report the receipt stages and the error code it carried.
async fn send_turn(
    provider: &mut Provider,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    operator: &Keys,
) -> (Vec<String>, Option<String>) {
    let event = command_event_by(
        channel_id,
        command_id,
        target,
        serde_json::json!({ "type": "thread.turn.start", "text": "carry on" }),
        operator,
    );
    provider
        .handle_command_event(channel_id, &event)
        .await
        .expect("handle turn");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let code = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id)
        .and_then(|receipt| receipt["error"]["code"].as_str().map(str::to_owned));
    (receipt_stages(&sink, command_id), code)
}

// -------------------------------------------------------------------------
// An active claim on another body
// -------------------------------------------------------------------------

/// B takes the session over on B's own provider. A's provider — this one — is
/// no longer the body, so A's turn, A's resume, and A's own provider-minted
/// wake are all refused `HANDOVER_FENCED`, and the refusals are durable.
#[tokio::test]
async fn a_claim_on_another_body_fences_turns_resumes_and_wakes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let other_body = "dd".repeat(32);
    let channel_id = Uuid::new_v4();

    let mut umbrella = umbrella(dir.path(), &relay_keys, Keys::generate(), channel_id).await;
    let grant = grant_for(channel_id, &umbrella.genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &umbrella.genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    umbrella.apply(&grant, &relay_keys).await;
    umbrella.apply(&takeover, &relay_keys).await;

    let claim = umbrella.claim();
    let active = claim.active().expect("the claim stands");
    assert_eq!(active.claimant, claimant_hex);
    assert_eq!(active.body_pubkey, other_body);

    // The founder's turn, on the body that no longer holds the session.
    let (stages, code) = send_turn(
        &mut umbrella.provider,
        channel_id,
        "turn-after-takeover",
        &umbrella.target,
        test_operator_keys(),
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(code.as_deref(), Some(HANDOVER_FENCED));
    assert!(
        umbrella
            .provider
            .state()
            .is_command_refused("turn-after-takeover"),
        "a fenced turn must be recorded in the durable refusal ledger before it is published"
    );

    // The founder's resume, same answer, naming who holds it and where.
    let decision = lifecycle_decision_by(
        &umbrella.provider,
        channel_id,
        "resume-after-takeover",
        "session.resume",
        &umbrella.target,
        test_operator_keys(),
    );
    match decision {
        LifecycleDecision::Fail { code, message, .. } => {
            assert_eq!(code, HANDOVER_FENCED);
            assert!(
                message.contains(&claimant_hex) && message.contains(&other_body),
                "the refusal must name the claimant and the body: {message}"
            );
        }
        other => panic!("expected a fenced resume, got {other:?}"),
    }

    // And this provider's own wake — the one case that proceeds on the
    // *claimed* body, and must not here.
    let refusal = umbrella
        .provider
        .team_wake_fence(&umbrella.scope())
        .expect("a wake on a claimed-elsewhere umbrella is fenced");
    assert_eq!(refusal.code, HANDOVER_FENCED);

    umbrella.shutdown().await;
}

// -------------------------------------------------------------------------
// An active claim on this body
// -------------------------------------------------------------------------

/// The claim names *this* provider. The claimant may steer; the founder — who
/// created the session and has never lost a grant — may not, and is told why.
/// A wake this provider mints for itself proceeds, because the claimant chose
/// this machine to carry the work.
#[tokio::test]
async fn a_claim_on_this_body_admits_only_the_claimant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let channel_id = Uuid::new_v4();

    let mut umbrella = umbrella(dir.path(), &relay_keys, provider_keys, channel_id).await;
    let grant = grant_for(channel_id, &umbrella.genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &umbrella.genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &body,
    );
    umbrella.apply(&grant, &relay_keys).await;
    umbrella.apply(&takeover, &relay_keys).await;

    // The founder is now a bystander on their own session.
    let (stages, code) = send_turn(
        &mut umbrella.provider,
        channel_id,
        "founder-turn",
        &umbrella.target,
        test_operator_keys(),
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(code.as_deref(), Some(HANDOVER_FENCED));

    // The claimant is not fenced. This record has no live process, so the turn
    // ends `turn_dropped/NO_LIVE_EXECUTION`; what matters is that the fence is
    // not what answered it.
    let (stages, code) = send_turn(
        &mut umbrella.provider,
        channel_id,
        "claimant-turn",
        &umbrella.target,
        &claimant,
    )
    .await;
    assert_ne!(
        code.as_deref(),
        Some(HANDOVER_FENCED),
        "the claimant's own turn must not be fenced: {stages:?}"
    );

    // The provider's own wake, on the body the claim names, proceeds.
    assert!(
        umbrella
            .provider
            .team_wake_fence(&umbrella.scope())
            .is_none(),
        "a provider-minted wake on the claimed body is not fenced"
    );

    umbrella.shutdown().await;
}

/// The claim is over the **umbrella**, not one execution: a sibling record
/// rooted at the same genesis is fenced alongside the one that was handed
/// over. This is the v1 scope decision (§1), stated as a test so a later
/// per-execution scope has to change it deliberately.
#[tokio::test]
async fn a_sibling_execution_of_the_same_umbrella_is_fenced_with_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let other_body = "dd".repeat(32);
    let channel_id = Uuid::new_v4();

    let mut umbrella = umbrella(dir.path(), &relay_keys, Keys::generate(), channel_id).await;
    let cwd = dir.path().join("checkout");
    let mut sibling = governed_record(channel_id, &cwd, &umbrella.genesis_ref);
    sibling.command_id = "create-sibling".into();
    let sibling_target = sibling.target(&umbrella.provider.config.instance_id);
    umbrella
        .provider
        .state
        .insert_session(sibling)
        .expect("insert sibling");

    let grant = grant_for(channel_id, &umbrella.genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &umbrella.genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    umbrella.apply(&grant, &relay_keys).await;
    umbrella.apply(&takeover, &relay_keys).await;

    for (command_id, target) in [
        ("claimed-execution", &umbrella.target),
        ("sibling-execution", &sibling_target),
    ] {
        let (stages, code) = send_turn(
            &mut umbrella.provider,
            channel_id,
            command_id,
            target,
            test_operator_keys(),
        )
        .await;
        assert_eq!(stages, vec!["turn_refused".to_owned()], "{command_id}");
        assert_eq!(code.as_deref(), Some(HANDOVER_FENCED), "{command_id}");
    }

    umbrella.shutdown().await;
}

// -------------------------------------------------------------------------
// A voided claim — the required regression
// -------------------------------------------------------------------------

/// Revoke the claimant and the fence stays up for **everybody**: the founder,
/// the ex-claimant, and — the regression this whole three-state fold exists
/// for — the ex-claimant again after being granted `grant-operator` a second
/// time. Only a fresh accepted `takeover` lifts it.
#[tokio::test]
async fn a_voided_claim_fences_everyone_and_a_regrant_does_not_lift_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let channel_id = Uuid::new_v4();

    let mut umbrella = umbrella(dir.path(), &relay_keys, provider_keys, channel_id).await;
    let genesis_ref = umbrella.genesis_ref.clone();

    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &body,
    );
    let revoke = authority_transition_event(
        channel_id,
        &genesis_ref,
        Some(takeover.id.to_hex()),
        3,
        &claimant_hex,
        CodingSessionAuthorityTransitionType::Revoke,
    );
    let regrant = grant_for(
        channel_id,
        &genesis_ref,
        Some(revoke.id.to_hex()),
        4,
        &claimant_hex,
    );
    let retake = takeover_event(
        channel_id,
        &genesis_ref,
        Some(regrant.id.to_hex()),
        5,
        test_operator_keys(),
        &body,
    );

    umbrella.apply(&grant, &relay_keys).await;
    umbrella.apply(&takeover, &relay_keys).await;
    umbrella.apply(&revoke, &relay_keys).await;
    assert!(
        matches!(umbrella.claim(), ClaimState::Voided { .. }),
        "revoking the claimant voids the claim: {:?}",
        umbrella.claim()
    );

    // Both parties, on the voided session.
    for (command_id, operator) in [
        ("founder-after-void", test_operator_keys()),
        ("ex-claimant-after-void", &claimant),
    ] {
        let (stages, code) = send_turn(
            &mut umbrella.provider,
            channel_id,
            command_id,
            &umbrella.target,
            operator,
        )
        .await;
        assert_eq!(stages, vec!["turn_refused".to_owned()], "{command_id}");
        assert_eq!(code.as_deref(), Some(HANDOVER_FENCED), "{command_id}");
    }

    // The regrant restores standing to steer other work — and leaves this
    // session voided. An actual turn, not just the folded state, is what
    // proves it.
    umbrella.apply(&regrant, &relay_keys).await;
    assert!(
        matches!(umbrella.claim(), ClaimState::Voided { .. }),
        "a regrant must not resurrect a claim: {:?}",
        umbrella.claim()
    );
    assert!(
        umbrella
            .provider
            .state()
            .session(&umbrella.target.session_id)
            .expect("record")
            .granted_operators
            .contains(&claimant_hex),
        "the regrant did restore the operator grant itself"
    );
    let (stages, code) = send_turn(
        &mut umbrella.provider,
        channel_id,
        "ex-claimant-after-regrant",
        &umbrella.target,
        &claimant,
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(code.as_deref(), Some(HANDOVER_FENCED));

    // A fresh takeover — by the founder, on this body — lifts it.
    umbrella.apply(&retake, &relay_keys).await;
    let (stages, code) = send_turn(
        &mut umbrella.provider,
        channel_id,
        "founder-after-retake",
        &umbrella.target,
        test_operator_keys(),
    )
    .await;
    assert_ne!(
        code.as_deref(),
        Some(HANDOVER_FENCED),
        "a fresh accepted takeover lifts the fence: {stages:?}"
    );

    umbrella.shutdown().await;
}

// -------------------------------------------------------------------------
// Restart
// -------------------------------------------------------------------------

/// A claim accepted while this provider was down is in force **before** its
/// first metadata event, not after it.
///
/// The order is the assertion: `recover` reads the chain and folds the claim,
/// and only then does the stranded loop publish. The proof is that the very
/// first 44223 this process publishes already carries the `handover` block —
/// a build that folded the claim afterwards would publish a bare
/// `disconnected` first, which is exactly the "ordinary outage" reading §3.1
/// exists to prevent.
#[tokio::test]
async fn a_restart_derives_the_fence_from_the_chain_before_it_publishes_metadata() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.path().join("state");
    let channel_id = Uuid::new_v4();
    let genesis_ref = "ab".repeat(32);
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let other_body = "dd".repeat(32);

    // A provider that knew nothing about any claim writes the record, then
    // dies.
    let target = {
        let mut provider = provider(&state_dir, None);
        let record = governed_record(channel_id, &cwd, &genesis_ref);
        let target = record.target(&provider.config.instance_id);
        provider.state.insert_session(record).expect("insert");
        target
    };

    // Meanwhile the claim is accepted on the relay.
    let takeover = takeover_event(channel_id, &genesis_ref, None, 1, &claimant, &other_body);
    let receipt = claim_receipt(&relay_keys, channel_id, &takeover);
    let provider_keys = Keys::generate();
    let (relay, _queries, server) =
        spawn_test_relay_with_events(&provider_keys, vec![takeover.clone(), receipt.clone()]).await;

    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut restarted =
        Provider::new(config_of(provider_keys, &state_dir, None, agent)).expect("provider");
    restarted.set_relay_self(relay_keys.public_key().to_hex());
    restarted.set_rest_client(relay.rest_client());
    restarted.recover().await.expect("recover");

    let sink = CollectingSink::new();
    restarted.flush(&sink).await.expect("flush");
    let metadata: Vec<serde_json::Value> = sink
        .contents_of(KIND_CODING_SESSION_METADATA)
        .into_iter()
        .filter(|row| row["session"]["sessionId"] == target.session_id)
        .collect();
    let first = metadata.first().expect("recovery published metadata");
    assert_eq!(first["status"], "disconnected");
    assert_eq!(
        first["handover"]["claimant"],
        serde_json::Value::from(claimant.public_key().to_hex()),
        "the first metadata a restart publishes already discloses the fence: {first}"
    );
    assert_eq!(
        first["handover"]["bodyPubkey"],
        serde_json::Value::from(other_body.clone())
    );
    assert_eq!(
        first["handover"]["state"], "active",
        "a claim in force says so, so a reader does not have to guess"
    );

    // And the fence itself is in force on the recovered record.
    let record = restarted
        .state()
        .session(&target.session_id)
        .expect("record");
    assert!(matches!(&record.handover, ClaimState::Active(claim)
        if claim.body_pubkey == other_body));

    relay.shutdown().await;
    server.abort();
}

/// A session nobody has handed over publishes no `handover` key at all — the
/// exact-key shape every pre-amendment consumer checks is unchanged.
#[tokio::test]
async fn an_unclaimed_session_publishes_no_handover_key() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");

    provider
        .publish_metadata(channel_id, &target, SessionStatus::Disconnected)
        .expect("publish");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let metadata = sink
        .contents_of(KIND_CODING_SESSION_METADATA)
        .into_iter()
        .next()
        .expect("metadata");
    assert!(
        metadata.get("handover").is_none(),
        "an unclaimed session must omit the key entirely: {metadata}"
    );
}

// -------------------------------------------------------------------------
// Seat requests and native restore
// -------------------------------------------------------------------------

/// The two host-facing consequences, in one place: a fenced seated generation
/// is *stated* as fenced so the host skips staging its key, and a retired one
/// is omitted entirely.
#[test]
fn seat_requests_mark_the_fenced_and_omit_the_retired() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    let channel_id = Uuid::new_v4();
    let provider_pubkey = "99".repeat(32);
    let claim = buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: "bb".repeat(32),
        body_pubkey: "dd".repeat(32),
        accepted_event_id: "11".repeat(32),
        seq: 1,
    };

    let mut fenced = governed_record(channel_id, &cwd, &"ab".repeat(32));
    fenced.session_id = "fenced".into();
    fenced.actor = Some("cd".repeat(32));
    fenced.role = Some("builder".into());
    fenced.handover = ClaimState::Active(claim.clone());

    let mut retired = governed_record(channel_id, &cwd, &"ab".repeat(32));
    retired.session_id = "retired".into();
    retired.command_id = "create-retired".into();
    retired.actor = Some("cd".repeat(32));
    retired.role = Some("builder".into());
    retired.retired = Some(crate::state::Retirement {
        deletion_event_id: "ee".repeat(32),
        receipt_event_id: None,
        at: 1,
    });

    let mut open = governed_record(channel_id, &cwd, &"ab".repeat(32));
    open.session_id = "open".into();
    open.command_id = "create-open".into();
    open.actor = Some("cd".repeat(32));
    open.role = Some("builder".into());

    let rows =
        crate::seat_requests::derive_seat_requests([&fenced, &retired, &open], &provider_pubkey);
    assert_eq!(
        rows.iter()
            .map(|row| row.session_id.as_str())
            .collect::<Vec<_>>(),
        vec!["fenced", "open"],
        "a retired generation asks for no custody at all"
    );
    assert!(rows[0].fenced, "the fenced row says so");
    assert!(!rows[1].fenced, "the open row does not");
}

/// A restore is a spawn plus a reattachment, so it answers the fence before it
/// asks an adapter anything: the code and the sentence are the ones a turn
/// would have been refused with.
#[tokio::test]
async fn a_fenced_or_retired_record_refuses_native_restore_before_any_adapter_call() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);

    let mut fenced = governed_record(channel_id, &cwd, &"ab".repeat(32));
    fenced.session_id = "fenced".into();
    fenced.resume_cursor = Some("cursor-1".into());
    fenced.handover = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: "bb".repeat(32),
        body_pubkey: "dd".repeat(32),
        accepted_event_id: "11".repeat(32),
        seq: 1,
    });
    provider.state.insert_session(fenced).expect("insert");

    let mut retired = governed_record(channel_id, &cwd, &"ab".repeat(32));
    retired.session_id = "retired".into();
    retired.command_id = "create-retired".into();
    retired.resume_cursor = Some("cursor-2".into());
    retired.retired = Some(crate::state::Retirement {
        deletion_event_id: "ee".repeat(32),
        receipt_event_id: None,
        at: 1,
    });
    provider.state.insert_session(retired).expect("insert");

    for (session_id, expected) in [("fenced", HANDOVER_FENCED), ("retired", SESSION_RETIRED)] {
        let outcome = provider
            .restore_generation(session_id, None)
            .await
            .expect("restore decides");
        let obstacle = outcome.expect_err("a fenced or retired record refuses");
        assert_eq!(obstacle.code(), expected, "{session_id}");
    }
}

// -------------------------------------------------------------------------
// `session.create` — the door the turn path does not cover
// -------------------------------------------------------------------------

/// A genesis-bearing `session.create` signed by `operator`.
///
/// The stock helper always signs as the founder; a claimant's own reconstruct
/// or native continuation is signed by the claimant, and telling those two
/// apart is the whole of what the create fence decides.
fn genesis_create_by(
    provider: &Provider,
    channel_id: Uuid,
    command_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    operator: &Keys,
) -> Event {
    let source =
        create_event_with_genesis_ref(provider, channel_id, command_id, session_ref, genesis_ref);
    signed_lifecycle_event_by(channel_id, source.content, operator)
}

/// A provider holding one record of `genesis_ref` whose claim is `claim`, and
/// a relay that can resolve the genesis so a create's founder lookup succeeds.
async fn provider_with_claimed_umbrella(
    dir: &Path,
    provider_keys: Keys,
    channel_id: Uuid,
    claim: ClaimState,
) -> (
    Provider,
    HarnessRelay,
    tokio::task::JoinHandle<()>,
    String,
    String,
) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.join("state");
    let projects = write_projects(dir, channel_id, &cwd);
    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");

    let session_ref = UMBRELLA_SESSION_REF.to_owned();
    let genesis = genesis_event(channel_id, &session_ref);
    let genesis_ref = genesis.id.to_hex();
    let mut record = governed_record(channel_id, &cwd, &genesis_ref);
    record.handover = claim;
    record.authority_seq = 2;
    provider.state.insert_session(record).expect("insert");

    let (relay, _control, server) = spawn_recording_test_relay(&provider_keys, vec![genesis]).await;
    provider.set_rest_client(relay.rest_client());
    (provider, relay, server, session_ref, genesis_ref)
}

/// A create under an umbrella somebody else holds is refused before an adapter
/// is ever spawned.
///
/// This is the door a fence that only covered turns leaves wide open: a create
/// mints its own record, spawns its own process and dispatches its own
/// `initialTurn` without passing through `decide_turn_command` at all. A's
/// machine coming back and "just starting again" would produce the second live
/// execution the whole feature exists to prevent.
#[tokio::test]
async fn a_create_under_someone_elses_claim_is_refused_before_any_adapter_starts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let claimant = Keys::generate();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let claim = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: claimant.public_key().to_hex(),
        body_pubkey: body,
        accepted_event_id: "11".repeat(32),
        seq: 2,
    });

    let (mut provider, mut relay, server, session_ref, genesis_ref) =
        provider_with_claimed_umbrella(dir.path(), provider_keys, channel_id, claim).await;
    let records_before = provider.state().sessions().count();

    // A — the founder, and not the claimant — starts again.
    let create = genesis_create_by(
        &provider,
        channel_id,
        "create-by-founder",
        &session_ref,
        &genesis_ref,
        test_operator_keys(),
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("the create is decided");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "create-by-founder")
        .expect("the create was answered");
    assert_eq!(receipt["status"], "failed");
    assert_eq!(receipt["error"]["code"], HANDOVER_FENCED);
    assert_eq!(
        provider.state().sessions().count(),
        records_before,
        "no record was minted"
    );
    assert_eq!(
        provider.sessions.live_count(),
        0,
        "and no adapter was spawned, so no `session/new` was ever sent"
    );

    relay.shutdown().await;
    server.abort();
}

/// The claimant's own create on the claimed body is admitted — this is B's
/// native continuation, and refusing it would refuse the feature — and the
/// record it mints **carries the claim**, so the very next turn from anybody
/// else is fenced rather than walking past a fence its siblings are behind.
#[tokio::test]
async fn the_claimants_own_create_is_admitted_and_its_record_carries_the_claim() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let claim = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: claimant_hex.clone(),
        body_pubkey: body,
        accepted_event_id: "11".repeat(32),
        seq: 2,
    });

    let (mut provider, mut relay, server, session_ref, genesis_ref) =
        provider_with_claimed_umbrella(dir.path(), provider_keys, channel_id, claim.clone()).await;
    let records_before = provider.state().sessions().count();

    let create = genesis_create_by(
        &provider,
        channel_id,
        "create-by-claimant",
        &session_ref,
        &genesis_ref,
        &claimant,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("the create is decided");

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let receipt = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "create-by-claimant")
        .expect("the create was answered");
    assert_eq!(
        receipt["status"], "created",
        "the claimant's own continuation is the feature: {receipt}"
    );
    assert_eq!(provider.state().sessions().count(), records_before + 1);

    let minted = provider
        .state()
        .sessions()
        .find(|record| record.command_id == "create-by-claimant")
        .expect("the new record")
        .clone();
    assert_eq!(
        minted.handover, claim,
        "a record born under a claimed umbrella carries the claim, not NoClaim"
    );

    // And the founder's turn on that brand-new execution is fenced.
    let target = minted.target(&provider.config.instance_id);
    let (stages, code) = send_turn(
        &mut provider,
        channel_id,
        "founder-turn-on-new-record",
        &target,
        test_operator_keys(),
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(code.as_deref(), Some(HANDOVER_FENCED));

    relay.shutdown().await;
    server.abort();
}

/// A voided umbrella starts nothing at all. Not the founder's create, not the
/// ex-claimant's: until a fresh accepted takeover is published, nobody opens
/// a new execution under it.
#[tokio::test]
async fn a_create_under_a_voided_claim_is_refused_for_everyone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let claimant = Keys::generate();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let claim = ClaimState::Voided {
        last: buzz_core::coding_session_authority_claim::CurrentClaim {
            claimant: claimant.public_key().to_hex(),
            body_pubkey: body,
            accepted_event_id: "11".repeat(32),
            seq: 2,
        },
        voided_by: "22".repeat(32),
        seq: 3,
    };

    let (mut provider, mut relay, server, session_ref, genesis_ref) =
        provider_with_claimed_umbrella(dir.path(), provider_keys, channel_id, claim).await;
    let records_before = provider.state().sessions().count();

    for (command_id, operator) in [
        ("create-founder-voided", test_operator_keys()),
        ("create-ex-claimant-voided", &claimant),
    ] {
        let create = genesis_create_by(
            &provider,
            channel_id,
            command_id,
            &session_ref,
            &genesis_ref,
            operator,
        );
        provider
            .handle_relay_event(&mut relay, channel_id, &create)
            .await
            .expect("the create is decided");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let receipt = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == command_id)
            .expect("answered");
        assert_eq!(receipt["error"]["code"], HANDOVER_FENCED, "{command_id}");
    }
    assert_eq!(
        provider.state().sessions().count(),
        records_before,
        "a voided umbrella minted nothing"
    );

    relay.shutdown().await;
    server.abort();
}

/// A provider seeing an umbrella for the first time fences nothing — it has
/// folded no claim, and inventing one would refuse the reconstruct this whole
/// feature exists to allow. The claim arrives with the first accepted receipt
/// and the record picks it up then.
#[tokio::test]
async fn a_provider_new_to_an_umbrella_admits_the_create_and_folds_the_claim_after() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.path().join("state");
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();

    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let genesis = genesis_event(channel_id, UMBRELLA_SESSION_REF);
    let genesis_ref = genesis.id.to_hex();
    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &body,
    );
    let (mut relay, control, server) = spawn_recording_test_relay(
        &provider_keys,
        vec![genesis, grant.clone(), takeover.clone()],
    )
    .await;
    provider.set_rest_client(relay.rest_client());
    let _ = &control;

    // This provider has never seen the umbrella, so nothing local fences it.
    let create = genesis_create_by(
        &provider,
        channel_id,
        "create-reconstruct",
        UMBRELLA_SESSION_REF,
        &genesis_ref,
        &claimant,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("the create is decided");
    let minted = provider
        .state()
        .sessions()
        .find(|record| record.command_id == "create-reconstruct")
        .expect("the reconstruct was admitted")
        .clone();
    assert_eq!(
        minted.handover,
        ClaimState::NoClaim,
        "nothing local to seed from yet"
    );

    // The chain then arrives, and the record folds it.
    for transition in [&grant, &takeover] {
        let receipt = claim_receipt(&relay_keys, channel_id, transition);
        provider
            .handle_relay_event(&mut relay, channel_id, &receipt)
            .await
            .expect("apply accepted link");
    }
    let folded = provider
        .state()
        .session(&minted.session_id)
        .expect("record")
        .handover
        .clone();
    let active = folded.active().expect("the claim is now folded in");
    assert_eq!(active.claimant, claimant_hex);
    assert_eq!(active.body_pubkey, body);

    relay.shutdown().await;
    server.abort();
}

/// A wake is judged against **every** record of the umbrella, not the first
/// one found. A sibling minted a moment ago carries no claim yet; reading only
/// that one would let it speak for an umbrella the rest of the provider is
/// fenced behind.
#[test]
fn a_wake_is_fenced_when_any_sibling_of_the_umbrella_is_fenced() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let genesis_ref = "ab".repeat(32);

    // Inserted first, so an implementation that reads only the first match
    // finds the unclaimed one and admits the wake.
    let mut unclaimed = governed_record(channel_id, &cwd, &genesis_ref);
    unclaimed.session_id = "fresh-sibling".into();
    unclaimed.command_id = "create-fresh".into();
    provider.state.insert_session(unclaimed).expect("insert");

    let mut claimed = governed_record(channel_id, &cwd, &genesis_ref);
    claimed.session_id = "claimed".into();
    claimed.command_id = "create-claimed".into();
    claimed.authority_seq = 2;
    claimed.handover =
        ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
            claimant: "bb".repeat(32),
            body_pubkey: "dd".repeat(32),
            accepted_event_id: "11".repeat(32),
            seq: 2,
        });
    provider.state.insert_session(claimed).expect("insert");

    let scope = team_wake::WakeScope {
        channel_ref: channel_id,
        session_ref: UMBRELLA_SESSION_REF.into(),
        genesis_ref,
    };
    let refusal = provider
        .team_wake_fence(&scope)
        .expect("one fenced sibling fences the umbrella's wakes");
    assert_eq!(refusal.code, HANDOVER_FENCED);
}

// -------------------------------------------------------------------------
// Stop, and interrupt
// -------------------------------------------------------------------------

/// The two halves of what a stop means under a claim.
///
/// On the machine the claim **names**, a running process is the claimant's
/// live continuation, and a founder-issued stop would undo the handover by
/// force — so it is fenced. On any **other** machine the process is a stranded
/// execution of the founder's own, and they may shut it down; refusing that
/// would leave a provider unable to release its own slot because of a claim
/// pointing somewhere else entirely.
#[tokio::test]
async fn a_stop_is_fenced_only_on_the_body_the_claim_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let body_here = provider.config.pubkey_hex();
    let claimant = "bb".repeat(32);

    let claim_of = |body: &str| {
        ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
            claimant: claimant.clone(),
            body_pubkey: body.to_owned(),
            accepted_event_id: "11".repeat(32),
            seq: 2,
        })
    };

    let mut on_this_body = governed_record(channel_id, &cwd, &"ab".repeat(32));
    on_this_body.session_id = "here".into();
    on_this_body.command_id = "create-here".into();
    on_this_body.handover = claim_of(&body_here);
    let here_target = on_this_body.target(&provider.config.instance_id);
    provider.state.insert_session(on_this_body).expect("insert");

    let mut elsewhere = governed_record(channel_id, &cwd, &"cd".repeat(32));
    elsewhere.session_id = "elsewhere".into();
    elsewhere.command_id = "create-elsewhere".into();
    elsewhere.handover = claim_of(&"dd".repeat(32));
    let elsewhere_target = elsewhere.target(&provider.config.instance_id);
    provider.state.insert_session(elsewhere).expect("insert");

    match lifecycle_decision_by(
        &provider,
        channel_id,
        "stop-the-claimed-body",
        "session.stop",
        &here_target,
        test_operator_keys(),
    ) {
        LifecycleDecision::Fail { code, .. } => assert_eq!(code, HANDOVER_FENCED),
        other => panic!("stopping the claimant's own live body must be fenced: {other:?}"),
    }
    assert!(
        matches!(
            lifecycle_decision_by(
                &provider,
                channel_id,
                "stop-my-own-stranded-body",
                "session.stop",
                &elsewhere_target,
                test_operator_keys(),
            ),
            LifecycleDecision::Stop(_)
        ),
        "a founder may still shut down a body the claim does not name"
    );
}

/// A voided umbrella has no claimant's work left to protect, so its orphaned
/// process stays stoppable. The alternative is a process nobody — founder
/// included — can ever release, which is a worse failure than the one the
/// fence is guarding against.
#[tokio::test]
async fn a_voided_claim_still_lets_the_founder_stop_an_orphaned_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let body_here = provider.config.pubkey_hex();

    let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    record.handover = ClaimState::Voided {
        last: buzz_core::coding_session_authority_claim::CurrentClaim {
            claimant: "bb".repeat(32),
            body_pubkey: body_here,
            accepted_event_id: "11".repeat(32),
            seq: 2,
        },
        voided_by: "22".repeat(32),
        seq: 3,
    };
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");

    assert!(matches!(
        lifecycle_decision_by(
            &provider,
            channel_id,
            "stop-orphan",
            "session.stop",
            &target,
            test_operator_keys(),
        ),
        LifecycleDecision::Stop(_)
    ));
}

/// Cancelling is not steering. A fenced founder who could destroy the whole
/// process but could not stop one runaway turn would be holding a control that
/// does the more destructive thing and refuses the gentler one — the same
/// reasoning that already keeps interrupts outside the turn budget. A
/// **retired** umbrella still refuses: there is no turn under a deleted
/// session to cancel.
#[tokio::test]
async fn an_interrupt_passes_the_fence_but_not_retirement() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);

    let mut fenced = governed_record(channel_id, &cwd, &"ab".repeat(32));
    fenced.session_id = "fenced".into();
    fenced.command_id = "create-fenced".into();
    fenced.handover = ClaimState::Active(buzz_core::coding_session_authority_claim::CurrentClaim {
        claimant: "bb".repeat(32),
        body_pubkey: "dd".repeat(32),
        accepted_event_id: "11".repeat(32),
        seq: 2,
    });
    let fenced_target = fenced.target(&provider.config.instance_id);
    provider.state.insert_session(fenced).expect("insert");

    let mut retired = governed_record(channel_id, &cwd, &"cd".repeat(32));
    retired.session_id = "retired".into();
    retired.command_id = "create-retired".into();
    retired.retired = Some(crate::state::Retirement {
        deletion_event_id: "ee".repeat(32),
        receipt_event_id: None,
        at: 1,
    });
    let retired_target = retired.target(&provider.config.instance_id);
    provider.state.insert_session(retired).expect("insert");

    for (command_id, target, expected) in [
        ("interrupt-fenced", &fenced_target, None),
        ("interrupt-retired", &retired_target, Some(SESSION_RETIRED)),
    ] {
        let event = interrupt_event(channel_id, command_id, target);
        provider
            .handle_command_event(channel_id, &event)
            .await
            .expect("handle interrupt");
        let sink = CollectingSink::new();
        provider.flush(&sink).await.expect("flush");
        let code = sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .find(|receipt| receipt["commandId"] == command_id)
            .and_then(|receipt| receipt["error"]["code"].as_str().map(str::to_owned));
        match expected {
            Some(expected) => assert_eq!(code.as_deref(), Some(expected), "{command_id}"),
            None => assert_ne!(
                code.as_deref(),
                Some(HANDOVER_FENCED),
                "an interrupt is outside the fence: {command_id}"
            ),
        }
    }
}

// -------------------------------------------------------------------------
// The chain that could not be re-read
// -------------------------------------------------------------------------

/// The asymmetry this gate exists for: a grant that cannot be verified is
/// never applied, so an unreadable chain leaves a session founder-only and
/// safe — but an unverifiable **claim** leaves the persisted `handover` at
/// whatever the machine had before it went down, which for the machine that
/// just came back is "nobody has taken this over". Grants fail closed for
/// free; the claim needs this.
///
/// So: a restart whose chain reads fail refuses by name and publishes no
/// metadata; when the chain becomes readable the executions are admitted or
/// fenced according to what it actually says, and the held metadata goes out.
#[tokio::test]
async fn an_unreadable_chain_refuses_and_holds_metadata_until_it_can_be_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.path().join("state");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let other_body = "dd".repeat(32);

    let genesis_ref = "ab".repeat(32);
    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider =
        Provider::new(config_of(provider_keys.clone(), &state_dir, None, agent)).expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let record = governed_record(channel_id, &cwd, &genesis_ref);
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");

    // A relay that goes away before recovery reads it.
    let (dead, _control, dead_server) =
        spawn_recording_test_relay(&provider_keys, Vec::new()).await;
    provider.set_rest_client(dead.rest_client());
    dead.shutdown().await;
    dead_server.abort();

    provider.recover().await.expect("recover survives it");

    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "an unread chain leaves the umbrella pending"
    );
    let (stages, code) = send_turn(
        &mut provider,
        channel_id,
        "turn-before-reverification",
        &target,
        test_operator_keys(),
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(
        code.as_deref(),
        Some(crate::commands::AUTHORITY_NOT_REVERIFIED),
        "the founder is refused by name rather than admitted on a guess"
    );
    assert!(
        published_metadata_for(&mut provider).await.is_empty(),
        "and nothing is advertised while the fence state is unknown"
    );

    // The chain becomes readable, and it says the session was taken over.
    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    let events = vec![
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];
    let (relay, _control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());

    provider.retry_pending_claim_verification().await;

    assert!(
        provider.claims_pending_reverification.is_empty(),
        "the chain read clean"
    );
    let metadata = published_metadata_for(&mut provider).await;
    let first = metadata
        .first()
        .expect("the held metadata is published now");
    assert_eq!(first["status"], "disconnected");
    assert_eq!(
        first["handover"]["claimant"],
        serde_json::Value::from(claimant_hex),
        "and it carries what the chain actually said: {first}"
    );

    let (stages, code) = send_turn(
        &mut provider,
        channel_id,
        "turn-after-reverification",
        &target,
        test_operator_keys(),
    )
    .await;
    assert_eq!(stages, vec!["turn_refused".to_owned()], "{stages:?}");
    assert_eq!(
        code.as_deref(),
        Some(HANDOVER_FENCED),
        "now refused for the real reason, not for not knowing"
    );

    relay.shutdown().await;
    server.abort();
}

/// Every 44223 the provider has queued, drained through a sink.
async fn published_metadata_for(provider: &mut Provider) -> Vec<serde_json::Value> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.contents_of(KIND_CODING_SESSION_METADATA)
}

/// A voided claim is still published — a returning body has to be able to say
/// why it is fenced — but it says `voided`, so a surface does not point a
/// person at a claimant who no longer holds anything (review finding N7).
#[tokio::test]
async fn a_voided_claim_publishes_the_last_claim_marked_voided() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let claimant = "bb".repeat(32);
    let body = "dd".repeat(32);

    let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    record.handover = ClaimState::Voided {
        last: buzz_core::coding_session_authority_claim::CurrentClaim {
            claimant: claimant.clone(),
            body_pubkey: body.clone(),
            accepted_event_id: "11".repeat(32),
            seq: 2,
        },
        voided_by: "22".repeat(32),
        seq: 3,
    };
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");

    provider
        .publish_metadata(channel_id, &target, SessionStatus::Disconnected)
        .expect("publish");
    let metadata = published_metadata_for(&mut provider)
        .await
        .into_iter()
        .next()
        .expect("metadata");
    assert_eq!(metadata["handover"]["state"], "voided");
    assert_eq!(
        metadata["handover"]["claimant"],
        serde_json::Value::from(claimant),
        "and it still names who held it, so the fence can be explained"
    );
    assert_eq!(
        metadata["handover"]["bodyPubkey"],
        serde_json::Value::from(body)
    );
}
