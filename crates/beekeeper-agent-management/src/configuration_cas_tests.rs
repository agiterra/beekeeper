use super::*;
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex,
};

struct Relay {
    keys: Keys,
    url: String,
    events: Mutex<Vec<Event>>,
    writes: AtomicUsize,
    queries: AtomicUsize,
    lose_publication_reply: AtomicBool,
}
impl RelayTransport for Relay {
    fn keys(&self) -> &Keys {
        &self.keys
    }
    fn relay_url(&self) -> &str {
        &self.url
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, CliError> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, CliError> {
        Err(fail("unused scope"))
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, CliError> {
        self.queries.fetch_add(1, Ordering::SeqCst);
        let rows = self.events.lock().expect("events");
        Ok(rows
            .iter()
            .filter(|event| {
                filter["kinds"]
                    .as_array()
                    .is_some_and(|kinds| kinds.contains(&json!(event.kind.as_u16())))
                    && filter.get("ids").is_none_or(|ids| {
                        ids.as_array()
                            .is_some_and(|ids| ids.contains(&json!(event.id.to_hex())))
                    })
                    && filter.get("authors").is_none_or(|authors| {
                        authors
                            .as_array()
                            .is_some_and(|authors| authors.contains(&json!(event.pubkey.to_hex())))
                    })
            })
            .map(|event| serde_json::to_value(event).expect("event"))
            .collect())
    }
    async fn submit_event(&self, event: Event) -> Result<Value, CliError> {
        assert!(event.verify().is_ok());
        assert_eq!(
            event.kind.as_u16(),
            30177,
            "refresh never sends lifecycle commands"
        );
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.events.lock().expect("events").push(event.clone());
        if self.lose_publication_reply.swap(false, Ordering::SeqCst) {
            return Err(CliError::Other("fixture acceptance reply dropped".into()));
        }
        Ok(json!({"event_id":event.id.to_hex(),"accepted":true}))
    }
}
fn fixture() -> (
    tempfile::TempDir,
    Context,
    Relay,
    String,
    Event,
    crate::PrivateConfiguration,
) {
    let home = tempfile::tempdir().expect("home");
    let context = Context {
        home: home.path().into(),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    let keys = Keys::generate();
    let url = "ws://127.0.0.1:31234".to_string();
    let provider = provision::mint_provider_record(&keys, &url).expect("provider");
    let actor = provision::mint_provider_record(&keys, &url).expect("actor");
    let state = home.path().join("provider-state");
    let config = HostConfig {
        version: 1,
        instance: context.instance,
        relay_url: url.clone(),
        provider_pubkey: provider.provider_pubkey.clone(),
        session_provider_base_dir: state.clone(),
        provider_state_dir: state.join(&provider.provider_pubkey),
        runtimes: Vec::new(),
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        provider_command: None,
        written_at: provider.created_at.clone(),
    };
    provision::write_commissioning(&context.home, &config, &provider).expect("commission");
    actor_custody(
        &config.provider_state_dir,
        &actor.provider_pubkey,
        Some((
            &actor.private_key_nsec,
            actor.auth_tag.as_deref().expect("actor attestation"),
        )),
    )
    .expect("actor custody");
    let public:ManagedAgentEventContent=serde_json::from_value(json!({"name":"Accepted", "parallelism":1, "system_prompt":"preserved instructions","respond_to":"owner-only"})).expect("public");
    let event =
        beekeeper_sdk::managed_agent::build_managed_agent_event(&actor.provider_pubkey, &public)
            .expect("head")
            .sign_with_keys(&keys)
            .expect("signed");
    let private = crate::PrivateConfiguration {
        record: json!({"pubkey":actor.provider_pubkey,"effective_command":"bash","effective_args":[],"effective_env":{"PRIVATE_OPTION":"preserved"}}),
    };
    let dir = config
        .provider_state_dir
        .join("actors")
        .join(&actor.provider_pubkey);
    let enrollment = json!({"actorPubkey":actor.provider_pubkey,"channelId":Uuid::new_v4(),"cwd":home.path().join("work"),"desiredRunning":false,"definition":event});
    save(&dir.join("enrollment.json"), &enrollment).expect("enrollment");
    super::super::private_config::commit_update(&config, &enrollment, &event, &private)
        .expect("snapshot");
    let relay = Relay {
        keys,
        url,
        events: Mutex::new(vec![event.clone()]),
        writes: AtomicUsize::new(0),
        queries: AtomicUsize::new(0),
        lose_publication_reply: AtomicBool::new(false),
    };
    (home, context, relay, actor.provider_pubkey, event, private)
}
fn request(
    context: &Context,
    actor: &str,
    head: &Event,
    private: crate::PrivateConfiguration,
) -> UpdateRequest {
    UpdateRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: actor.into(),
        expected_revision: head.id.to_hex(),
        display_name: None,
        system_prompt: None,
        model: None,
        home: Some(context.home.clone()),
        public_definition: None,
        private_config: Some(private),
    }
}
#[tokio::test]
async fn intervening_shared_private_update_refuses_stale_snapshot_before_journal_or_publication() {
    let (_home, context, relay, actor, head, mut desired) = fixture();
    let before = private_configuration_snapshot(&context, &relay, &actor).expect("verified read");
    desired.record["effective_env"]["PRIVATE_OPTION"] = json!("newer client setting");
    let newer = request(&context, &actor, &head, desired.clone());
    update(&newer, &relay, &context)
        .await
        .expect("other client canonical update");
    let current = private_configuration_snapshot(&context, &relay, &actor).expect("new snapshot");
    assert_ne!(current.snapshot_hash, before.snapshot_hash);
    let guarded = request(&context, &actor, &current.definition, before.private);
    let writes = relay.writes.load(Ordering::SeqCst);
    let queries = relay.queries.load(Ordering::SeqCst);
    assert!(matches!(
        update_if_private_unchanged(&guarded, &relay, &context, &before.snapshot_hash).await,
        Err(CliError::Conflict(_))
    ));
    assert_eq!(relay.writes.load(Ordering::SeqCst), writes);
    assert_eq!(
        relay.queries.load(Ordering::SeqCst),
        queries,
        "hash refusal precedes relay activity"
    );
    assert!(!operation_path(&context, guarded.operation_id)
        .expect("path")
        .exists());
    assert_eq!(
        private_configuration_snapshot(&context, &relay, &actor)
            .expect("unchanged")
            .snapshot_hash,
        current.snapshot_hash
    );
}
#[tokio::test]
async fn publication_and_commit_lost_reply_retries_keep_original_hash_and_exact_operation() {
    let (_home, context, relay, actor, head, mut desired) = fixture();
    let before = private_configuration_snapshot(&context, &relay, &actor).expect("snapshot");
    desired.record["effective_env"]["PRIVATE_OPTION"] = json!("desired refresh");
    let args = request(&context, &actor, &head, desired);
    relay.lose_publication_reply.store(true, Ordering::SeqCst);
    assert!(
        update_if_private_unchanged(&args, &relay, &context, &before.snapshot_hash)
            .await
            .is_err()
    );
    assert_eq!(
        private_configuration_snapshot(&context, &relay, &actor)
            .expect("precommit")
            .snapshot_hash,
        before.snapshot_hash
    );
    let path = operation_path(&context, args.operation_id).expect("journal");
    let original = std::fs::read(&path).expect("immutable operation");
    let accepted = update_if_private_unchanged(&args, &relay, &context, &before.snapshot_hash)
        .await
        .expect("retry exact publication");
    assert_eq!(accepted["host_application"], "unconfirmed");
    let committed = private_configuration_snapshot(&context, &relay, &actor).expect("commit");
    assert_ne!(committed.snapshot_hash, before.snapshot_hash);
    update_if_private_unchanged(&args, &relay, &context, &before.snapshot_hash)
        .await
        .expect("lost completion reply retry");
    assert_eq!(
        relay.writes.load(Ordering::SeqCst),
        1,
        "same signed event is not republished"
    );
    assert_eq!(std::fs::read(&path).expect("same operation"), original);
    assert_eq!(
        private_configuration_snapshot(&context, &relay, &actor)
            .expect("same snapshot")
            .snapshot_hash,
        committed.snapshot_hash
    );
    assert!(matches!(
        update_if_private_unchanged(&args, &relay, &context, &committed.snapshot_hash).await,
        Err(CliError::Conflict(_))
    ));
    assert_eq!(std::fs::read(&path).expect("preserved"), original);
}
#[test]
fn snapshot_reader_rejects_foreign_signed_scope_and_shared_permissions() {
    let (_home, context, relay, actor, _head, _private) = fixture();
    let snapshot = private_configuration_snapshot(&context, &relay, &actor).expect("read");
    let (config, bytes) =
        super::super::private_config::snapshot_bytes(&context, &relay, &actor).expect("bytes");
    let path = config
        .provider_state_dir
        .join("actors")
        .join(&actor)
        .join("configuration.json");
    let mut value: Value = serde_json::from_slice(&bytes).expect("JSON");
    value["actorPubkey"] = json!(Keys::generate().public_key().to_hex());
    save(&path, &value).expect("foreign fixture");
    assert!(matches!(
        private_configuration_snapshot(&context, &relay, &actor),
        Err(CliError::Auth(_))
    ));
    atomic_write::atomic_write_json_restricted(&path, &bytes).expect("restore owned test fixture");
    assert_eq!(
        private_configuration_snapshot(&context, &relay, &actor)
            .expect("same")
            .snapshot_hash,
        snapshot.snapshot_hash
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("unsafe fixture");
        assert!(private_configuration_snapshot(&context, &relay, &actor).is_err());
    }
}

#[tokio::test]
async fn configuration_written_before_enrollment_crash_replays_exact_operation_to_finish_both() {
    let (_home, context, relay, actor, head, mut desired) = fixture();
    let before = private_configuration_snapshot(&context, &relay, &actor).expect("snapshot");
    desired.record["effective_env"]["PRIVATE_OPTION"] = json!("desired partial commit");
    let args = request(&context, &actor, &head, desired);
    let (config, _) =
        super::super::private_config::snapshot_bytes(&context, &relay, &actor).expect("scope");
    super::super::private_config::fail_next_enrollment_commit(
        &config.provider_state_dir.join("actors").join(&actor),
    );
    assert!(
        update_if_private_unchanged(&args, &relay, &context, &before.snapshot_hash)
            .await
            .is_err()
    );
    let partial = private_configuration_snapshot(&context, &relay, &actor)
        .expect("partial is observed, not completed");
    assert_ne!(partial.definition.id, partial.enrollment_definition.id);
    assert_eq!(partial.enrollment_definition.id, head.id);
    let path = operation_path(&context, args.operation_id).expect("operation");
    let original = std::fs::read(&path).expect("same journal");
    update_if_private_unchanged(&args, &relay, &context, &before.snapshot_hash)
        .await
        .expect("reconcile original operation");
    let settled = private_configuration_snapshot(&context, &relay, &actor).expect("canonical");
    assert_eq!(settled.definition.id, settled.enrollment_definition.id);
    assert_eq!(
        settled.snapshot_hash, partial.snapshot_hash,
        "config bytes not reminted during enrollment completion"
    );
    assert_eq!(std::fs::read(path).expect("journal preserved"), original);
    assert_eq!(relay.writes.load(Ordering::SeqCst), 1);
}
