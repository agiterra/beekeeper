use super::*;
use crate::test_transport::Client as BeekeeperClient;
use beekeeper_core::coding_session_payload::{LifecycleReceipt, LIFECYCLE_RECEIPT_SCHEMA};
use std::future::IntoFuture;

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "provider-instance".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    }
}
fn receipt(keys: &Keys, channel: Uuid, target: &CodingSessionTarget) -> Value {
    let command = Uuid::new_v4().to_string();
    let body = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: command.clone(),
        status: ReceiptStatus::Stopped,
        session: Some(target.clone()),
        error: None,
        turn_id: None,
        rewind: None,
    };
    let event = beekeeper_sdk::builders::build_coding_session_lifecycle_receipt(
        channel,
        &command,
        &serde_json::to_string(&body).expect("body"),
    )
    .expect("builder")
    .sign_with_keys(keys)
    .expect("signed");
    serde_json::to_value(event).expect("row")
}

#[test]
fn successor_requires_exact_signed_provider_stop_not_wrong_generation_or_channel() {
    let provider = Keys::generate();
    let other = Keys::generate();
    let channel = Uuid::new_v4();
    let old = target();
    let row = receipt(&provider, channel, &old);
    assert!(stopped_receipt(
        std::slice::from_ref(&row),
        &channel.to_string(),
        &provider.public_key().to_hex(),
        &old
    ));
    let mut replacement = old.clone();
    replacement.generation += 1;
    assert!(!stopped_receipt(
        std::slice::from_ref(&row),
        &channel.to_string(),
        &provider.public_key().to_hex(),
        &replacement
    ));
    assert!(!stopped_receipt(
        std::slice::from_ref(&row),
        &Uuid::new_v4().to_string(),
        &provider.public_key().to_hex(),
        &old
    ));
    assert!(!stopped_receipt(
        &[receipt(&other, channel, &old)],
        &channel.to_string(),
        &provider.public_key().to_hex(),
        &old
    ));
    let mut forged = row;
    forged["content"] = json!("tampered");
    assert!(!stopped_receipt(
        &[forged],
        &channel.to_string(),
        &provider.public_key().to_hex(),
        &old
    ));
}

#[test]
fn stopped_actor_successor_uses_preserved_pair_and_refuses_restoration() {
    let home = tempfile::tempdir().expect("home");
    let actor = Keys::generate().public_key().to_hex();
    let old = target();
    let relay = "wss://isolated.example";
    let config = HostConfig {
        version: beekeeper_host_core::config::HOST_CONFIG_VERSION,
        instance: layout::Instance::Dev,
        relay_url: relay.into(),
        provider_pubkey: "cd".repeat(32),
        session_provider_base_dir: home.path().into(),
        provider_state_dir: home.path().into(),
        runtimes: vec![],
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        provider_command: None,
        written_at: String::new(),
    };
    let enrolled_op = Uuid::new_v4();
    ownership::record_host_enrollment(
        home.path(),
        config.instance,
        &actor,
        relay,
        &enrolled_op.to_string(),
        &"ab".repeat(32),
        Some(old.clone()),
    )
    .expect("modern enrollment");
    ownership::host_stopped(home.path(), config.instance, &actor, relay, &old).expect("stopped");
    let next = Uuid::new_v4();
    let boundary = prepare(home.path(), &config, Some(&actor), next)
        .expect("eligible")
        .expect("boundary");
    assert_eq!(boundary.target, Some(old));
    assert!(prepare(home.path(), &config, Some(&actor), enrolled_op).is_err());
    let new_target = target();
    ownership::successor(
        home.path(),
        config.instance,
        &actor,
        relay,
        &next.to_string(),
        "ef".repeat(32).as_str(),
        new_target.clone(),
    )
    .expect("canonical transition");
    let adopted = ownership::for_pair(home.path(), config.instance, &actor, relay)
        .expect("read")
        .expect("adopted");
    assert_eq!(adopted.phase, Phase::Adopted);
    assert_eq!(adopted.target, Some(new_target));
    assert_eq!(adopted.operation_id, next.to_string());
    ownership::successor(
        home.path(),
        config.instance,
        &actor,
        relay,
        &next.to_string(),
        &adopted.definition_event_id,
        adopted.target.clone().expect("target"),
    )
    .expect("idempotent");
    let journal_path =
        layout::host_dir(home.path(), config.instance).join("managed-ownership.json");
    let mut journal: Value =
        serde_json::from_slice(&std::fs::read(&journal_path).expect("journal")).expect("decode");
    journal["transfers"][0]["phase"] = json!("restoring");
    save(&journal_path, &journal).expect("historical phase fixture");
    assert!(prepare(home.path(), &config, Some(&actor), Uuid::new_v4()).is_err());
}

#[tokio::test]
async fn canonical_cli_successor_fences_other_contexts_and_reconciles_without_desktop() {
    use axum::{extract::State, routing::post, Json, Router};
    use std::sync::{Arc, Mutex};
    let home = tempfile::tempdir().expect("home");
    let state = home.path().join("provider");
    std::fs::create_dir(&state).expect("state");
    let owner = Keys::generate();
    let provider =
        provision::mint_provider_record(&owner, "wss://isolated.example").expect("provider");
    let signer = Keys::parse(&provider.private_key_nsec).expect("provider signer");
    let actor = Keys::generate().public_key().to_hex();
    let old = CodingSessionTarget {
        instance_id: provider.instance_id.clone(),
        ..target()
    };
    let other = CodingSessionTarget {
        instance_id: provider.instance_id.clone(),
        ..target()
    };
    let next = CodingSessionTarget {
        instance_id: provider.instance_id.clone(),
        ..target()
    };
    let operation_id = Uuid::new_v4();
    let channel = Uuid::new_v4();
    let record = |target: &CodingSessionTarget, command: &str| {
        json!({
            "sessionId":target.session_id,"generation":target.generation,"channelId":channel,"commandId":command,
            "driver":target.driver,"cwd":state,"actor":actor,"createdAtMs":1,"nextSeq":1,"closed":true
        })
    };
    save(
        &state.join("state.json"),
        &json!({"sessions":{
            &old.session_id:record(&old,"prior"), &other.session_id:record(&other,"other"),
            &next.session_id:record(&next,&operation_id.to_string())
        }}),
    )
    .expect("inventory");
    let rows = Arc::new(Mutex::new(vec![receipt(&signer, channel, &old)]));
    async fn query(State(rows): State<Arc<Mutex<Vec<Value>>>>) -> Json<Vec<Value>> {
        Json(rows.lock().expect("test rows").clone())
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(
        axum::serve(
            listener,
            Router::new()
                .route("/query", post(query))
                .with_state(rows.clone()),
        )
        .into_future(),
    );
    let client = BeekeeperClient::new(url.clone(), owner.clone(), None, None).expect("client");
    let config = HostConfig {
        version: beekeeper_host_core::config::HOST_CONFIG_VERSION,
        instance: layout::Instance::Dev,
        relay_url: url.clone(),
        provider_pubkey: provider.provider_pubkey.clone(),
        session_provider_base_dir: home.path().into(),
        provider_state_dir: state,
        runtimes: vec![],
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        provider_command: None,
        written_at: String::new(),
    };
    ownership::record_host_enrollment(
        home.path(),
        config.instance,
        &actor,
        &url,
        &Uuid::new_v4().to_string(),
        &"ab".repeat(32),
        Some(old.clone()),
    )
    .expect("modern enrollment");
    ownership::host_stopped(
        home.path(),
        config.instance,
        &actor,
        &config.relay_url,
        &old,
    )
    .expect("stopped");
    let boundary = prepare(home.path(), &config, Some(&actor), operation_id)
        .expect("prepare")
        .expect("boundary");
    assert!(
        require_prior_stops(
            &config,
            &provider,
            &boundary,
            operation_id,
            Some(&next),
            &client
        )
        .await
        .is_err(),
        "an unstopped second responsibility must prevent a successor"
    );
    rows.lock()
        .expect("rows")
        .push(receipt(&signer, channel, &other));
    require_prior_stops(
        &config,
        &provider,
        &boundary,
        operation_id,
        Some(&next),
        &client,
    )
    .await
    .expect("all prior responsibilities stopped");
    let event = nostr::EventBuilder::new(nostr::Kind::Custom(30177), "{}")
        .sign_with_keys(&owner)
        .expect("definition");
    let operation = Operation {
        request: json!({"type":"create"}),
        actor_pubkey: actor.clone(),
        events: vec![event],
        target: Some(next.clone()),
        receipt_event_id: Some("cd".repeat(32)),
        predecessor: Some(boundary.clone()),
        custody_handoff: None,
    };
    reconcile(
        home.path(),
        &config,
        &provider,
        &operation,
        operation_id,
        operation.predecessor.as_ref(),
        &client,
    )
    .await
    .expect("canonical CLI transition");
    let adopted = ownership::for_pair(home.path(), config.instance, &actor, &config.relay_url)
        .expect("journal")
        .expect("adopted");
    assert_eq!(adopted.phase, Phase::Adopted);
    assert_eq!(adopted.target, Some(next));
    assert_eq!(adopted.operation_id, operation_id.to_string());
    reconcile(
        home.path(),
        &config,
        &provider,
        &operation,
        operation_id,
        operation.predecessor.as_ref(),
        &client,
    )
    .await
    .expect("lost reply retry is idempotent");
    server.abort();
}
