use super::*;
use beekeeper_core::coding_session_catalog::{Catalog, CatalogProvider, CATALOG_SCHEMA};
use beekeeper_core::coding_session_payload::Capabilities;
use beekeeper_host_core::{config::HostConfig, provision};

fn enable_catalog_probe(f: &mut Fixture) -> Keys {
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
