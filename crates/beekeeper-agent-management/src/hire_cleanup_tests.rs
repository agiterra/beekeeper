use super::*;

async fn bounded(f: &Fixture) -> Result<Disposition, Error> {
    answer_bounded(
        &f.context, &f.relay, &f.effects, &f.scope, &f.request, 1, 30,
    )
    .await
}
fn expire(f: &Fixture) {
    let relay =
        beekeeper_host_core::record::canonical_relay_key(&crate::to_ws_url(f.relay.relay_url()));
    let command = decode_coding_session_lifecycle_command(&f.request.content)
        .unwrap()
        .command_id;
    let path = journal_path(
        &f.context,
        &relay,
        &f.relay.keys.public_key().to_hex(),
        &f.scope,
        &command,
    )
    .unwrap();
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["hire"]["cleanup"]["deadline"] = json!(1);
    std::fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
}
fn stops(f: &Fixture) -> Vec<Event> {
    f.relay
        .writes
        .lock()
        .unwrap()
        .iter()
        .filter(|e| {
            decode_coding_session_lifecycle_command(&e.content)
                .is_ok_and(|p| matches!(p.action, CodingSessionLifecycleAction::SessionStop { .. }))
        })
        .cloned()
        .collect()
}
fn confirm_stop(f: &Fixture, event: &Event) {
    let p = decode_coding_session_lifecycle_command(&event.content).unwrap();
    let CodingSessionLifecycleAction::SessionStop { session, .. } = p.action else {
        panic!("stop")
    };
    let receipt: LifecycleReceipt = serde_json::from_value(json!({
        "schema":LIFECYCLE_RECEIPT_SCHEMA,"commandId":p.command_id,"status":"stopped","session":session
    })).unwrap();
    f.relay.rows.lock().unwrap().push(
        beekeeper_sdk::build_coding_session_lifecycle_receipt(
            f.scope.channel,
            &receipt.command_id,
            &serde_json::to_string(&receipt).unwrap(),
        )
        .unwrap()
        .sign_with_keys(&f.provider)
        .unwrap(),
    );
}
#[tokio::test]
async fn created_without_grants_expires_and_exact_stop_fences_restart() {
    let f = fixture();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    expire(&f);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending {
            target: Some(_),
            ..
        }
    ));
    let stop = stops(&f)[0].clone();
    assert_eq!(stop.pubkey, f.relay.keys.public_key());
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending { .. }
    ));
    assert!(stops(&f).iter().all(|e| e == &stop));
    confirm_stop(&f, &stop);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::Stopped { target: Some(_) }
    ));
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::Stopped { .. }
    ));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
}
#[tokio::test]
async fn late_creation_after_expiry_is_observed_not_republished() {
    let f = fixture();
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::PublicationUnconfirmed { .. }
    ));
    expire(&f);
    let count = f.relay.writes.lock().unwrap().len();
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending { target: None, .. }
    ));
    assert_eq!(f.relay.writes.lock().unwrap().len(), count);
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending {
            target: Some(_),
            ..
        }
    ));
    assert_eq!(stops(&f).len(), 1);
}
#[tokio::test]
async fn revoked_lead_cannot_disable_owner_cleanup_or_launch_new_work() {
    let f = fixture();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    f.effects.unauthorized.store(true, Ordering::SeqCst);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending {
            target: Some(_),
            ..
        }
    ));
    confirm_stop(&f, &stops(&f)[0]);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::Stopped { .. }
    ));
    let fresh = fixture();
    fresh.effects.unauthorized.store(true, Ordering::SeqCst);
    assert!(matches!(bounded(&fresh).await, Err(Error::Auth(_))));
    assert!(fresh.relay.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn permanent_stop_refusal_is_persisted_without_republish_or_release() {
    let f = fixture();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    f.effects.grants_ready.store(true, Ordering::SeqCst);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::Ready { .. }
    ));
    expire(&f);
    f.relay.refuse_stop.store(true, Ordering::SeqCst);
    assert!(
        matches!(bounded(&f).await.unwrap(), Disposition::CleanupPending { reason, .. } if reason.contains("permanently refused"))
    );
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::CleanupPending { .. }
    ));
    assert_eq!(stops(&f).len(), 1);
    assert_eq!(
        pending_requests(&f.context, &f.relay, &f.scope)
            .unwrap()
            .len(),
        1
    );
    let relay =
        beekeeper_host_core::record::canonical_relay_key(&crate::to_ws_url(f.relay.relay_url()));
    let command = decode_coding_session_lifecycle_command(&f.request.content)
        .unwrap()
        .command_id;
    let path = journal_path(
        &f.context,
        &relay,
        &f.relay.keys.public_key().to_hex(),
        &f.scope,
        &command,
    )
    .unwrap();
    let value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(pending_plans(&path, &value["request"]).unwrap().len(), 1);
}

#[tokio::test]
async fn expired_before_publication_never_stages_or_creates_again() {
    let f = fixture();
    f.effects.stage_crash.store(true, Ordering::SeqCst);
    assert!(bounded(&f).await.is_err());
    expire(&f);
    assert!(matches!(
        bounded(&f).await.unwrap(),
        Disposition::Stopped { target: None }
    ));
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn context_refresh_failure_fences_fresh_work_but_reconciles_exact_owner_stop() {
    for failure in ["malformed", "conflicting", "atomic_write"] {
        let mut f = fixture();
        f.relay.rows.lock().unwrap().push(f.receipt.clone());
        assert!(matches!(
            bounded(&f).await.unwrap(),
            Disposition::CreatedAwaitingGrants { .. }
        ));
        super::catalog_tests::enable_catalog_probe(&mut f);
        let actor = f.request.pubkey.to_hex();
        let dir = f.context.home.join("state/conversation-projects");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{actor}.json"));
        let mut config = beekeeper_session_provider::conversation_project::ProjectContext {
            project: f.scope.project.clone(),
            channel: f.scope.channel,
            actor: actor.clone(),
            genesis: f.scope.genesis.clone(),
            hire_selection: None,
        };
        if failure == "conflicting" {
            config.project = "30621:other:project".into();
        }
        if failure == "malformed" {
            crate::management::local::save(&path, &json!({"malformed":true})).unwrap();
        } else {
            crate::management::local::save(&path, &config).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let result = crate::conversation_project::configure_with_writer(
            &f.context,
            &f.relay,
            &f.scope,
            &actor,
            None,
            |_, _| Err(Error::Other("injected atomic context write failure".into())),
        )
        .await;
        let reason = result.unwrap_err().to_string();
        if failure == "atomic_write" {
            assert!(reason.contains("atomic context write"));
        }
        assert_eq!(std::fs::read(&path).unwrap(), original);
        let recover = || {
            answer_bounded_with_readiness(
                &f.context,
                &f.relay,
                &f.effects,
                &f.scope,
                &f.request,
                1,
                30,
                Some(&reason),
            )
        };
        assert!(matches!(
            recover().await.unwrap(),
            Disposition::CleanupPending {
                target: Some(_),
                ..
            }
        ));
        let stop = stops(&f)[0].clone();
        assert_eq!(stop.pubkey, f.relay.keys.public_key());
        assert!(recover().await.is_ok());
        assert!(stops(&f).iter().all(|event| event == &stop));
        confirm_stop(&f, &stop);
        assert!(matches!(
            recover().await.unwrap(),
            Disposition::Stopped { target: Some(_) }
        ));
        let mut payload = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
        payload.command_id = Uuid::new_v4().to_string();
        let fresh =
            beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &payload)
                .unwrap()
                .sign_with_keys(&f.relay.keys)
                .unwrap();
        assert!(
            matches!(answer_bounded_with_readiness(&f.context,&f.relay,&f.effects,&f.scope,&fresh,1,30,Some(&reason)).await,Err(Error::Unconfirmed(message)) if message.contains("HIRE_CONTEXT_NOT_READY"))
        );
        assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}
