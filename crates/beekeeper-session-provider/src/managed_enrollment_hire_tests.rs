//! An enrolled identity keeps the team hire path (ledger 401). The C4 run's
//! Codex hire of a host-enrolled `builder` was refused AGENT_ENROLLMENT_UNAVAILABLE:
//! the provider took a desktop-staged team-hire create for a managed Solo start.
use super::*;
use nostr::EventBuilder;

struct Fixture {
    _dir: tempfile::TempDir,
    host: crate::Provider,
    channel: Uuid,
    actor: Keys,
    enrollment: std::path::PathBuf,
    routes: std::path::PathBuf,
    _server: tokio::task::JoinHandle<()>,
}

/// A host-enrolled actor (custody, attestation, enrollment) whose hire the
/// desktop answered by staging the seat in actor-seat custody.
async fn enrolled_with_staged_seat() -> Fixture {
    let dir = tempfile::tempdir().expect("provider");
    let channel = Uuid::new_v4();
    let cwd = dir.path().join("work");
    std::fs::create_dir(&cwd).expect("workspace");
    let cwd = cwd.canonicalize().expect("canonical workspace");
    let projects = crate::tests::write_projects(dir.path(), channel, &cwd);
    let mut host = crate::tests::provider(&dir.path().join("state"), Some(&projects));
    let owner = crate::tests::test_operator_keys();
    let actor = Keys::generate();
    let (relay, _, server) =
        crate::tests::spawn_test_relay_with_events(&host.config.keys, vec![]).await;
    host.set_rest_client(relay.rest_client());
    host.set_relay_self(host.config.keys.public_key().to_hex());
    let auth =
        beekeeper_sdk::nip_oa::compute_auth_tag(owner, &actor.public_key(), "").expect("auth");
    let seats = dir.path().join("actor-seats.json");
    write(&seats, &json!({"version":1,"pending":{"hire":{"pubkey":actor.public_key().to_hex(),"nsec":actor.secret_key().to_secret_hex(),"authTag":auth,"relayUrl":host.config.relay_url}}})).expect("seat");
    host.config.actor_seats_file = Some(seats);
    let actor_dir = host
        .config
        .state_dir
        .join("actors")
        .join(actor.public_key().to_hex());
    atomic_write::create_dir_all_restricted(&actor_dir).expect("custody");
    crate::state::atomic_write(
        &actor_dir.join("key"),
        actor.secret_key().to_secret_hex().as_bytes(),
    )
    .expect("key");
    crate::state::atomic_write(&actor_dir.join("auth-tag.json"), auth.as_bytes())
        .expect("auth custody");
    let enrollment = actor_dir.join("enrollment.json");
    write(&enrollment, &json!({"actorPubkey":actor.public_key().to_hex(),"providerInstance":"codex-primary","channelId":Uuid::new_v4(),"cwd":cwd,"projectRef":null})).expect("enrollment");
    let routes = host.config.state_dir.join("conversation-routes.json");
    Fixture {
        _dir: dir,
        host,
        channel,
        actor,
        enrollment,
        routes,
        _server: server,
    }
}

fn hire_content(fixture: &Fixture, command: &str) -> (nostr::Kind, Vec<nostr::Tag>, String) {
    let create = crate::tests::create_event(&fixture.host, fixture.channel, command);
    let mut payload: Value = serde_json::from_str(&create.content).expect("create");
    payload["action"]["actor"] = json!(fixture.actor.public_key().to_hex());
    payload["action"]["role"] = json!("builder");
    (create.kind, create.tags.to_vec(), payload.to_string())
}

#[tokio::test]
async fn the_owner_hires_an_enrolled_identity_on_its_staged_seat_without_restaging() {
    let mut fixture = enrolled_with_staged_seat().await;
    let enrolled = std::fs::read(&fixture.enrollment).expect("enrollment bytes");
    let (kind, tags, content) = hire_content(&fixture, "hire");
    let event = EventBuilder::new(kind, content)
        .tags(tags)
        .sign_with_keys(crate::tests::test_operator_keys())
        .expect("owner-signed hire");
    fixture
        .host
        .handle_command_event(fixture.channel, &event)
        .await
        .expect("handled");
    let record = fixture
        .host
        .state
        .sessions()
        .next()
        .expect("the hire is seated, not refused AGENT_ENROLLMENT_UNAVAILABLE")
        .clone();
    assert_eq!(
        record.actor.as_deref(),
        Some(fixture.actor.public_key().to_hex().as_str())
    );
    assert_eq!(
        std::fs::read(&fixture.enrollment).expect("enrollment"),
        enrolled,
        "a team hire restages nothing"
    );
    assert!(
        !fixture.routes.exists(),
        "a team-hire seat is never recorded as the identity's primary route"
    );
}

#[tokio::test]
async fn only_the_attested_owner_may_hire_an_enrolled_identity() {
    let fixture = enrolled_with_staged_seat().await;
    let (_, _, content) = hire_content(&fixture, "stranger");
    let stranger = Keys::generate().public_key().to_hex();
    let refusal = stage_with_context(
        &fixture.host.config,
        &fixture.host.state,
        None,
        fixture.channel,
        &stranger,
        &content,
        None,
    )
    .await
    .expect_err("a stranger's hire stays refused");
    assert_eq!(refusal.code, "AGENT_ENROLLMENT_UNAVAILABLE");
    assert!(
        refusal.message.contains("attested owner"),
        "{}",
        refusal.message
    );
}

/// A team hire never crosses an unresolved managed start or an unquiesced stop
/// of the same identity (ledger 403): both refuse before any seat is staged.
#[tokio::test]
async fn a_team_hire_waits_for_an_unresolved_start_and_an_unquiesced_stop() {
    let mut fixture = enrolled_with_staged_seat().await;
    let owner = crate::tests::test_operator_keys().public_key().to_hex();
    let actor = fixture.actor.public_key().to_hex();

    let (kind, tags, content) = hire_content(&fixture, "hire");
    let seated = EventBuilder::new(kind, content)
        .tags(tags)
        .sign_with_keys(crate::tests::test_operator_keys())
        .expect("owner-signed hire");
    fixture
        .host
        .handle_command_event(fixture.channel, &seated)
        .await
        .expect("handled");
    let record = fixture
        .host
        .state
        .sessions()
        .next()
        .expect("seated")
        .clone();

    // Unquiesced stop of that seat.
    let marker = fixture
        .host
        .config
        .state_dir
        .join(format!("stop-quiescence-{}.json", record.session_id));
    write(&marker, &json!({"pending": true})).expect("quiescence marker");
    let (_, _, content) = hire_content(&fixture, "after-stop");
    let refused = stage_with_context(
        &fixture.host.config,
        &fixture.host.state,
        None,
        fixture.channel,
        &owner,
        &content,
        None,
    )
    .await
    .expect_err("unquiesced stop");
    assert!(
        refused.message.contains("quiescence"),
        "{}",
        refused.message
    );
    std::fs::remove_file(&marker).expect("quiesced");

    // Unresolved managed start of the identity.
    fixture
        .host
        .state
        .stage_terminal_disposition(
            "starting",
            crate::state::TerminalDisposition {
                semantic_key: "starting".into(),
                event: seated.clone(),
                lifecycle_start: Some(crate::state::LifecycleStartIntent {
                    phase: crate::state::LifecycleStartPhase::RuntimeStarting,
                    fingerprint: "retained".into(),
                    target: fixture.host.target_for(&record),
                    actor: Some(actor),
                    channel: fixture.channel,
                    provider_ref: record.provider_instance_ref.clone(),
                    runtime: record.runtime.clone(),
                    initial_turn: false,
                }),
            },
        )
        .expect("unresolved start");
    let (_, _, content) = hire_content(&fixture, "after-start");
    let refused = stage_with_context(
        &fixture.host.config,
        &fixture.host.state,
        None,
        fixture.channel,
        &owner,
        &content,
        None,
    )
    .await
    .expect_err("unresolved start");
    assert!(
        refused.message.contains("startup outcome is unconfirmed"),
        "{}",
        refused.message
    );
}
