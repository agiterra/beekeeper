use super::*;
use crate::test_transport::Client as BeekeeperClient;

#[tokio::test]
async fn old_signed_journal_events_reuse_only_exact_authorized_stored_publication() {
    use axum::{
        extract::State,
        response::{IntoResponse, Response},
        routing::post,
        Json, Router,
    };
    use std::{
        future::IntoFuture,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc, Mutex,
        },
    };
    #[derive(Clone)]
    struct Fixture {
        rows: Arc<Mutex<Vec<Value>>>,
        publications: Arc<AtomicUsize>,
        deny_query: Arc<AtomicBool>,
    }
    async fn query(State(fixture): State<Fixture>, Json(filters): Json<Vec<Value>>) -> Response {
        if fixture.deny_query.load(Ordering::SeqCst) {
            return (axum::http::StatusCode::FORBIDDEN, "channel scope refused").into_response();
        }
        let rows = fixture.rows.lock().expect("rows").clone();
        if let Some(row) = rows.first() {
            let filter = &filters[0];
            // The real relay receives exact kind/id/author and, for channel
            // operations, h scope. A broad unscoped read cannot certify reuse.
            assert_eq!(filter["kinds"], json!([row["kind"]]));
            assert_eq!(filter["ids"], json!([row["id"]]));
            assert_eq!(filter["authors"], json!([row["pubkey"]]));
            if let Some(channel) = row["tags"]
                .as_array()
                .expect("tags")
                .iter()
                .find(|tag| tag[0] == "h")
                .and_then(|tag| tag[1].as_str())
            {
                assert_eq!(filter["#h"], json!([channel]));
            }
        }
        Json(rows).into_response()
    }
    async fn publish(State(fixture): State<Fixture>) -> (axum::http::StatusCode, &'static str) {
        fixture.publications.fetch_add(1, Ordering::SeqCst);
        (
            axum::http::StatusCode::BAD_REQUEST,
            "invalid: event timestamp too far from server time",
        )
    }
    let actor = Keys::generate();
    let profile = nostr::EventBuilder::new(nostr::Kind::Metadata, "{\"name\":\"old actor\"}")
        .custom_created_at(Timestamp::from(1))
        .sign_with_keys(&actor)
        .expect("old signed profile");
    let fixture = Fixture {
        rows: Arc::new(Mutex::new(vec![
            serde_json::to_value(&profile).expect("row")
        ])),
        publications: Arc::new(AtomicUsize::new(0)),
        deny_query: Arc::new(AtomicBool::new(false)),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/query", post(query))
                .route("/events", post(publish))
                .with_state(fixture.clone()),
        )
        .into_future(),
    );
    let client = BeekeeperClient::new(url.clone(), actor.clone(), None, None).expect("client");
    submit(&client, &profile)
        .await
        .expect("exact old publication reused");
    assert_eq!(fixture.publications.load(Ordering::SeqCst), 0);
    let channel = Uuid::new_v4().to_string();
    for kind in [9000, 44221] {
        let persisted = nostr::EventBuilder::new(nostr::Kind::Custom(kind), "persisted request")
            .tags([Tag::parse(["h", &channel]).expect("channel")])
            .custom_created_at(Timestamp::from(1))
            .sign_with_keys(&actor)
            .expect("old signed request");
        *fixture.rows.lock().expect("rows") = vec![serde_json::to_value(&persisted).expect("row")];
        submit(&client, &persisted)
            .await
            .expect("accepted old request reused");
        assert_eq!(fixture.publications.load(Ordering::SeqCst), 0);
        fixture.deny_query.store(true, Ordering::SeqCst);
        assert!(
            submit(&client, &persisted).await.is_err(),
            "denied channel query fails closed"
        );
        assert_eq!(fixture.publications.load(Ordering::SeqCst), 0);
        fixture.deny_query.store(false, Ordering::SeqCst);
        fixture.rows.lock().expect("rows").clear();
        assert!(
            submit(&client, &persisted).await.is_err(),
            "unpublished old request refuses"
        );
        fixture.publications.store(0, Ordering::SeqCst);
    }
    let wrong_signer =
        BeekeeperClient::new(url, Keys::generate(), None, None).expect("other signer");
    assert!(matches!(
        submit(&wrong_signer, &profile).await,
        Err(CliError::Auth(_))
    ));
    fixture.rows.lock().expect("rows").clear();
    assert!(
        submit(&client, &profile).await.is_err(),
        "unpublished expired event must refuse"
    );
    assert_eq!(fixture.publications.load(Ordering::SeqCst), 1);
    server.abort();
}

#[test]
fn fresh_actor_membership_is_discoverable_bot_without_rewriting_existing_roles() {
    let owner = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let channel = Uuid::new_v4();
    let fresh = actor_membership(channel, &actor, false)
        .expect("fresh membership")
        .sign_with_keys(&owner)
        .expect("signed membership");
    assert!(fresh.verify().is_ok());
    assert_eq!(fresh.kind, nostr::Kind::Custom(9000));
    let tags: Vec<Vec<String>> = fresh
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert!(tags.contains(&vec!["h".into(), channel.to_string()]));
    assert!(tags.contains(&vec!["p".into(), actor.clone()]));
    assert!(tags.contains(&vec!["role".into(), "bot".into()]));
    let existing = actor_membership(channel, &actor, true)
        .expect("existing membership")
        .sign_with_keys(&owner)
        .expect("signed membership");
    assert!(existing.verify().is_ok());
    assert!(!existing
        .tags
        .iter()
        .any(|tag| tag.as_slice().first().is_some_and(|key| key == "role")));
    // Existing role-less lifecycle wire is the ordinary Solo contract, not a
    // channel membership designation. No team role is added by this change.
    let action = CodingSessionLifecycleAction::SessionCreate {
        project_ref: None,
        repo_ref: None,
        session_ref: None,
        genesis_ref: None,
        provider_instance_ref: ProviderInstanceAlias::from_wire("codex-primary").expect("alias"),
        provider_authority_pubkey: owner.public_key().to_hex(),
        model: None,
        title: None,
        initial_turn: None,
        actor: Some(actor),
        role: None,
        hire_ref: None,
        routing: None,
    };
    let wire = serde_json::to_value(action).expect("lifecycle wire");
    assert!(wire.get("role").is_none_or(Value::is_null));
}

#[test]
fn duplicate_acceptance_requires_exact_signed_event_not_another_replaceable_head() {
    let keys = Keys::generate();
    let event = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{\"name\":\"one\"}")
        .tags([Tag::parse(["d", &keys.public_key().to_hex()]).expect("d")])
        .sign_with_keys(&keys)
        .expect("event");
    let replacement = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{\"name\":\"two\"}")
        .tags(event.tags.iter().cloned())
        .sign_with_keys(&keys)
        .expect("replacement");
    assert!(exact_event_stored(
        &[serde_json::to_value(&event).expect("json")],
        &event
    ));
    assert!(!exact_event_stored(
        &[serde_json::to_value(&replacement).expect("json")],
        &event
    ));
    let mut forged = serde_json::to_value(&event).expect("json");
    forged["content"] = json!("altered");
    assert!(!exact_event_stored(&[forged], &event));
    assert!(!exact_event_stored(&[], &event));
}

#[test]
fn signed_profile_uses_actor_transport_and_management_uses_owner_transport() {
    let owner_keys = Keys::generate();
    let actor_keys = Keys::generate();
    let unknown = Keys::generate();
    let owner = BeekeeperClient::new(
        "http://localhost:3000".into(),
        owner_keys.clone(),
        None,
        None,
    )
    .expect("owner");
    let actor = BeekeeperClient::new(
        "http://localhost:3000".into(),
        actor_keys.clone(),
        None,
        None,
    )
    .expect("actor");
    let profile = nostr::EventBuilder::new(nostr::Kind::Metadata, "{}")
        .sign_with_keys(&actor_keys)
        .expect("profile");
    assert_eq!(
        event_transport(&profile, &owner, &actor)
            .expect("transport")
            .keys()
            .public_key(),
        actor_keys.public_key()
    );
    let definition = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{}")
        .sign_with_keys(&owner_keys)
        .expect("definition");
    assert_eq!(
        event_transport(&definition, &owner, &actor)
            .expect("transport")
            .keys()
            .public_key(),
        owner_keys.public_key()
    );
    let delegated_management = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{}")
        .sign_with_keys(&actor_keys)
        .expect("invalid delegation");
    assert!(event_transport(&delegated_management, &owner, &actor).is_err());
    let wrong_profile = nostr::EventBuilder::new(nostr::Kind::Metadata, "{}")
        .sign_with_keys(&unknown)
        .expect("unknown");
    assert!(event_transport(&wrong_profile, &owner, &actor).is_err());
}

#[test]
fn operation_retry_keeps_exact_events_and_reused_id_conflicts() {
    let dir = tempfile::tempdir().expect("home");
    let path = dir.path().join("operation.json");
    let keys = Keys::generate();
    let event = nostr::EventBuilder::new(nostr::Kind::Custom(9), "work")
        .sign_with_keys(&keys)
        .expect("event");
    let operation = Operation {
        request: json!({"name":"agent"}),
        actor_pubkey: keys.public_key().to_hex(),
        events: vec![event.clone()],
        target: None,
        receipt_event_id: None,
        predecessor: None,
        custody_handoff: None,
    };
    save(&path, &operation).expect("reserve");
    let retried = read_operation(&path, &operation.request)
        .expect("retry")
        .expect("operation");
    assert_eq!(retried.events[0].id, event.id);
    assert!(matches!(
        read_operation(&path, &json!({"name":"changed"})),
        Err(CliError::Conflict(_))
    ));
    assert_eq!(
        read_operation(&path, &operation.request)
            .expect("original")
            .expect("present")
            .actor_pubkey,
        operation.actor_pubkey
    );
}

#[test]
fn custody_retry_checks_identity_and_refuses_replacement() {
    let dir = tempfile::tempdir().expect("state");
    let owner = Keys::generate();
    let actor = provision::mint_agent_identity(&owner).expect("mint");
    actor_custody(
        dir.path(),
        &actor.pubkey,
        Some((&actor.private_key_nsec, &actor.auth_tag)),
    )
    .expect("custody");
    let (key, _) = actor_custody(dir.path(), &actor.pubkey, None).expect("retry");
    assert_eq!(key, actor.private_key_nsec);
    assert!(actor_custody(dir.path(), &Keys::generate().public_key().to_hex(), None).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join("actors").join(&actor.pubkey).join("key"))
                .expect("metadata")
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[test]
fn route_reconciliation_preserves_other_responsibilities_without_duplicates() {
    let dir = tempfile::tempdir().expect("state");
    let actor = Keys::generate().public_key().to_hex();
    let channel = Uuid::new_v4();
    let mut target = CodingSessionTarget {
        driver: "codex".into(),
        instance_id: "provider".into(),
        session_id: "one".into(),
        generation: 1,
    };
    install_route(dir.path(), &actor, channel, &target, None).expect("one");
    install_route(dir.path(), &actor, channel, &target, None).expect("replay");
    target.session_id = "two".into();
    install_route(dir.path(), &actor, channel, &target, None).expect("two");
    let routes: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("conversation-routes.json")).expect("read"),
    )
    .expect("routes");
    assert_eq!(routes["routes"].as_array().expect("array").len(), 2);
}

#[test]
fn ordinary_non_git_workspace_is_allowed_while_custody_and_ancestors_are_refused() {
    let dir = tempfile::tempdir().expect("isolated");
    let home = dir.path().join("service");
    let state = home.join("state");
    let workspace = dir.path().join("writing");
    std::fs::create_dir_all(&state).expect("state");
    std::fs::create_dir_all(&workspace).expect("workspace");
    assert!(validate_checkout(&workspace, &home, &state, None).is_ok());
    assert!(validate_checkout(&home, &home, &state, None).is_err());
    assert!(validate_checkout(dir.path(), &home, &state, None).is_err());
    assert!(validate_checkout(&state, &home, &state, None).is_err());
}

#[tokio::test]
async fn owner_signing_material_and_reserved_identity_env_are_refused_before_journal_or_custody() {
    let owner = Keys::generate();
    let client = BeekeeperClient::new("http://127.0.0.1:1".into(), owner.clone(), None, None)
        .expect("client");
    let home = tempfile::tempdir().expect("home");
    let context = Context {
        home: home.path().into(),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    for record in [
        json!({"effective_env":{"MODEL_TOKEN":owner.secret_key().to_secret_hex()}}),
        json!({"env_vars":{"BEEKEEPER_PRIVATE_KEY":"any-scoped-or-owner-key"}}),
    ] {
        let request = CreateRequest {
            operation_id: Uuid::new_v4(),
            wait_secs: 1,
            activate: false,
            private_config: Some(crate::PrivateConfiguration { record }),
            ..Default::default()
        };
        assert!(matches!(
            create(&request, &client, &context).await,
            Err(CliError::Usage(_))
        ));
        assert!(
            !layout::host_dir(home.path(), layout::Instance::Dev).exists(),
            "no journal or custody was created for rejected secret settings"
        );
    }
}
#[test]
fn route_replay_boundary_is_immutable_for_the_same_accepted_target() {
    let dir = tempfile::tempdir().expect("state");
    let actor = Keys::generate().public_key().to_hex();
    let channel = Uuid::new_v4();
    let target = CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "provider".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    install_route(dir.path(), &actor, channel, &target, Some(123)).expect("route");
    install_route(dir.path(), &actor, channel, &target, Some(123)).expect("same route");
    assert!(matches!(
        install_route(dir.path(), &actor, channel, &target, Some(124)),
        Err(CliError::Conflict(_))
    ));
    let value: Value = serde_json::from_slice(
        &std::fs::read(dir.path().join("conversation-routes.json")).expect("routes"),
    )
    .expect("JSON");
    assert_eq!(value["routes"].as_array().expect("routes").len(), 1);
    assert_eq!(value["routes"][0]["replayFloorUnix"], 123);
}
