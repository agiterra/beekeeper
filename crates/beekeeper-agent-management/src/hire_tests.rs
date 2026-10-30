//! Failure injection through the real journal/publication transaction.
use super::*;
use beekeeper_core::coding_session_genesis::CodingSessionGenesisPayload;
use beekeeper_core::coding_session_lifecycle_command::CodingSessionLifecycleCommandPayload;
use beekeeper_core::coding_session_payload::{
    LifecycleReceipt, ReceiptStatus, LIFECYCLE_RECEIPT_SCHEMA,
};
use nostr::Keys;
use serde_json::Value;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

struct Relay {
    keys: Keys,
    rows: Mutex<Vec<Event>>,
    writes: Mutex<Vec<Event>>,
    ack_lost: AtomicBool,
    unavailable: AtomicBool,
    delegated: bool,
}
impl RelayTransport for Relay {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        "ws://127.0.0.1:31987"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(self.delegated.then(|| self.keys.public_key().to_hex()))
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        panic!("hire never lends owner signer")
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        if self.unavailable.load(Ordering::SeqCst) {
            return Err(Error::Other("offline".into()));
        }
        Ok(self
            .rows
            .lock()
            .unwrap()
            .iter()
            .filter(|e| {
                filter["kinds"]
                    .as_array()
                    .unwrap()
                    .contains(&json!(e.kind.as_u16()))
                    && filter
                        .get("ids")
                        .is_none_or(|ids| ids.as_array().unwrap().contains(&json!(e.id.to_hex())))
            })
            .map(|e| serde_json::to_value(e).unwrap())
            .collect())
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        self.writes.lock().unwrap().push(event.clone());
        self.rows.lock().unwrap().push(event);
        if self.ack_lost.load(Ordering::SeqCst) {
            Err(Error::Other("ACK lost".into()))
        } else {
            Ok(json!({"accepted":true}))
        }
    }
}
struct Effects {
    catalog_probe: Option<(Context, Relay)>,
    decision: Mutex<Option<String>>,
    disclose: AtomicBool,
    planning_unavailable: AtomicBool,
    plan: SeatPlan,
    plans: AtomicUsize,
    stages: AtomicUsize,
    stage_crash: AtomicBool,
    stale: AtomicBool,
    unauthorized: AtomicBool,
    grants_ready: AtomicBool,
    grant_events: Mutex<Vec<Event>>,
}
impl Adapter for Effects {
    async fn authorize(&self, _: &Scope, _: &Event) -> Result<(), Error> {
        if self.unauthorized.load(Ordering::SeqCst) {
            Err(Error::Auth("unauthorized".into()))
        } else {
            Ok(())
        }
    }
    async fn plan(&self, scope: &Scope, _: &Event, _: &[SeatPlan]) -> Result<SeatPlan, Error> {
        self.plans.fetch_add(1, Ordering::SeqCst);
        if let Some((context, relay)) = &self.catalog_probe {
            crate::hire_host::LocalHost {
                context,
                client: relay,
                actor: String::new(),
                provider_instance: "acme-primary".into(),
                model: "acme-test".into(),
                policy: crate::hire_host::Policy::default(),
            }
            .runtime(scope)
            .await?;
        }
        if self.planning_unavailable.load(Ordering::SeqCst) {
            return Err(Error::Unconfirmed("catalog read unavailable".into()));
        }
        if let Some(reason) = self.decision.lock().unwrap().clone() {
            return Err(Error::Refused(reason));
        }
        Ok(self.plan.clone())
    }
    async fn refusal_target(&self, _: &Scope, _: &Event) -> Result<CodingSessionTarget, Error> {
        if !self.disclose.load(Ordering::SeqCst) {
            return Err(Error::Unconfirmed("target unavailable".into()));
        }
        Ok(CodingSessionTarget {
            driver: "acme-acp".into(),
            instance_id: "fixture".into(),
            session_id: "requester".into(),
            generation: 1,
        })
    }
    async fn revalidate(&self, _: &Scope, _: &SeatPlan) -> Result<(), Error> {
        if self.stale.load(Ordering::SeqCst) {
            Err(Error::Refused("stale canonical selection".into()))
        } else {
            Ok(())
        }
    }
    async fn stage(&self, _: &Scope, plan: &SeatPlan) -> Result<(), Error> {
        self.stages.fetch_add(1, Ordering::SeqCst);
        // The first effect must find its reservation already on disk.
        std::fs::create_dir_all(&plan.worktree).unwrap();
        std::fs::write(plan.worktree.join("staged"), plan.create.id.to_hex()).unwrap();
        if self.stage_crash.swap(false, Ordering::SeqCst) {
            Err(Error::Other("crash after staging".into()))
        } else {
            Ok(())
        }
    }
    async fn grants(
        &self,
        _: &Scope,
        _: &SeatPlan,
        _: &CodingSessionTarget,
    ) -> Result<Vec<Event>, Error> {
        let events = self.grant_events.lock().unwrap().clone();
        if events.is_empty() {
            Err(Error::Other("authority unavailable".into()))
        } else {
            Ok(events)
        }
    }
    async fn grants_ready(
        &self,
        _: &Scope,
        _: &SeatPlan,
        _: &CodingSessionTarget,
    ) -> Result<bool, Error> {
        Ok(self.grants_ready.load(Ordering::SeqCst))
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    context: Context,
    relay: Relay,
    effects: Effects,
    scope: Scope,
    request: Event,
    receipt: Event,
    provider: Keys,
}
fn signed(keys: &Keys, channel: Uuid, id: Uuid, action: Value) -> Event {
    let payload: CodingSessionLifecycleCommandPayload = serde_json::from_value(json!({
        "schema":"buzz-coding-session-lifecycle-command/v1", "commandId":id.to_string(), "action":action
    })).unwrap();
    beekeeper_sdk::build_coding_session_lifecycle_command(channel, &payload)
        .unwrap()
        .sign_with_keys(keys)
        .unwrap()
}
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let owner = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let channel = Uuid::new_v4();
    let session = Uuid::new_v4().to_string();
    let genesis = beekeeper_sdk::build_coding_session_genesis(
        channel,
        &CodingSessionGenesisPayload::new(&session),
    )
    .unwrap()
    .sign_with_keys(&owner)
    .unwrap();
    let scope = Scope {
        project: format!("30621:{}:fixture", owner.public_key().to_hex()),
        channel,
        genesis,
    };
    let request = signed(
        &owner,
        channel,
        Uuid::new_v4(),
        json!({
            "type":"session.hire", "sessionRef":session, "genesisRef":scope.genesis.id.to_hex(),
            "role":"builder", "brief":"Read the marker.", "providerInstanceRef":"acme-primary", "model":"acme-test"
        }),
    );
    let command = Uuid::new_v4();
    let create = signed(
        &owner,
        channel,
        command,
        json!({
            "type":"session.create", "projectRef":scope.project, "repoRef":null,
            "sessionRef":session,"genesisRef":scope.genesis.id.to_hex(),
            "providerInstanceRef":"acme-primary","providerAuthorityPubkey":provider.public_key().to_hex(),
            "model":"acme-test","title":"Fixture","initialTurn":"[From the lead] Read the marker.",
            "actor":actor.public_key().to_hex(),"role":"builder","hireRef":request.id.to_hex()
        }),
    );
    let target = CodingSessionTarget {
        driver: "acme-acp".into(),
        instance_id: "provider-instance".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let body = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.into(),
        command_id: command.to_string(),
        status: ReceiptStatus::Created,
        session: Some(target),
        error: None,
        turn_id: None,
        rewind: None,
        quiescence: None,
        checkpoint_skipped: None,
    };
    let receipt = beekeeper_sdk::build_coding_session_lifecycle_receipt(
        channel,
        &command.to_string(),
        &serde_json::to_string(&body).unwrap(),
    )
    .unwrap()
    .sign_with_keys(&provider)
    .unwrap();
    let plan = SeatPlan {
        create,
        definition_revision: "a".repeat(64),
        private_snapshot_hash: "b".repeat(64),
        source_event_id: "c".repeat(64),
        pack_ref: PackRef {
            repo: format!("30617:{}:roles", owner.public_key().to_hex()),
            sha: "d".repeat(40),
            role: "builder".into(),
            path: "roles/builder".into(),
        },
        checkout: dir.path().join("code"),
        checkout_revision: "d".repeat(40),
        worktree: dir.path().join("seat"),
        pack_dir: dir.path().join("packs/immutable"),
        agents_checkout: Some(dir.path().join("seat-agents")),
    };
    Fixture {
        context: Context {
            home: dir.path().join("custody"),
            instance: beekeeper_host_core::layout::Instance::Dev,
            native_home: None,
        },
        _dir: dir,
        relay: Relay {
            keys: owner,
            rows: Mutex::new(vec![]),
            writes: Mutex::new(vec![]),
            ack_lost: AtomicBool::new(false),
            unavailable: AtomicBool::new(false),
            delegated: false,
        },
        effects: Effects {
            catalog_probe: None,
            decision: Mutex::new(None),
            disclose: AtomicBool::new(false),
            planning_unavailable: AtomicBool::new(false),
            plan,
            plans: AtomicUsize::new(0),
            stages: AtomicUsize::new(0),
            stage_crash: AtomicBool::new(false),
            stale: AtomicBool::new(false),
            unauthorized: AtomicBool::new(false),
            grants_ready: AtomicBool::new(false),
            grant_events: Mutex::new(Vec::new()),
        },
        scope,
        request,
        receipt,
        provider,
    }
}
async fn run(f: &Fixture) -> Result<Disposition, Error> {
    answer(&f.context, &f.relay, &f.effects, &f.scope, &f.request, 1).await
}

#[tokio::test]
async fn restart_after_staged_custody_keeps_create_actor_and_tree() {
    let f = fixture();
    f.effects.stage_crash.store(true, Ordering::SeqCst);
    assert!(run(&f).await.is_err());
    assert!(f.relay.writes.lock().unwrap().is_empty());
    let staged = std::fs::read(f.effects.plan.worktree.join("staged")).unwrap();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(f.effects.plan.worktree.join("staged")).unwrap(),
        staged
    );
    assert_eq!(
        f.relay.writes.lock().unwrap().as_slice(),
        std::slice::from_ref(&f.effects.plan.create)
    );
}

#[tokio::test]
async fn lost_ack_receipt_timeout_and_grant_failure_never_rehire() {
    let f = fixture();
    f.relay.ack_lost.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PublicationUnconfirmed { .. }
    ));
    assert!(f.effects.plan.worktree.join("staged").is_file());
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    f.effects.stale.store(true, Ordering::SeqCst); // Repair does not select a new execution.
    f.effects.grants_ready.store(true, Ordering::SeqCst);
    assert!(matches!(run(&f).await.unwrap(), Disposition::Ready { .. }));
    assert!(matches!(run(&f).await.unwrap(), Disposition::Ready { .. }));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.relay.writes.lock().unwrap().as_slice(),
        std::slice::from_ref(&f.effects.plan.create)
    );
}

#[tokio::test]
async fn changed_signed_request_reusing_command_id_preserves_original() {
    let mut f = fixture();
    f.effects.stage_crash.store(true, Ordering::SeqCst);
    assert!(run(&f).await.is_err());
    let mut payload = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
    if let CodingSessionLifecycleAction::SessionHire { brief, .. } = &mut payload.action {
        *brief = "different request".into();
    }
    f.request = beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &payload)
        .unwrap()
        .sign_with_keys(f.relay.keys())
        .unwrap();
    assert!(matches!(run(&f).await, Err(Error::Conflict(_))));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unauthorized_or_stale_selection_has_no_publication() {
    let mut f = fixture();
    f.relay.delegated = true;
    assert!(matches!(run(&f).await, Err(Error::Auth(_))));
    f.relay.delegated = false;
    f.effects.unauthorized.store(true, Ordering::SeqCst);
    assert!(matches!(run(&f).await, Err(Error::Auth(_))));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 0);
    f.effects.unauthorized.store(false, Ordering::SeqCst);
    f.effects.stale.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused {
            disclosure_confirmed: false,
            ..
        }
    ));
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[test]
fn same_umbrella_excludes_duplicate_selection_not_other_umbrellas() {
    let f = fixture();
    let _one = scope_lock(&f.context, "one").unwrap();
    assert!(scope_lock(&f.context, "one").is_err());
    assert!(scope_lock(&f.context, "two").is_ok());
}

#[tokio::test]
async fn journal_write_failure_precedes_all_effects() {
    let f = fixture();
    let command = decode_coding_session_lifecycle_command(&f.request.content)
        .unwrap()
        .command_id;
    let path = journal_path(
        &f.context,
        f.relay.relay_url(),
        &f.relay.keys.public_key().to_hex(),
        &f.scope,
        &command,
    )
    .unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(run(&f).await.is_err());
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn accepted_create_reconciles_even_when_old_selection_is_now_stale() {
    let f = fixture();
    f.relay.ack_lost.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PublicationUnconfirmed { .. }
    ));
    f.effects.stale.store(true, Ordering::SeqCst);
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn grants_are_saved_before_send_and_retry_the_same_signed_bytes() {
    use beekeeper_core::coding_session_authority_transition::CodingSessionAuthorityTransitionPayload;
    let f = fixture();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    let actor = validate_plan_actor(&f.effects.plan).unwrap();
    let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
        f.scope.genesis.id.to_hex(),
        None,
        1,
        actor,
    );
    let grant = beekeeper_sdk::build_coding_session_authority_transition(f.scope.channel, &payload)
        .unwrap()
        .sign_with_keys(f.relay.keys())
        .unwrap();
    f.effects.grant_events.lock().unwrap().push(grant.clone());
    f.relay.ack_lost.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    f.effects.grant_events.lock().unwrap().clear(); // Restart cannot regenerate grants.
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
    let writes = f.relay.writes.lock().unwrap();
    assert_eq!(writes.iter().filter(|e| e.id == grant.id).count(), 1);
    assert_eq!(writes[1], grant);
}

#[tokio::test]
async fn wrong_project_plan_is_rejected_before_reservation_or_staging() {
    let mut f = fixture();
    f.scope.project = format!("30621:{}:other", f.relay.keys().public_key().to_hex());
    assert!(matches!(run(&f).await, Err(Error::Conflict(_))));
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[tokio::test]
async fn another_request_cannot_reserve_an_unconfirmed_actor() {
    let mut f = fixture();
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PublicationUnconfirmed { .. }
    ));
    let mut request = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
    request.command_id = Uuid::new_v4().to_string();
    f.request = beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &request)
        .unwrap()
        .sign_with_keys(f.relay.keys())
        .unwrap();
    let mut create =
        decode_coding_session_lifecycle_command(&f.effects.plan.create.content).unwrap();
    create.command_id = Uuid::new_v4().to_string();
    if let CodingSessionLifecycleAction::SessionCreate { hire_ref, .. } = &mut create.action {
        *hire_ref = Some(f.request.id.to_hex());
    }
    f.effects.plan.create =
        beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &create)
            .unwrap()
            .sign_with_keys(f.relay.keys())
            .unwrap();
    assert!(
        matches!(run(&f).await.unwrap(), Disposition::PolicyRefused { code, .. } if code == "HIRE_ROLE_BUSY")
    );
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 1);
    assert_eq!(f.relay.writes.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn other_management_journal_schemas_do_not_block_hire_selection() {
    let f = fixture();
    let path = operation_path(&f.context, Uuid::new_v4()).unwrap();
    save(
        &path,
        &json!({"request":{"action":"stop"}, "commands":[], "receipts":[]}),
    )
    .unwrap();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
}

#[tokio::test]
async fn arbitrary_protocol_command_ids_keep_wire_identity_and_scope_the_journal() {
    let mut f = fixture();
    let mut payload = decode_coding_session_lifecycle_command(&f.request.content).unwrap();
    payload.command_id = "legacy-hire-command-2026".into();
    f.request = beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &payload)
        .unwrap()
        .sign_with_keys(f.relay.keys())
        .unwrap();
    let mut create =
        decode_coding_session_lifecycle_command(&f.effects.plan.create.content).unwrap();
    if let CodingSessionLifecycleAction::SessionCreate { hire_ref, .. } = &mut create.action {
        *hire_ref = Some(f.request.id.to_hex());
    }
    f.effects.plan.create =
        beekeeper_sdk::build_coding_session_lifecycle_command(f.scope.channel, &create)
            .unwrap()
            .sign_with_keys(f.relay.keys())
            .unwrap();
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    f.effects.grants_ready.store(true, Ordering::SeqCst);
    assert!(matches!(run(&f).await.unwrap(), Disposition::Ready { .. }));
    assert!(matches!(run(&f).await.unwrap(), Disposition::Ready { .. }));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    let owner = f.relay.keys.public_key().to_hex();
    let path = journal_path(
        &f.context,
        f.relay.relay_url(),
        &owner,
        &f.scope,
        &payload.command_id,
    )
    .unwrap();
    assert!(path.exists());
    assert_ne!(
        path,
        journal_path(
            &f.context,
            "ws://other",
            &owner,
            &f.scope,
            &payload.command_id
        )
        .unwrap()
    );
    assert_eq!(
        decode_coding_session_lifecycle_command(&f.request.content)
            .unwrap()
            .command_id,
        "legacy-hire-command-2026"
    );
}

#[tokio::test]
async fn signed_terminal_refusal_releases_selection_but_never_replays_old_create() {
    let f = fixture();
    let create = decode_coding_session_lifecycle_command(&f.effects.plan.create.content).unwrap();
    let refusal = beekeeper_sdk::build_coding_session_lifecycle_receipt(
        f.scope.channel,
        &create.command_id,
        &serde_json::to_string(&LifecycleReceipt::failed(
            &create.command_id,
            "MODEL_NOT_OFFERED",
            "model unavailable",
        ))
        .unwrap(),
    )
    .unwrap()
    .sign_with_keys(&f.provider)
    .unwrap();
    f.relay.rows.lock().unwrap().push(refusal);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::Refused { .. }
    ));
    let count = f.relay.writes.lock().unwrap().len();
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::Refused { .. }
    ));
    assert_eq!(f.relay.writes.lock().unwrap().len(), count);
    let owner = f.relay.keys.public_key().to_hex();
    let command = decode_coding_session_lifecycle_command(&f.request.content)
        .unwrap()
        .command_id;
    let path = journal_path(&f.context, f.relay.relay_url(), &owner, &f.scope, &command).unwrap();
    let binding = json!({"type":"hire","owner":owner,"relay":f.relay.relay_url(),"scope":f.scope});
    assert!(pending_plans(&path, &binding).unwrap().is_empty());
}

#[tokio::test]
async fn startup_uncertainty_does_not_release_reservation_or_claim_refusal() {
    let f = fixture();
    let create = decode_coding_session_lifecycle_command(&f.effects.plan.create.content).unwrap();
    let refusal = beekeeper_sdk::build_coding_session_lifecycle_receipt(
        f.scope.channel,
        &create.command_id,
        &serde_json::to_string(&LifecycleReceipt::failed(
            &create.command_id,
            beekeeper_core::coding_session_payload::CREATE_START_OUTCOME_UNCONFIRMED,
            "startup uncertain",
        ))
        .unwrap(),
    )
    .unwrap()
    .sign_with_keys(&f.provider)
    .unwrap();
    f.relay.rows.lock().unwrap().push(refusal);
    assert!(matches!(run(&f).await, Err(Error::Unconfirmed(_))));
    assert_eq!(
        pending_requests(&f.context, &f.relay, &f.scope)
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn terminal_decision_survives_ack_loss_and_enabled_restart_without_launch() {
    let f = fixture();
    *f.effects.decision.lock().unwrap() = Some("HIRE_OFF".into());
    f.effects.disclose.store(true, Ordering::SeqCst);
    f.relay.ack_lost.store(true, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused {
            disclosure_confirmed: false,
            ..
        }
    ));
    let first = f.relay.writes.lock().unwrap()[0].clone();
    assert_eq!(first.kind.as_u16(), 44220);
    assert_eq!(first.pubkey, f.relay.keys.public_key());
    assert!(first.verify().is_ok());
    assert!(first.content.contains("hire refused: HIRE_OFF"));
    assert_eq!(
        pending_requests(&f.context, &f.relay, &f.scope)
            .unwrap()
            .len(),
        1
    );
    f.relay.rows.lock().unwrap().clear(); // ACK and read visibility lost: resend exact bytes.
    *f.effects.decision.lock().unwrap() = None; // policy enabled / process restart
    f.relay.ack_lost.store(false, Ordering::SeqCst);
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused {
            disclosure_confirmed: true,
            ..
        }
    ));
    assert_eq!(
        f.relay.writes.lock().unwrap().as_slice(),
        &[first.clone(), first]
    );
    assert!(pending_requests(&f.context, &f.relay, &f.scope)
        .unwrap()
        .is_empty());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::PolicyRefused { .. }
    ));
    assert_eq!(f.effects.plans.load(Ordering::SeqCst), 1);
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn unknown_selection_is_retryable_and_unauthorized_request_gets_no_disclosure() {
    let f = fixture();
    f.effects.disclose.store(true, Ordering::SeqCst);
    *f.effects.decision.lock().unwrap() = Some("HIRE_OFF".into());
    f.effects.unauthorized.store(true, Ordering::SeqCst);
    assert!(matches!(run(&f).await, Err(Error::Auth(_))));
    assert!(f.relay.writes.lock().unwrap().is_empty());
    f.effects.unauthorized.store(false, Ordering::SeqCst);
    f.effects.planning_unavailable.store(true, Ordering::SeqCst);
    assert!(matches!(run(&f).await, Err(Error::Unconfirmed(_))));
    assert!(f.relay.writes.lock().unwrap().is_empty());
    f.effects
        .planning_unavailable
        .store(false, Ordering::SeqCst);
    *f.effects.decision.lock().unwrap() = None;
    f.relay.rows.lock().unwrap().push(f.receipt.clone());
    assert!(matches!(
        run(&f).await.unwrap(),
        Disposition::CreatedAwaitingGrants { .. }
    ));
}

#[tokio::test]
async fn possibly_published_incomplete_preparation_is_never_restarted() {
    let f = fixture();
    f.effects.stage_crash.store(true, Ordering::SeqCst);
    assert!(run(&f).await.is_err());
    let command = decode_coding_session_lifecycle_command(&f.request.content)
        .unwrap()
        .command_id;
    let path = journal_path(
        &f.context,
        f.relay.relay_url(),
        &f.relay.keys.public_key().to_hex(),
        &f.scope,
        &command,
    )
    .unwrap();
    let mut value: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    value["hire"]["publicationAttempted"] = json!(true);
    save(&path, &value).unwrap();
    let stages = f.effects.stages.load(Ordering::SeqCst);
    assert!(matches!(run(&f).await, Err(Error::Unconfirmed(_))));
    assert_eq!(f.effects.stages.load(Ordering::SeqCst), stages);
    assert!(f.relay.writes.lock().unwrap().is_empty());
}

#[path = "hire_catalog_tests.rs"]
mod catalog_tests;
