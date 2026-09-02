//! The one thing a published session policy (kind 44245) actually enforces.
//!
//! `docs/design/portable-team-loop/POLICY.md` §4 used to say "no enforcement",
//! and that was true: a founder could publish a budget and nothing counted it.
//! Exactly one field is now enforced — `budget.turns`, at the provider's D9
//! turn gate — and the refusal has to say *which* ceiling bound, because a
//! reader who cannot tell the host's `BUZZ_CSP_TURN_BUDGET` from the session's
//! own published policy cannot tell which one to change.
//!
//! Everything else in a 44245 is read and shown, never enforced. These tests
//! pin the enforced field and the untouched fallback; the projection side is
//! pinned in `context_projector`'s own tests.

use super::*;

/// A provider holding one governed umbrella with a granted seat on it, and the
/// seat's keys. The seat matters because the umbrella's *founder* is exempt
/// from every turn budget by construction — a budget bounds delegated work.
fn budgeted_umbrella(
    dir: &tempfile::TempDir,
    env_budget: u64,
) -> (Provider, Uuid, Keys, String, CodingSessionTarget) {
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    provider.config.turn_budget = env_budget;

    let seat = Keys::generate();
    let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    record.granted_operators.insert(seat.public_key().to_hex());
    record.authority_seq = 1;
    let umbrella = record.session_ref.clone().expect("umbrella");
    let session_id = record.session_id.clone();
    let target = record.target(&provider.config.instance_id);
    provider.state.insert_session(record).expect("insert");
    // Claiming the founder is what makes the seat's turns chargeable and the
    // founder's exempt, exactly as a real create does.
    provider
        .state
        .claim_umbrella_founder(&umbrella, &test_operator_keys().public_key().to_hex())
        .expect("claim founder");
    let _ = session_id;
    (provider, channel_id, seat, umbrella, target)
}

/// Charge `count` started turns to the umbrella, the way a live adapter does.
fn spend_turns(provider: &mut Provider, session_id: &str, count: usize) {
    for n in 0..count {
        provider
            .handle_session_event(SessionEvent::TurnStarted {
                session_id: session_id.to_owned(),
                turn_id: format!("turn-id-{n}"),
                command_id: format!("spend-{n}"),
                text: "work".into(),
            })
            .expect("turn started");
    }
}

/// The refusal receipt for one command id, or `None`.
async fn refusal_for(
    provider: &mut Provider,
    channel_id: Uuid,
    command_id: &str,
    target: &CodingSessionTarget,
    seat: &Keys,
) -> Option<serde_json::Value> {
    let command = command_event_by(
        channel_id,
        command_id,
        target,
        serde_json::json!({ "type": "thread.turn.start", "text": "one more" }),
        seat,
    );
    provider
        .handle_command_event(channel_id, &command)
        .await
        .expect("handle");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
        .into_iter()
        .find(|receipt| receipt["commandId"] == command_id)
}

/// `budget.turns = 3` binds even where the host set no environment ceiling at
/// all, and the refusal names the policy so a reader knows what to change.
#[tokio::test]
async fn a_policy_turn_budget_binds_and_names_itself_in_the_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, seat, umbrella, target) =
        budgeted_umbrella(&dir, config::UNLIMITED_TURN_BUDGET);
    provider.set_policy_turn_budget(&umbrella, Some(3));
    let session_id = target.session_id.clone();

    spend_turns(&mut provider, &session_id, 3);
    assert_eq!(provider.state().turns_used(&umbrella), 3);

    let receipt = refusal_for(&mut provider, channel_id, "turn-four", &target, &seat)
        .await
        .expect("the fourth turn is answered");
    assert_eq!(receipt["status"], "turn_refused", "{receipt}");
    assert_eq!(receipt["error"]["code"], BUDGET_EXHAUSTED, "{receipt}");
    let message = receipt["error"]["message"]
        .as_str()
        .expect("message")
        .to_owned();
    assert!(
        message.contains("3 of the 3"),
        "the refusal names used and limit: {message}"
    );
    assert!(
        message.contains("policy"),
        "the refusal says which ceiling bound: {message}"
    );
}

/// An umbrella with no policy is exactly the D9 path it always was: the
/// environment ceiling, and a message that names the setting, not a policy
/// nobody published.
#[tokio::test]
async fn no_policy_leaves_the_environment_budget_exactly_as_it_was() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, seat, umbrella, target) = budgeted_umbrella(&dir, 2);
    let session_id = target.session_id.clone();

    spend_turns(&mut provider, &session_id, 2);
    let receipt = refusal_for(&mut provider, channel_id, "turn-three", &target, &seat)
        .await
        .expect("the third turn is answered");
    assert_eq!(receipt["error"]["code"], BUDGET_EXHAUSTED, "{receipt}");
    let message = receipt["error"]["message"]
        .as_str()
        .expect("message")
        .to_owned();
    assert!(
        message.contains("2 of its 2 allowed turns"),
        "unchanged wording: {message}"
    );
    assert!(
        !message.contains("policy"),
        "an environment ceiling must not claim a policy bound it: {message}"
    );
    assert_eq!(provider.state().turns_used(&umbrella), 2);
}

/// A policy is a ceiling on delegated work. The founder it was protecting is
/// still exempt — the same rule the environment budget has always had.
#[tokio::test]
async fn a_policy_budget_never_refuses_the_umbrellas_founder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, _seat, umbrella, target) =
        budgeted_umbrella(&dir, config::UNLIMITED_TURN_BUDGET);
    provider.set_policy_turn_budget(&umbrella, Some(1));
    let session_id = target.session_id.clone();
    spend_turns(&mut provider, &session_id, 1);

    let founder_turn = turn_event(channel_id, "turn-founder", &target);
    provider
        .handle_command_event(channel_id, &founder_turn)
        .await
        .expect("founder turn");
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        !sink
            .contents_of(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .into_iter()
            .any(|receipt| receipt["commandId"] == "turn-founder"
                && receipt["error"]["code"] == BUDGET_EXHAUSTED),
        "a founder turn is never refused for a budget"
    );
}

/// The learning half: a create under an umbrella that has a published policy
/// reads it off the relay and records the ceiling it will enforce.
#[tokio::test]
async fn a_create_reads_the_umbrellas_published_policy() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let genesis = genesis_event(channel_id, session_ref);

    let payload = buzz_core::coding_session_policy::CodingSessionPolicyPayload {
        budget: Some(
            buzz_core::coding_session_policy::CodingSessionPolicyBudget {
                turns: Some(12),
                tokens_per_seat: None,
                tokens_per_session: None,
                cost_usd_per_session: None,
                context_tier: None,
            },
        ),
        ..buzz_core::coding_session_policy::CodingSessionPolicyPayload::empty(
            session_ref,
            genesis.id.to_hex(),
        )
    };
    let policy = buzz_sdk::coding_session_policy::build_coding_session_policy(
        &channel_id.to_string(),
        payload,
    )
    .expect("policy builder")
    .sign_with_keys(test_operator_keys())
    .expect("sign policy");

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-under-policy",
        session_ref,
        &genesis.id.to_hex(),
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![genesis.clone(), policy]).await;

    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");

    assert_eq!(
        provider.policy_turn_budget(session_ref),
        Some(12),
        "the create learns the ceiling its umbrella published"
    );

    relay.shutdown().await;
    server.abort();
}

/// A policy signed by somebody with no standing in this umbrella binds
/// nothing. The relay's own gate for a stored 44245 is channel membership, so
/// without this any member of the room could publish a `turns: 1` and stop the
/// crew — or a `turns: 1000000` and lift the ceiling the operator set.
#[tokio::test]
async fn a_strangers_policy_binds_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));
    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let genesis = genesis_event(channel_id, session_ref);

    let payload = buzz_core::coding_session_policy::CodingSessionPolicyPayload {
        budget: Some(
            buzz_core::coding_session_policy::CodingSessionPolicyBudget {
                turns: Some(1),
                tokens_per_seat: None,
                tokens_per_session: None,
                cost_usd_per_session: None,
                context_tier: None,
            },
        ),
        ..buzz_core::coding_session_policy::CodingSessionPolicyPayload::empty(
            session_ref,
            genesis.id.to_hex(),
        )
    };
    let policy = buzz_sdk::coding_session_policy::build_coding_session_policy(
        &channel_id.to_string(),
        payload,
    )
    .expect("policy builder")
    .sign_with_keys(&Keys::generate())
    .expect("sign policy");

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-under-stranger-policy",
        session_ref,
        &genesis.id.to_hex(),
    );
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, vec![genesis.clone(), policy]).await;

    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");

    assert_eq!(
        provider.policy_turn_budget(session_ref),
        None,
        "only the founder or a granted operator may set this session's policy"
    );

    relay.shutdown().await;
    server.abort();
}

/// A `budget.turns` policy for `session_ref`, signed by `signer`.
fn policy_event(
    channel_id: Uuid,
    session_ref: &str,
    genesis_ref: &str,
    turns: u32,
    signer: &Keys,
) -> Event {
    let payload = buzz_core::coding_session_policy::CodingSessionPolicyPayload {
        budget: Some(
            buzz_core::coding_session_policy::CodingSessionPolicyBudget {
                turns: Some(turns),
                tokens_per_seat: None,
                tokens_per_session: None,
                cost_usd_per_session: None,
                context_tier: None,
            },
        ),
        ..buzz_core::coding_session_policy::CodingSessionPolicyPayload::empty(
            session_ref,
            genesis_ref.to_owned(),
        )
    };
    buzz_sdk::coding_session_policy::build_coding_session_policy(&channel_id.to_string(), payload)
        .expect("policy builder")
        .sign_with_keys(signer)
        .expect("sign policy")
}

/// A governed umbrella whose authority chain already grants one operator, and
/// a relay holding the genesis, the transition, its acceptance receipt, and
/// whatever `extra_policies` builds from the channel, session ref and genesis
/// ref.
///
/// This is the create path a grant-holder's policy has to survive: the grant
/// exists on the relay *before* the session does, so only the create's own
/// authority backfill can make the grantee known to the policy fold.
async fn create_under_accepted_grant(
    dir: &tempfile::TempDir,
    grantee: &Keys,
    extra_policies: impl Fn(Uuid, &str, &str) -> Vec<Event>,
) -> (Provider, String, HarnessRelay, tokio::task::JoinHandle<()>) {
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let mut provider = provider(&dir.path().join("state"), Some(&projects));

    let relay_keys = Keys::generate();
    provider.set_relay_self(relay_keys.public_key().to_hex());

    let session_ref = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    let genesis = genesis_event(channel_id, session_ref);
    let genesis_ref = genesis.id.to_hex();
    let grantee_hex = grantee.public_key().to_hex();
    let transition = grant_transition_event(channel_id, &genesis_ref, None, 1, &grantee_hex);
    let receipt = acceptance_receipt_event(
        &relay_keys,
        channel_id,
        &genesis_ref,
        &transition,
        1,
        &grantee_hex,
    );

    let mut events = vec![genesis.clone(), transition, receipt];
    events.extend(extra_policies(channel_id, session_ref, &genesis_ref));
    let (mut relay, _queries, server) =
        spawn_test_relay_with_events(&provider.config.keys, events).await;

    let create = create_event_with_genesis_ref(
        &provider,
        channel_id,
        "create-under-granted-policy",
        session_ref,
        &genesis_ref,
    );
    provider
        .handle_relay_event(&mut relay, channel_id, &create)
        .await
        .expect("create");

    (provider, session_ref.to_owned(), relay, server)
}

/// F3. The case the create path used to miss entirely: an operator holding an
/// accepted grant on this umbrella publishes the policy, and the create — not
/// the *next* resume — has to fold it.
///
/// The grant is only knowable from the create's own authority backfill, so
/// this test fails outright whenever the policy read runs before that backfill.
#[tokio::test]
async fn a_grant_holders_policy_binds_a_create() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lead = Keys::generate();
    let (provider, session_ref, relay, server) =
        create_under_accepted_grant(&dir, &lead, |channel_id, session_ref, genesis_ref| {
            vec![policy_event(channel_id, session_ref, genesis_ref, 7, &lead)]
        })
        .await;

    assert_eq!(
        provider.policy_turn_budget(&session_ref),
        Some(7),
        "an operator holding an accepted grant may set this umbrella's policy, and the create \
         that folded the grant must enforce it"
    );

    relay.shutdown().await;
    server.abort();
}

/// The other half of the same rule: folding the grants *first* must not widen
/// who may publish a policy. An identity with no accepted grant binds nothing,
/// even on a create whose chain granted somebody else.
#[tokio::test]
async fn an_ungranted_leads_policy_does_not_bind_a_create() {
    let dir = tempfile::tempdir().expect("tempdir");
    let granted = Keys::generate();
    let ungranted = Keys::generate();
    let (provider, session_ref, relay, server) =
        create_under_accepted_grant(&dir, &granted, |channel_id, session_ref, genesis_ref| {
            vec![policy_event(
                channel_id,
                session_ref,
                genesis_ref,
                1,
                &ungranted,
            )]
        })
        .await;

    assert_eq!(
        provider.policy_turn_budget(&session_ref),
        None,
        "a lead without an accepted operator grant sets no ceiling on this umbrella"
    );

    relay.shutdown().await;
    server.abort();
}

/// The founder's own policy binds whatever else the chain says. Moving the
/// policy read after the backfill must not cost the one signer who never
/// needed a grant.
#[tokio::test]
async fn the_founders_policy_binds_a_create_that_also_folds_a_grant() {
    let dir = tempfile::tempdir().expect("tempdir");
    let granted = Keys::generate();
    let (provider, session_ref, relay, server) =
        create_under_accepted_grant(&dir, &granted, |channel_id, session_ref, genesis_ref| {
            vec![policy_event(
                channel_id,
                session_ref,
                genesis_ref,
                9,
                test_operator_keys(),
            )]
        })
        .await;

    assert_eq!(
        provider.policy_turn_budget(&session_ref),
        Some(9),
        "the umbrella's founder sets its policy with no grant of any kind"
    );

    relay.shutdown().await;
    server.abort();
}
