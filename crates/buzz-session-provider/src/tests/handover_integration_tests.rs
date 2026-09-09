//! The four ways the fence could still be walked past, from root's
//! independent review of this candidate.
//!
//! Every test here is a hostile reading of code that already passed its own
//! tests, and each one is about the same shape of mistake: a place where "I do
//! not know" was written down as "nothing has changed". The four:
//!
//! 1. **A chain read that stopped early cleared the restart fence.** A capped
//!    page, a sequence gap and an unresolvable link all broke the fold and
//!    then reported the umbrella verified, which admits the old body under a
//!    takeover it never managed to read.
//! 2. **A create on a machine new to the umbrella dispatched its first
//!    prompt.** No local record meant no fence, and the backfill that learned
//!    the real claim ran after the adapter had already started.
//! 3. **A live takeover did not stop the turn already running.** Admission
//!    fenced the *next* command; the prompt in flight, and everything queued
//!    behind it, carried on.
//! 4. **Retirement left the queue alone.** A metadata event signed before an
//!    outage is durable, cannot have been named by a later deletion, and
//!    landed on the next flush — recreating exactly the ghost session
//!    retirement exists to remove.

use super::*;

use crate::commands::AUTHORITY_NOT_REVERIFIED;
use crate::payload::HANDOVER_FENCED;
use crate::tests::handover_fence_tests::{claim_receipt, grant_for, takeover_event};
use buzz_core::coding_session_authority_claim::ClaimState;

/// A fake adapter that logs every ACP method and never answers a prompt.
///
/// The stall is the point: the turn is genuinely in flight when the takeover
/// lands, which is the only state where "does the provider stop work it has
/// already started" is a real question. The method log is how the cancel is
/// *observed* rather than assumed — `session/cancel` either appears in it or
/// the provider did not send one.
fn stalling_logged_agent(method_log: &str) -> String {
    format!(
        r#"
LAST_PROMPT=""
while IFS= read -r line; do
  printf '%s\n' "$line" | sed -n 's/.*"method":"\([a-z/_]*\)".*/\1/p' >> "{method_log}"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-session-1"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      LAST_PROMPT="$id" ;;
    *'"method":"session/cancel"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"cancelled"}}}}\n' "$LAST_PROMPT" ;;
  esac
done
"#
    )
}

/// Wait, bounded, for `method` to appear in the adapter's log.
///
/// The cancel is delivered through the actor's mailbox, so it reaches the
/// adapter on a later scheduler pass than the call that queued it. Polling the
/// log is how the test observes what actually reached the runtime rather than
/// what the provider intended.
async fn wait_for_method(log: &Path, method: &str) -> bool {
    for _ in 0..200 {
        if methods(log).iter().any(|seen| seen == method) {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    false
}

/// Every ACP method the fake adapter was asked for, in order.
fn methods(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect()
}

/// A system message signed by the relay identity that is *not* about this
/// chain — the ordinary traffic a busy channel is full of.
fn unrelated_system_message(relay_keys: &Keys, channel_id: Uuid, at: u64) -> Event {
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({ "type": "member_joined" }).to_string(),
    )
    .custom_created_at(nostr::Timestamp::from(at))
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(relay_keys)
    .expect("sign system message")
}

/// A receipt-shaped 40099 that cannot be verified, naming `genesis_ref`.
///
/// `seq: 0` is refused by [`authority::verify_acceptance_receipt`], so this is
/// a genuinely unverifiable row rather than one this build merely dislikes.
fn unverifiable_receipt(relay_keys: &Keys, channel_id: Uuid, genesis_ref: &str) -> Event {
    nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": authority::ACCEPTANCE_RECEIPT_TYPE,
            "genesisRef": genesis_ref,
            "acceptedEventId": "cd".repeat(32),
            "seq": 0,
            "transitionType": "grant-operator",
            "granteePubkey": "ef".repeat(32),
        })
        .to_string(),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(relay_keys)
    .expect("sign receipt")
}

/// One governed record on a provider whose keys and channel the caller chose,
/// wired to a recording relay serving `events`.
struct Restarted {
    provider: Provider,
    relay: HarnessRelay,
    control: RecordingTestRelay,
    server: tokio::task::JoinHandle<()>,
    target: CodingSessionTarget,
    genesis_ref: String,
}

impl Restarted {
    fn pending(&self) -> bool {
        self.provider
            .claims_pending_reverification
            .contains(&self.genesis_ref)
    }

    async fn shutdown(self) {
        self.relay.shutdown().await;
        self.server.abort();
    }
}

async fn restarted_over(
    dir: &Path,
    relay_keys: &Keys,
    provider_keys: Keys,
    channel_id: Uuid,
    genesis_ref: &str,
    events: Vec<Event>,
) -> Restarted {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let state_dir = dir.join("state");
    let agent = fake_agent(state_dir_parent(&state_dir), "good-agent", GOOD_AGENT);
    let mut provider =
        Provider::new(config_of(provider_keys.clone(), &state_dir, None, agent)).expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let record = governed_record(channel_id, &cwd, genesis_ref);
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");
    let (relay, control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());
    Restarted {
        provider,
        relay,
        control,
        server,
        target,
        genesis_ref: genesis_ref.to_owned(),
    }
}

/// The founder's real turn, and the code it was answered with.
async fn founder_turn_code(
    provider: &mut Provider,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
) -> Option<String> {
    let event = command_event_by(
        channel_id,
        command_id,
        target,
        serde_json::json!({ "type": "thread.turn.start", "text": "carry on" }),
        test_operator_keys(),
    );
    provider
        .handle_command_event(channel_id, &event)
        .await
        .expect("handle turn");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id)
        .and_then(|receipt| receipt["error"]["code"].as_str().map(str::to_owned))
}

// =========================================================================
// 1. A chain read that stopped early must not clear the fence
// =========================================================================

/// A first page that comes back exactly full is **evidence of uncertainty**,
/// not evidence that nothing was found behind it.
///
/// Here the whole page is one second's worth of unrelated system messages, so
/// paging with an inclusive `until` cannot advance past it — the honest answer
/// is "I could not read to the end", and the takeover sitting behind the cap
/// stays unread. Before the fix this looked exactly like a chain with no claim
/// in it and the old body was admitted.
#[tokio::test]
async fn a_capped_read_that_cannot_page_leaves_the_umbrella_unverified() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let provider_keys = Keys::generate();
    let claimant = Keys::generate();
    let genesis_ref = "ab".repeat(32);
    let other_body = "dd".repeat(32);

    let grant = grant_for(
        channel_id,
        &genesis_ref,
        None,
        1,
        &claimant.public_key().to_hex(),
    );
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    let mut events = vec![
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];
    // A full page of newer traffic, all in one second, so the page is capped
    // and cannot be paged past.
    let wall = now_secs() + 3_600;
    events.extend(
        (0..AUTHORITY_BACKFILL_QUERY_LIMIT)
            .map(|_| unrelated_system_message(&relay_keys, channel_id, wall)),
    );

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        &genesis_ref,
        events,
    )
    .await;
    restarted.provider.recover().await.expect("recover");

    assert!(
        restarted.pending(),
        "a capped, unpageable read must leave the umbrella unverified"
    );
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-behind-the-cap",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(AUTHORITY_NOT_REVERIFIED),
        "and a real turn on the old body is refused rather than admitted"
    );

    restarted.shutdown().await;
}

/// The same cap, pageable: the takeover is older than a full page of traffic
/// whose timestamps advance, so the read pages past it, finds the claim, and
/// the fence applies for real.
#[tokio::test]
async fn a_capped_read_that_can_page_finds_the_claim_behind_the_cap() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let provider_keys = Keys::generate();
    let claimant = Keys::generate();
    let genesis_ref = "ab".repeat(32);
    let other_body = "dd".repeat(32);

    let grant = grant_for(
        channel_id,
        &genesis_ref,
        None,
        1,
        &claimant.public_key().to_hex(),
    );
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    let mut events = vec![
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];
    let wall = now_secs() + 3_600;
    events.extend(
        (0..AUTHORITY_BACKFILL_QUERY_LIMIT)
            .map(|index| unrelated_system_message(&relay_keys, channel_id, wall + index as u64)),
    );

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        &genesis_ref,
        events,
    )
    .await;
    restarted.provider.recover().await.expect("recover");

    assert!(
        !restarted.pending(),
        "the read reached the end of the history"
    );
    assert!(
        matches!(
            &restarted
                .provider
                .state()
                .session(&restarted.target.session_id)
                .expect("record")
                .handover,
            ClaimState::Active(claim) if claim.body_pubkey == other_body
        ),
        "and it found the takeover that was behind the first page"
    );
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-after-paging",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(HANDOVER_FENCED)
    );

    restarted.shutdown().await;
}

/// A gap in the accepted sequence is a link this provider has not seen. The
/// chain is not verified, and the fence stays up.
#[tokio::test]
async fn a_sequence_gap_leaves_the_umbrella_unverified() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let provider_keys = Keys::generate();
    let claimant = Keys::generate();
    let genesis_ref = "ab".repeat(32);

    // seq 2 with no seq 1 anywhere.
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some("11".repeat(32)),
        2,
        &claimant,
        &"dd".repeat(32),
    );
    let events = vec![
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        &genesis_ref,
        events,
    )
    .await;
    restarted.provider.recover().await.expect("recover");

    assert!(restarted.pending(), "a gap is not a verified chain");
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-over-a-gap",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(AUTHORITY_NOT_REVERIFIED)
    );

    restarted.shutdown().await;
}

/// A relay-verified receipt whose transition cannot be resolved is the exact
/// case that used to clear the fence: the loop broke, and the code below it
/// said the chain had been read end to end. It had not.
///
/// The second half is the one that matters most — when the transition becomes
/// resolvable, the retry learns the claim and the founder is refused for the
/// **real** reason instead of for not knowing.
#[tokio::test]
async fn an_unresolvable_link_holds_the_fence_until_it_resolves() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let provider_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let genesis_ref = "ab".repeat(32);
    let other_body = "dd".repeat(32);

    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    // Both receipts are served; the takeover *transition* is deliberately not,
    // so its exact-id lookup finds nothing.
    let events = vec![
        grant.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        &genesis_ref,
        events,
    )
    .await;
    restarted.provider.recover().await.expect("recover");

    assert!(
        restarted.pending(),
        "an accepted link that cannot be resolved is not a verified chain"
    );
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-before-resolution",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(AUTHORITY_NOT_REVERIFIED)
    );

    // The transition becomes readable.
    restarted
        .control
        .events
        .lock()
        .expect("events")
        .push(takeover);
    restarted.provider.retry_pending_claim_verification().await;

    assert!(!restarted.pending(), "now the chain reads end to end");
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-after-resolution",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(HANDOVER_FENCED),
        "refused for the real reason"
    );

    restarted.shutdown().await;
}

/// Foreign noise must not manufacture a denial. An unverifiable receipt naming
/// **another** umbrella says nothing about this chain, which verifies normally
/// — while an unverifiable receipt naming *this* genesis does hold the fence.
///
/// Both halves matter: the first stops anyone jamming a session by publishing
/// garbage, and the second is what makes an unreadable link fail closed.
#[tokio::test]
async fn foreign_garbage_does_not_deny_but_garbage_about_this_chain_does() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let genesis_ref = "ab".repeat(32);
    let other_body = "dd".repeat(32);

    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    let chain = vec![
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    // (a) Garbage about somebody else's umbrella.
    {
        let mut events = chain.clone();
        events.push(unverifiable_receipt(
            &relay_keys,
            channel_id,
            &"99".repeat(32),
        ));
        let mut restarted = restarted_over(
            dir.path(),
            &relay_keys,
            Keys::generate(),
            channel_id,
            &genesis_ref,
            events,
        )
        .await;
        restarted.provider.recover().await.expect("recover");
        assert!(
            !restarted.pending(),
            "another umbrella's unreadable receipt is not evidence about this one"
        );
        assert_eq!(
            founder_turn_code(
                &mut restarted.provider,
                channel_id,
                "turn-with-foreign-noise",
                &restarted.target,
            )
            .await
            .as_deref(),
            Some(HANDOVER_FENCED)
        );
        restarted.shutdown().await;
    }

    // (b) Garbage claiming *this* umbrella.
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut events = chain;
        events.push(unverifiable_receipt(&relay_keys, channel_id, &genesis_ref));
        let mut restarted = restarted_over(
            dir.path(),
            &relay_keys,
            Keys::generate(),
            channel_id,
            &genesis_ref,
            events,
        )
        .await;
        restarted.provider.recover().await.expect("recover");
        assert!(
            restarted.pending(),
            "a receipt naming this genesis that cannot be read is a link this chain is \
             missing, and the fence stays up"
        );
        restarted.shutdown().await;
    }
}

/// One sibling verifying must not unlock another that did not.
///
/// The pending flag is umbrella-wide; admission reads each record's own
/// `handover`. So a sibling that failed to fold the takeover would be admitted
/// on the strength of a sibling that did — with the old, `NoClaim` answer.
#[tokio::test]
async fn one_sibling_verifying_does_not_unlock_a_sibling_that_did_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let provider_keys = Keys::generate();
    let claimant = Keys::generate();
    let genesis_ref = "ab".repeat(32);

    let grant = grant_for(
        channel_id,
        &genesis_ref,
        None,
        1,
        &claimant.public_key().to_hex(),
    );
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &"dd".repeat(32),
    );
    let events = vec![
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        &genesis_ref,
        events,
    )
    .await;
    // A sibling of the same umbrella that cannot fold anything: with no
    // recorded owner, every transition fails its signer check.
    let cwd = dir.path().join("checkout");
    let mut ownerless = governed_record(channel_id, &cwd, &genesis_ref);
    ownerless.session_id = "ownerless-sibling".into();
    ownerless.command_id = "create-ownerless".into();
    ownerless.founder_pubkey = None;
    restarted
        .provider
        .state
        .insert_session(ownerless)
        .expect("insert sibling");

    restarted.provider.recover().await.expect("recover");

    assert!(
        restarted.pending(),
        "the umbrella stays unverified while one of its executions is behind"
    );
    assert_eq!(
        founder_turn_code(
            &mut restarted.provider,
            channel_id,
            "turn-on-verified-sibling",
            &restarted.target,
        )
        .await
        .as_deref(),
        Some(AUTHORITY_NOT_REVERIFIED),
        "including on the sibling that did fold the chain"
    );

    restarted.shutdown().await;
}

// =========================================================================
// 2. A create on a machine new to the umbrella
// =========================================================================

/// A signed genesis-bearing create, from `operator`, addressed to `provider`.
fn join_create(
    provider: &Provider,
    channel_id: Uuid,
    command_id: &str,
    session_ref: &str,
    genesis_ref: &str,
    operator: &Keys,
) -> Event {
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": command_id,
        "action": {
            "type": "session.create",
            "projectRef": null,
            "repoRef": null,
            "sessionRef": session_ref,
            "genesisRef": genesis_ref,
            "providerInstanceRef": "claude-primary",
            "providerAuthorityPubkey": provider.config.pubkey_hex(),
            "model": null,
            "title": "Join",
            "initialTurn": "pick up where A left off",
        },
    })
    .to_string();
    signed_lifecycle_event_by(channel_id, content, operator)
}

/// A provider that has never seen this umbrella, wired to a relay serving
/// `events`, with a working checkout and an adapter that logs its methods.
async fn fresh_body(
    dir: &Path,
    relay_keys: &Keys,
    provider_keys: Keys,
    channel_id: Uuid,
    events: Vec<Event>,
    log: &Path,
) -> (Provider, HarnessRelay, tokio::task::JoinHandle<()>) {
    let cwd = dir.join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let projects = write_projects(dir, channel_id, &cwd);
    let state_dir = dir.join("state");
    let agent = fake_agent(
        state_dir_parent(&state_dir),
        "stalling-logged-agent",
        &stalling_logged_agent(&log.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());
    let (relay, _control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());
    (provider, relay, server)
}

/// The failure root traced: a machine with no record of the umbrella treated
/// itself as unfenced, started an adapter, and handed it the create's first
/// prompt — a second live execution of work somebody else holds.
///
/// The assertion that matters is the method log: **no `session/new`**, because
/// the fence answered before anything was spawned.
#[tokio::test]
async fn a_create_on_a_fresh_body_under_another_bodys_claim_starts_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let other_body = "dd".repeat(32);

    let genesis = genesis_event(channel_id, session_ref);
    let genesis_ref = genesis.id.to_hex();
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
        genesis,
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let log = dir.path().join("methods.log");
    let (mut provider, mut relay, server) = fresh_body(
        dir.path(),
        &relay_keys,
        Keys::generate(),
        channel_id,
        events,
        &log,
    )
    .await;

    let create = join_create(
        &provider,
        channel_id,
        "join-under-claim",
        session_ref,
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
        .find(|receipt| receipt["commandId"] == "join-under-claim")
        .expect("answered");
    assert_eq!(receipt["error"]["code"], HANDOVER_FENCED);
    assert!(
        receipt["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains(&claimant_hex)),
        "the refusal names who holds it: {receipt}"
    );
    assert_eq!(
        provider.state().sessions().count(),
        0,
        "no record was minted"
    );
    assert!(
        methods(&log).is_empty(),
        "no adapter was started, so no session/new and no prompt: {:?}",
        methods(&log)
    );

    relay.shutdown().await;
    server.abort();
}

/// The same create, with the chain unreadable. "I could not check" must not
/// become "nobody has taken this over": no prompt, no invented `NoClaim`, a
/// named refusal, and the umbrella marked pending so the tick retries it.
#[tokio::test]
async fn a_create_on_a_fresh_body_with_an_unreadable_chain_starts_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    let genesis = genesis_event(channel_id, session_ref);
    let genesis_ref = genesis.id.to_hex();
    let grant = grant_for(
        channel_id,
        &genesis_ref,
        None,
        1,
        &claimant.public_key().to_hex(),
    );
    // The receipt is served; the transition it names is not.
    let events = vec![genesis, claim_receipt(&relay_keys, channel_id, &grant)];

    let log = dir.path().join("methods.log");
    let (mut provider, mut relay, server) = fresh_body(
        dir.path(),
        &relay_keys,
        Keys::generate(),
        channel_id,
        events,
        &log,
    )
    .await;

    let create = join_create(
        &provider,
        channel_id,
        "join-unreadable",
        session_ref,
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
        .find(|receipt| receipt["commandId"] == "join-unreadable")
        .expect("answered");
    assert_eq!(receipt["error"]["code"], AUTHORITY_NOT_REVERIFIED);
    assert_eq!(provider.state().sessions().count(), 0);
    assert!(methods(&log).is_empty(), "{:?}", methods(&log));
    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "and the umbrella is queued for another read rather than forgotten"
    );

    relay.shutdown().await;
    server.abort();
}

/// The positive case, on the same code path: the claimant's own machine, also
/// meeting the umbrella for the first time, verifies the same chain and is
/// admitted — and the record it mints carries the claim rather than the
/// `NoClaim` a fresh body would otherwise be born with.
#[tokio::test]
async fn the_claimants_own_fresh_body_verifies_the_chain_and_creates() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    let genesis = genesis_event(channel_id, session_ref);
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
    let events = vec![
        genesis,
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let log = dir.path().join("methods.log");
    let (mut provider, mut relay, server) = fresh_body(
        dir.path(),
        &relay_keys,
        provider_keys,
        channel_id,
        events,
        &log,
    )
    .await;

    let create = join_create(
        &provider,
        channel_id,
        "join-as-claimant",
        session_ref,
        &genesis_ref,
        &claimant,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("the create is decided");

    let record = provider
        .state()
        .sessions()
        .next()
        .expect("the claimant's own continuation was admitted")
        .clone();
    assert!(
        matches!(&record.handover, ClaimState::Active(claim)
            if claim.claimant == claimant_hex && claim.body_pubkey == body),
        "and the record carries the claim it was verified against: {:?}",
        record.handover
    );
    assert!(
        methods(&log).iter().any(|method| method == "session/new"),
        "the adapter really did start: {:?}",
        methods(&log)
    );

    relay.shutdown().await;
    server.abort();
}

// =========================================================================
// 3. A live takeover stops the turn already running
// =========================================================================

/// Admission fences the next command; this fences the one already in flight.
///
/// The stated limit is in the terminal text and it is not a hedge: the
/// provider requests the cancel the instant it applies the claim, and refuses
/// everything afterwards, but it cannot promise the runtime stops inside a
/// tool call it has already entered. What it *can* promise — and what this
/// pins — is that the cancel was sent, that nothing queued behind the turn
/// runs, and that a replay of the interrupted command is silent.
#[tokio::test]
async fn a_live_takeover_cancels_the_running_turn_and_releases_the_body() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let other_body = "dd".repeat(32);

    let log = dir.path().join("methods.log");
    let state_dir = dir.path().join("state");
    let agent = fake_agent(
        state_dir_parent(&state_dir),
        "stalling-logged-agent",
        &stalling_logged_agent(&log.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let genesis = genesis_event(channel_id, session_ref);
    let genesis_ref = genesis.id.to_hex();
    let grant = grant_for(channel_id, &genesis_ref, None, 1, &claimant_hex);
    let takeover = takeover_event(
        channel_id,
        &genesis_ref,
        Some(grant.id.to_hex()),
        2,
        &claimant,
        &other_body,
    );
    let (mut relay, control, server) = spawn_recording_test_relay(
        &provider_keys,
        vec![genesis, grant.clone(), takeover.clone()],
    )
    .await;
    provider.set_rest_client(relay.rest_client());

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-governed",
        session_ref,
        &genesis_ref,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    let record = provider.state().sessions().next().expect("session").clone();
    let target = record.target(&provider.config.instance_id);

    // The founder starts a turn that never finishes.
    let turn = command_event_by(
        channel_id,
        "turn-in-flight",
        &target,
        serde_json::json!({ "type": "thread.turn.start", "text": "long job" }),
        test_operator_keys(),
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &turn)
        .await
        .expect("turn");
    pump_until_turn_started(&mut provider).await;
    assert!(
        provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .open_turn
            .is_some(),
        "the turn really is in flight"
    );

    // B takes the session over on another machine, and the receipts arrive
    // live on this provider's channel subscription.
    for transition in [&grant, &takeover] {
        let receipt = claim_receipt(&relay_keys, channel_id, transition);
        control.events.lock().expect("events").push(receipt.clone());
        provider
            .handle_relay_event(&mut relay, channel_id, &receipt)
            .await
            .expect("apply accepted link");
    }

    assert!(
        wait_for_method(&log, "session/cancel").await,
        "the provider asked the runtime to stop at once: {:?}",
        methods(&log)
    );
    let record = provider
        .state()
        .session(&target.session_id)
        .expect("record")
        .clone();
    assert!(
        record.open_turn.is_none(),
        "the turn is no longer in flight"
    );
    assert_eq!(
        provider.sessions.live_count(),
        0,
        "and the actor is gone, so nothing queued behind the cancelled turn runs"
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let terminal = transcript_items_in_sequence(&sink)
        .into_iter()
        .filter_map(|row| row["item"]["result"].as_str().map(str::to_owned))
        .find(|text| text.contains(HANDOVER_FENCED))
        .expect("a terminal row says why the turn ended");
    assert!(
        terminal.contains("may still finish a tool call it had already started"),
        "and it states the limit rather than promising zero overlap: {terminal}"
    );

    // A replay of the interrupted command is answered from the ledger, not
    // republished and not re-run.
    assert!(
        provider.state().is_command_refused("turn-in-flight"),
        "the interrupted command is durably refused"
    );
    let before = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .len();
    provider
        .handle_relay_event(&mut relay, channel_id, &turn)
        .await
        .expect("replay");
    let after = CollectingSink::new();
    provider.flush(&after).await.expect("flush");
    assert!(
        after
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .is_empty(),
        "a replayed interrupted command publishes nothing new (was {before})"
    );

    relay.shutdown().await;
    server.abort();
}

/// The claimant's own work is not interrupted by the claimant's own claim.
///
/// The mirror of the test above, and the reason the fence is asked with the
/// turn's **operator** rather than only with the body: a takeover naming this
/// provider is the claimant choosing this machine, and the turn already
/// running is theirs. Cancelling it would be the fence undoing the handover it
/// exists to protect. (A turn on this same body driven by somebody who is
/// *not* the new claimant is a different case, and is stopped — that is what
/// the takeover test above exercises.)
#[tokio::test]
async fn a_claim_on_this_body_does_not_interrupt_its_own_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    let log = dir.path().join("methods.log");
    let state_dir = dir.path().join("state");
    let agent = fake_agent(
        state_dir_parent(&state_dir),
        "stalling-logged-agent",
        &stalling_logged_agent(&log.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let genesis = genesis_event(channel_id, session_ref);
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
    let (mut relay, _control, server) = spawn_recording_test_relay(
        &provider_keys,
        vec![genesis, grant.clone(), takeover.clone()],
    )
    .await;
    provider.set_rest_client(relay.rest_client());

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-governed",
        session_ref,
        &genesis_ref,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");
    let record = provider.state().sessions().next().expect("session").clone();
    let target = record.target(&provider.config.instance_id);

    // The grant lands first, so the claimant has standing to steer, and the
    // running turn is *theirs*. That is the case this test is about: the claim
    // that arrives next names this body and this person, so it is the handover
    // arriving over its own work.
    provider
        .handle_relay_event(
            &mut relay,
            channel_id,
            &claim_receipt(&relay_keys, channel_id, &grant),
        )
        .await
        .expect("apply the grant");

    let turn = command_event_by(
        channel_id,
        "turn-on-the-claimed-body",
        &target,
        serde_json::json!({ "type": "thread.turn.start", "text": "long job" }),
        &claimant,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &turn)
        .await
        .expect("turn");
    pump_until_turn_started(&mut provider).await;

    provider
        .handle_relay_event(
            &mut relay,
            channel_id,
            &claim_receipt(&relay_keys, channel_id, &takeover),
        )
        .await
        .expect("apply the takeover");
    // Give the actor the same chance to act on a cancel that the interrupting
    // test gives it, so "no cancel" is an observation rather than a race.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    assert!(
        !methods(&log)
            .iter()
            .any(|method| method == "session/cancel"),
        "a claim naming this body must not cancel the work it just claimed: {:?}",
        methods(&log)
    );
    assert!(
        provider
            .state()
            .session(&target.session_id)
            .expect("record")
            .open_turn
            .is_some(),
        "and the turn is still running"
    );
    assert_eq!(provider.sessions.live_count(), 1);

    relay.shutdown().await;
    server.abort();
}

// =========================================================================
// 4. Retirement empties the queue it inherited
// =========================================================================

/// A signed 44223 queued before an outage is durable, and a deletion published
/// afterwards cannot have named it. Flushing it after retirement recreates the
/// deleted session on the relay under a brand-new event id — the exact ghost
/// row retirement exists to remove.
///
/// The scope matters as much as the suppression: an unrelated session's queued
/// metadata still goes out, and so does the lifecycle receipt that tells an
/// operator *why* the retired one answers nothing.
#[tokio::test]
async fn retirement_discards_queued_facts_for_the_deleted_session_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let genesis_ref = "ab".repeat(32);
    let deletion_id = "ee".repeat(32);

    let receipt = nostr::EventBuilder::new(
        nostr::Kind::Custom(KIND_SYSTEM_MESSAGE as u16),
        serde_json::json!({
            "type": "coding_session_deletion_accepted",
            "genesisRef": genesis_ref,
            "sessionRef": "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "deletionEventId": deletion_id,
            "channelId": channel_id.to_string(),
        })
        .to_string(),
    )
    .tags(vec![
        nostr::Tag::parse(["h", &channel_id.to_string()]).expect("tag")
    ])
    .sign_with_keys(&relay_keys)
    .expect("sign deletion receipt");

    let mut restarted = restarted_over(
        dir.path(),
        &relay_keys,
        Keys::generate(),
        channel_id,
        &genesis_ref,
        vec![receipt],
    )
    .await;

    // An unrelated execution in the same channel, so suppression can be shown
    // to be scoped rather than blanket.
    let mut unrelated = governed_record(channel_id, &cwd, &"cd".repeat(32));
    unrelated.session_id = "unrelated".into();
    unrelated.command_id = "create-unrelated".into();
    let unrelated_target = unrelated.target(&restarted.provider.config.instance_id);
    restarted
        .provider
        .state
        .insert_session(unrelated)
        .expect("insert");

    // The queue the previous process left behind: metadata and a transcript
    // row for the doomed execution, and metadata for the unrelated one.
    let doomed_target = restarted.target.clone();
    restarted
        .provider
        .publish_metadata(channel_id, &doomed_target, SessionStatus::Disconnected)
        .expect("queue metadata");
    restarted
        .provider
        .enqueue_transcript(
            channel_id,
            &doomed_target,
            None,
            payload::status_item("mid_flight"),
            Priority::Normal,
        )
        .expect("queue transcript");
    restarted
        .provider
        .publish_metadata(channel_id, &unrelated_target, SessionStatus::Disconnected)
        .expect("queue unrelated metadata");
    assert!(restarted.provider.pending_publishes() >= 3);

    // The deletion is reconciled at startup.
    restarted.provider.recover().await.expect("recover");
    assert!(
        restarted
            .provider
            .state()
            .session(&doomed_target.session_id)
            .expect("record")
            .is_retired(),
        "the receipt retired the umbrella"
    );

    // A command arrives for the retired execution and is answered by name.
    let turn = command_event_by(
        channel_id,
        "turn-after-deletion",
        &doomed_target,
        serde_json::json!({ "type": "thread.turn.start", "text": "still there?" }),
        test_operator_keys(),
    );
    restarted
        .provider
        .handle_command_event(channel_id, &turn)
        .await
        .expect("handle turn");

    let sink = CollectingSink::new();
    restarted.provider.flush(&sink).await.expect("flush");

    let published_for = |target: &CodingSessionTarget| {
        sink.contents_of(KIND_CODING_SESSION_METADATA)
            .into_iter()
            .filter(|row| row["session"]["sessionId"] == target.session_id)
            .count()
    };
    assert_eq!(
        published_for(&doomed_target),
        0,
        "nothing queued for the deleted session escaped"
    );
    assert!(
        transcript_items_in_sequence(&sink).is_empty(),
        "including its transcript row"
    );
    assert_eq!(
        published_for(&unrelated_target),
        1,
        "and the unrelated session's queued metadata still went out"
    );
    let refusal = sink
        .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == "turn-after-deletion")
        .expect("the command was still answered");
    assert_eq!(refusal["error"]["code"], crate::payload::SESSION_RETIRED);

    restarted.shutdown().await;
}

// =========================================================================
// No witnessed relay identity is unreadable authority
// =========================================================================

/// A provider that could not read the relay's identity cannot verify a single
/// receipt, so it cannot tell an unclaimed umbrella from one it merely failed
/// to read — and the machine most likely to be in that state is the one coming
/// back from the outage during which it lost the session.
///
/// So the genesis-bearing create is refused by name and the umbrella is queued
/// for another read. Then the identity is witnessed on the tick, the chain
/// reads, and the same create is answered for the **real** reason: admitted
/// for the claimant, `HANDOVER_FENCED` for anybody else.
#[tokio::test]
async fn a_create_with_no_witnessed_identity_is_refused_until_the_tick_witnesses_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let provider_keys = Keys::generate();
    let body = provider_keys.public_key().to_hex();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

    let genesis = genesis_event(channel_id, session_ref);
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
    let events = vec![
        genesis,
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let log = dir.path().join("methods.log");
    let agent = fake_agent(
        state_dir_parent(&state_dir),
        "stalling-logged-agent",
        &stalling_logged_agent(&log.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    let (mut relay, control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());

    // The relay publishes no identity yet — a startup that raced the relay, or
    // a relay not yet configured with one.
    *control.relay_self.lock().expect("relay self") = None;
    assert!(
        !provider.witness_relay_identity().await,
        "nothing to witness yet"
    );

    let create = join_create(
        &provider,
        channel_id,
        "join-without-identity",
        session_ref,
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
        .find(|receipt| receipt["commandId"] == "join-without-identity")
        .expect("answered");
    assert_eq!(receipt["error"]["code"], AUTHORITY_NOT_REVERIFIED);
    assert_eq!(
        provider.state().sessions().count(),
        0,
        "no record was minted"
    );
    assert!(
        methods(&log).is_empty(),
        "and no adapter was started: {:?}",
        methods(&log)
    );
    assert!(
        provider
            .claims_pending_reverification
            .contains(&genesis_ref),
        "the umbrella is queued for another read"
    );

    // The relay's identity becomes readable, and the tick witnesses it —
    // no restart.
    *control.relay_self.lock().expect("relay self") = Some(relay_keys.public_key().to_hex());
    assert!(
        provider.witness_relay_identity().await,
        "the tick's retry witnesses the identity"
    );

    // The claimant's own create is now admitted, on the chain this provider
    // can finally read.
    let create = join_create(
        &provider,
        channel_id,
        "join-after-witnessing",
        session_ref,
        &genesis_ref,
        &claimant,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("the create is decided");
    let record = provider
        .state()
        .sessions()
        .next()
        .expect("the claimant's continuation was admitted once the chain could be read")
        .clone();
    assert!(
        matches!(&record.handover, ClaimState::Active(claim)
            if claim.claimant == claimant_hex && claim.body_pubkey == body),
        "and it carries the claim it was verified against: {:?}",
        record.handover
    );

    relay.shutdown().await;
    server.abort();
}

/// The other half of the same tick: once the identity is witnessed, a create
/// from the **old body's** operator is refused for the real reason rather than
/// for not knowing.
#[tokio::test]
async fn once_the_identity_is_witnessed_the_old_body_is_refused_by_the_fence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let channel_id = Uuid::new_v4();
    let relay_keys = Keys::generate();
    let claimant = Keys::generate();
    let claimant_hex = claimant.public_key().to_hex();
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let other_body = "dd".repeat(32);

    let genesis = genesis_event(channel_id, session_ref);
    let genesis_ref = genesis.id.to_hex();
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
        genesis,
        grant.clone(),
        takeover.clone(),
        claim_receipt(&relay_keys, channel_id, &grant),
        claim_receipt(&relay_keys, channel_id, &takeover),
    ];

    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let state_dir = dir.path().join("state");
    let log = dir.path().join("methods.log");
    let provider_keys = Keys::generate();
    let agent = fake_agent(
        state_dir_parent(&state_dir),
        "stalling-logged-agent",
        &stalling_logged_agent(&log.to_string_lossy()),
    );
    let mut provider = Provider::new(config_of(
        provider_keys.clone(),
        &state_dir,
        Some(&projects),
        agent,
    ))
    .expect("provider");
    let (mut relay, control, server) = spawn_recording_test_relay(&provider_keys, events).await;
    provider.set_rest_client(relay.rest_client());
    *control.relay_self.lock().expect("relay self") = Some(relay_keys.public_key().to_hex());
    assert!(provider.witness_relay_identity().await);

    let create = join_create(
        &provider,
        channel_id,
        "join-as-old-body",
        session_ref,
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
        .find(|receipt| receipt["commandId"] == "join-as-old-body")
        .expect("answered");
    assert_eq!(receipt["error"]["code"], HANDOVER_FENCED);
    assert_eq!(provider.state().sessions().count(), 0);
    assert!(methods(&log).is_empty(), "{:?}", methods(&log));

    relay.shutdown().await;
    server.abort();
}
