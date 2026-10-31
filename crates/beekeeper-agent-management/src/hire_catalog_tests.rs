use super::*;
use beekeeper_core::coding_session_catalog::{Catalog, CatalogProvider, CATALOG_SCHEMA};
use beekeeper_core::coding_session_payload::Capabilities;
use beekeeper_host_core::{config::HostConfig, provision};

pub(super) fn enable_catalog_probe(f: &mut Fixture) -> Keys {
    let provider = provision::mint_provider_record(&f.relay.keys, f.relay.relay_url()).unwrap();
    let keys = Keys::parse(&provider.private_key_nsec).unwrap();
    let config: HostConfig = serde_json::from_value(json!({
        "version":1, "instance":f.context.instance,
        "relayUrl":f.relay.relay_url(), "providerPubkey":provider.provider_pubkey,
        "sessionProviderBaseDir":f.context.home.join("provider"),
        "providerStateDir":f.context.home.join("state"),
        "runtimes":[{"instanceRef":"acme-primary", "driver":"acme-acp", "runtime":"acme",
            "agentCommand":"acme-acp", "defaultModel":"acme-test", "allowedModels":["acme-test"]}],
        "writtenAt":""
    }))
    .unwrap();
    provision::write_commissioning(&f.context.home, &config, &provider).unwrap();
    let mut store = beekeeper_host_core::record::CodingSessionProviderStore::default();
    store.upsert(f.relay.relay_url(), provider);
    std::fs::create_dir_all(&config.session_provider_base_dir).unwrap();
    std::fs::write(
        config.record_store_path(),
        serde_json::to_vec(&store).unwrap(),
    )
    .unwrap();
    let relay = Relay {
        keys: f.relay.keys.clone(),
        rows: Mutex::new(vec![]),
        writes: Mutex::new(vec![]),
        ack_lost: AtomicBool::new(false),
        unavailable: AtomicBool::new(false),
        delegated: false,
        refuse_stop: AtomicBool::new(false),
    };
    f.effects.catalog_probe = Some((
        Context {
            home: f.context.home.clone(),
            instance: f.context.instance,
            native_home: None,
        },
        relay,
    ));
    keys
}
fn catalog(f: &Fixture, keys: &Keys, model: &str) -> Event {
    let body = Catalog {
        schema: CATALOG_SCHEMA.into(),
        revision: 1,
        projects: vec![],
        providers: vec![CatalogProvider {
            provider_instance_ref: "acme-primary".into(),
            driver: "acme-acp".into(),
            runtime: "acme".into(),
            default_model: model.into(),
            allowed_models: vec![model.into()],
            capabilities: Capabilities::v1_for_runtime("acme"),
            models: vec![],
        }],
    };
    beekeeper_sdk::build_coding_session_provider_catalog(
        f.scope.channel,
        1,
        &serde_json::to_string(&body).unwrap(),
    )
    .unwrap()
    .sign_with_keys(keys)
    .unwrap()
}
fn set_rows(f: &Fixture, rows: Vec<Event>) {
    *f.effects
        .catalog_probe
        .as_ref()
        .unwrap()
        .1
        .rows
        .lock()
        .unwrap() = rows;
}

#[tokio::test]
async fn production_catalog_absence_and_unusable_rows_retry_the_same_request() {
    let mut f = fixture();
    let keys = enable_catalog_probe(&mut f);
    let valid = catalog(&f, &keys, "acme-test");
    let mut bad_signature = valid.clone();
    bad_signature.content.push(' ');
    let malformed = beekeeper_sdk::build_coding_session_provider_catalog(f.scope.channel, 1, "{}")
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();
    let wrong_channel =
        beekeeper_sdk::build_coding_session_provider_catalog(Uuid::new_v4(), 1, &valid.content)
            .unwrap()
            .sign_with_keys(&keys)
            .unwrap();
    for rows in [vec![], vec![bad_signature, malformed, wrong_channel]] {
        set_rows(&f, rows);
        assert!(
            matches!(run(&f).await, Err(Error::Unconfirmed(reason)) if reason.contains("HIRE_CATALOG_UNCONFIRMED"))
        );
        assert!(f.relay.writes.lock().unwrap().is_empty());
        assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
    }
    set_rows(&f, vec![valid]);
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_catalog_explicit_exclusion_remains_terminal_after_offer_changes() {
    let mut f = fixture();
    let keys = enable_catalog_probe(&mut f);
    f.effects.disclose.store(true, Ordering::SeqCst);
    set_rows(&f, vec![catalog(&f, &keys, "other-model")]);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused { .. }
    ));
    let first = f.relay.writes.lock().unwrap().clone();
    assert_eq!(first.len(), 1);
    set_rows(&f, vec![catalog(&f, &keys, "acme-test")]);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused { .. }
    ));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
    assert_eq!(*f.relay.writes.lock().unwrap(), first);
}

#[tokio::test]
async fn explicit_bare_model_is_not_the_effort_qualified_listener_selection() {
    let mut f = fixture();
    let local = crate::hire_host::LocalHost {
        context: &f.context,
        client: &f.relay,
        actor: "a".repeat(64),
        provider_instance: "acme-primary".into(),
        model: "gpt-6.1-sol[low]".into(),
        policy: Default::default(),
    };
    let mut payload = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
    if let CodingSessionLifecycleAction::SessionHire { model, .. } = &mut payload.action {
        *model = Some("gpt-6.1-sol".into());
    }
    let request = beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &payload)
        .unwrap()
        .sign_with_keys(&f.relay.keys)
        .unwrap();
    let refusal = local.plan(&f.scope, &request, &[]).await.err().unwrap();
    assert!(
        matches!(&refusal, Error::Refused(reason) if reason.starts_with("HIRE_MODEL_NOT_OFFERED"))
    );
    f.request = request; // Journal the actual bare-model request, not the fixture default.
    *f.effects.decision.lock().unwrap() = match refusal {
        Error::Refused(reason) => Some(reason),
        _ => unreachable!(),
    };
    run(&f).await.unwrap();
    *f.effects.decision.lock().unwrap() = None;
    assert!(
        matches!(run(&f).await.unwrap(), Disposition::PolicyRefused {code, ..} if code == "HIRE_MODEL_NOT_OFFERED")
    );
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn production_refusal_chat_route_rejects_unverified_legacy_origins() {
    let mut f = fixture();
    enable_catalog_probe(&mut f);
    let (context, relay) = f.effects.catalog_probe.as_ref().unwrap();
    let actor = f.request.pubkey.to_hex();
    let source = "a".repeat(64);
    let root = "b".repeat(64);
    let configured = beekeeper_session_provider::conversation_project::ProjectContext {
        project: f.scope.project.clone(),
        channel: f.scope.channel,
        actor: actor.clone(),
        genesis: f.scope.genesis.clone(),
        hire_selection: None,
    };
    let state = context.home.join("state");
    std::fs::create_dir_all(state.join("conversation-projects")).unwrap();
    crate::management::local::save(
        &state
            .join("conversation-projects")
            .join(format!("{actor}.json")),
        &configured,
    )
    .unwrap();
    let origin = json!({"actor":actor,"channel":f.scope.channel,"root":root,"event":source,"target":{"driver":"acme","instanceId":"fixture","sessionId":"primary","generation":1}});
    std::fs::write(
        state.join("conversation-origins.json"),
        json!({"requests":{source.clone():origin},"bindings":{}}).to_string(),
    )
    .unwrap();
    let mut payload = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
    if let CodingSessionLifecycleAction::SessionHire { reply_to, .. } = &mut payload.action {
        *reply_to = Some(source.clone());
    }
    let request = beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &payload)
        .unwrap()
        .sign_with_keys(&f.relay.keys)
        .unwrap();
    let local = crate::hire_host::LocalHost {
        context,
        client: relay,
        actor: "c".repeat(64),
        provider_instance: "acme-primary".into(),
        model: "acme-test".into(),
        policy: Default::default(),
    };
    assert!(local
        .refusal_conversation(&f.scope, &request)
        .await
        .is_err());
    // No create in the delegation umbrella is fabricated or needed for chat.
    assert!(local.refusal_target(&f.scope, &request).await.is_err());
    let mut wrong = origin.clone();
    wrong["actor"] = json!("d".repeat(64));
    std::fs::write(
        state.join("conversation-origins.json"),
        json!({"requests":{source:wrong},"bindings":{}}).to_string(),
    )
    .unwrap();
    assert!(local
        .refusal_conversation(&f.scope, &request)
        .await
        .is_err());
}
