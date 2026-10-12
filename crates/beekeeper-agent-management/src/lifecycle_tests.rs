use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

struct DisappearingRelay {
    owner: Keys,
    relay: Keys,
    profiles: Vec<Event>,
    observation: Event,
    definition: Event,
    retirement: Mutex<Option<Event>>,
    metadata_reads: AtomicUsize,
    published: Mutex<Vec<Event>>,
}
impl RelayTransport for DisappearingRelay {
    fn keys(&self) -> &Keys {
        &self.owner
    }
    fn relay_url(&self) -> &str {
        "ws://127.0.0.1:31415"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused fixture scope".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        Ok(self.relay.public_key().to_hex())
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        let kind = filter["kinds"][0].as_u64().expect("explicit kind");
        let events = match kind {
            0 => self
                .profiles
                .iter()
                .filter(|event| filter["authors"][0] == event.pubkey.to_hex())
                .cloned()
                .collect(),
            44223 if self.metadata_reads.fetch_add(1, Ordering::SeqCst) == 0 => {
                vec![self.observation.clone()]
            }
            30177 => vec![self.definition.clone()],
            8002 => self
                .retirement
                .lock()
                .expect("retirement")
                .iter()
                .cloned()
                .collect(),
            44224 => self
                .published
                .lock()
                .expect("published")
                .iter()
                .filter(|event| event.kind.as_u16() == 44224)
                .cloned()
                .collect(),
            44221 => self
                .published
                .lock()
                .expect("published")
                .iter()
                .filter(|event| {
                    event.kind.as_u16() == 44221
                        && (filter.get("ids").is_none() || filter["ids"][0] == event.id.to_hex())
                })
                .cloned()
                .collect(),
            _ => vec![],
        };
        Ok(events
            .into_iter()
            .map(|event| serde_json::to_value(event).expect("event"))
            .collect())
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        assert!([44221, 9035, 5].contains(&event.kind.as_u16()));
        if event.kind.as_u16() == 9035 {
            let actor = event
                .tags
                .iter()
                .find(|tag| tag.as_slice().first().is_some_and(|v| v == "p"))
                .expect("actor")
                .as_slice()[1]
                .clone();
            let delta = EventBuilder::new(Kind::Custom(8002), "")
                .tags([
                    Tag::parse(["p", actor.as_str()]).expect("p"),
                    Tag::parse(["e", &event.id.to_hex()]).expect("e"),
                    Tag::parse(["consent", "owner", &event.pubkey.to_hex()]).expect("consent"),
                    Tag::parse(["reason", "retired"]).expect("reason"),
                ])
                .sign_with_keys(&self.relay)
                .expect("retirement delta");
            *self.retirement.lock().expect("retirement") = Some(delta);
        }
        self.published.lock().expect("published").push(event);
        Ok(json!({"accepted":true,"message":"stored"}))
    }
}
fn attested_profile(owner: &Keys, identity: &Keys) -> Event {
    let auth = beekeeper_sdk::nip_oa::compute_auth_tag(owner, &identity.public_key(), "")
        .expect("attestation");
    let values: Vec<String> = serde_json::from_str(&auth).expect("tag");
    EventBuilder::new(Kind::Metadata, "{}")
        .tags([Tag::parse(values).expect("tag")])
        .sign_with_keys(identity)
        .expect("profile")
}
fn fixture() -> (DisappearingRelay, LifecycleRequest) {
    use beekeeper_core::coding_session_payload::{
        Capabilities, SessionMetadata, SessionStatus, METADATA_SCHEMA,
    };
    let owner = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let channel = Uuid::new_v4();
    let target = CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: Uuid::new_v4().to_string(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    };
    let metadata = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: Some(actor.public_key().to_hex()),
        role: None,
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: None,
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
        compose_ref: None,
    };
    let content = serde_json::to_string(&metadata).expect("metadata");
    let observation = beekeeper_sdk::build_coding_session_metadata(channel, &target, &content)
        .expect("metadata builder")
        .sign_with_keys(&provider)
        .expect("metadata signature");
    let profiles = vec![
        attested_profile(&owner, &actor),
        attested_profile(&owner, &provider),
    ];
    let content = beekeeper_core::managed_agent::ManagedAgentEventContent {
        name: "fixture".into(),
        persona_id: None,
        system_prompt: Some("ordinary work".into()),
        model: None,
        provider: None,
        persona_source_version: None,
        parallelism: 1,
        respond_to: beekeeper_core::managed_agent::RespondTo::OwnerOnly,
        respond_to_allowlist: vec![],
        home_role: None,
        project_digest: None,
        project_withdrawn: false,
    };
    let definition = beekeeper_sdk::managed_agent::build_managed_agent_event(
        &actor.public_key().to_hex(),
        &content,
    )
    .expect("definition builder")
    .sign_with_keys(&owner)
    .expect("definition");
    let request = LifecycleRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: actor.public_key().to_hex(),
        channels: vec![channel],
        wait_secs: 1,
    };
    (
        DisappearingRelay {
            owner,
            relay: Keys::generate(),
            profiles,
            observation,
            definition,
            retirement: Mutex::new(None),
            metadata_reads: AtomicUsize::new(0),
            published: Mutex::new(vec![]),
        },
        request,
    )
}
#[tokio::test]
async fn disappearance_without_signed_stop_never_certifies_terminal_and_retry_retains_exact_target()
{
    let (client, request) = fixture();
    let home = tempfile::tempdir().expect("home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let result = stop(&request, &client, &context)
        .await
        .expect("pending stop");
    assert_eq!(result["terminal"], false);
    assert_eq!(result["pending"].as_array().expect("pending").len(), 1);
    let expected_target = result["pending"][0]["target"].clone();
    let path = local::operation_path(&context, request.operation_id).expect("journal path");
    let first: LifecycleOperation =
        serde_json::from_slice(&std::fs::read(&path).expect("journal")).expect("journal decode");
    assert_eq!(first.inventory.len(), 1);
    assert_eq!(first.events.len(), 1);
    let event_id = first.events[0].id;
    let retry = stop(&request, &client, &context)
        .await
        .expect("pending retry");
    assert_eq!(retry["terminal"], false);
    assert_eq!(retry["pending"][0]["target"], expected_target);
    let second: LifecycleOperation =
        serde_json::from_slice(&std::fs::read(path).expect("journal")).expect("journal decode");
    assert_eq!(second.inventory, first.inventory);
    assert_eq!(second.events[0].id, event_id);
    assert_eq!(
        client.published.lock().expect("publications").len(),
        1,
        "retry reused the exact accepted stop event"
    );
}
#[tokio::test]
async fn deletion_captured_inventory_seeds_stop_even_when_all_observations_are_gone() {
    let (client, request) = fixture();
    let captured = inventory(&request, &client)
        .await
        .expect("initial inventory");
    assert_eq!(captured.len(), 1);
    let home = tempfile::tempdir().expect("home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let result = stop_inner(&request, &client, &context, &captured)
        .await
        .expect("pending deletion stop");
    assert_eq!(result["terminal"], false);
    assert_eq!(
        result["pending"][0]["target"],
        serde_json::to_value(&captured[0].target).expect("target")
    );
}

#[tokio::test]
async fn actual_delete_flow_retains_captured_responsibility_and_custody_after_retirement_when_visibility_disappears(
) {
    let (client, scope) = fixture();
    let home = tempfile::tempdir().expect("home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let request = DeleteRequest {
        operation_id: scope.operation_id,
        agent_pubkey: scope.agent_pubkey.clone(),
        expected_revision: client.definition.id.to_hex(),
        channels: scope.channels.clone(),
        wait_secs: 1,
        remote_deployment: false,
    };
    let pending = delete(&request, &client, &context)
        .await
        .expect("pending delete");
    assert_eq!(pending["terminal"], false);
    assert_eq!(pending["retirement_requested"], true);
    assert_eq!(
        pending["stop"]["pending"]
            .as_array()
            .expect("targets")
            .len(),
        1
    );
    let first_stop = pending["stop"]["pending"][0]["target"].clone();
    let retried = delete(&request, &client, &context)
        .await
        .expect("pending replay");
    assert_eq!(retried["terminal"], false);
    assert_eq!(retried["stop"]["pending"][0]["target"], first_stop);
    let publications = client.published.lock().expect("published");
    assert_eq!(
        publications
            .iter()
            .filter(|event| event.kind.as_u16() == 9035)
            .count(),
        1,
        "exact relay delta reuses prior retirement instead of replaying its transient request"
    );
    assert_eq!(
        publications
            .iter()
            .filter(|event| event.kind.as_u16() == 44221)
            .count(),
        1,
        "exact stop event retained"
    );
    assert_eq!(
        publications
            .iter()
            .filter(|event| event.kind.as_u16() == 5)
            .count(),
        0,
        "no deletion without terminal receipt"
    );
    assert_eq!(retried["custody"], "retained");
}

#[tokio::test]
async fn relay_only_uncertain_start_without_metadata_cannot_be_deleted_as_zero_work() {
    let (client, mut request) = fixture();
    client.metadata_reads.store(1, Ordering::SeqCst);
    request.wait_secs = 1;
    let metadata = beekeeper_core::coding_session_payload::decode_coding_session_metadata(
        &client.observation.content,
    )
    .expect("fixture target");
    let provider = Keys::generate();
    // Replace provider profile with a genuine same-owner attestation for this
    // independent signed failed-start fixture.
    let mut client = client;
    client
        .profiles
        .push(attested_profile(&client.owner, &provider));
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.into(),
        command_id: Uuid::new_v4().to_string(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: None,
            genesis_ref: None,
            provider_instance_ref: "claude-primary".try_into().expect("alias"),
            provider_authority_pubkey: provider.public_key().to_hex(),
            model: None,
            title: None,
            initial_turn: None,
            actor: Some(request.agent_pubkey.clone()),
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let command =
        beekeeper_sdk::build_coding_session_lifecycle_command(request.channels[0], &payload)
            .expect("command")
            .sign_with_keys(&client.owner)
            .expect("owner signature");
    let mut answer = beekeeper_core::coding_session_payload::LifecycleReceipt::failed(
        &payload.command_id,
        "CREATE_START_OUTCOME_UNCONFIRMED",
        "startup completion not established",
    );
    answer.session = Some(metadata.session.clone());
    let receipt = beekeeper_sdk::build_coding_session_lifecycle_receipt(
        request.channels[0],
        &payload.command_id,
        &serde_json::to_string(&answer).expect("answer"),
    )
    .expect("receipt")
    .sign_with_keys(&provider)
    .expect("provider signature");
    client
        .published
        .lock()
        .expect("fixture exchange")
        .push(command);
    assert!(
        matches!(
            inventory(&request, &client).await,
            Err(Error::Unconfirmed(_))
        ),
        "accepted Create without any receipt cannot certify zero work"
    );
    let mut missing_target = answer.clone();
    missing_target.session = None;
    let uncertain_without_target = beekeeper_sdk::build_coding_session_lifecycle_receipt(
        request.channels[0],
        &payload.command_id,
        &serde_json::to_string(&missing_target).expect("missing-target outcome"),
    )
    .expect("builder")
    .sign_with_keys(&provider)
    .expect("provider signature");
    client
        .published
        .lock()
        .expect("fixture exchange")
        .push(uncertain_without_target);
    assert!(
        matches!(
            inventory(&request, &client).await,
            Err(Error::Unconfirmed(_))
        ),
        "null-target uncertainty cannot certify zero work"
    );
    client
        .published
        .lock()
        .expect("independent target-shape fixture")
        .pop();
    client
        .published
        .lock()
        .expect("fixture exchange")
        .push(receipt);
    let rows = inventory(&request, &client)
        .await
        .expect("relay-only inventory");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].target, metadata.session);
    let home = tempfile::tempdir().expect("empty second-client home");
    let context = Context {
        home: home.path().to_path_buf(),
        instance: beekeeper_host_core::layout::Instance::Production,
        native_home: None,
    };
    let result = delete(
        &DeleteRequest {
            operation_id: request.operation_id,
            agent_pubkey: request.agent_pubkey,
            expected_revision: client.definition.id.to_hex(),
            channels: request.channels,
            wait_secs: 1,
            remote_deployment: false,
        },
        &client,
        &context,
    )
    .await
    .expect("truthful pending");
    assert_eq!(result["terminal"], false);
    assert_eq!(result["custody"], "retained");
    assert!(
        !client
            .published
            .lock()
            .expect("published")
            .iter()
            .any(|event| event.kind.as_u16() == 5),
        "no tombstone before exact terminal"
    );
}

struct QuiescenceRetryRelay {
    inner: DisappearingRelay,
    provider: Keys,
    absent: std::sync::atomic::AtomicBool,
    outcomes: Mutex<Vec<Event>>,
}
impl RelayTransport for QuiescenceRetryRelay {
    fn keys(&self) -> &Keys {
        &self.inner.owner
    }
    fn relay_url(&self) -> &str {
        self.inner.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused fixture scope".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        self.inner.relay_identity().await
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        if filter["kinds"][0] == 44224 {
            return Ok(self
                .outcomes
                .lock()
                .expect("outcomes")
                .iter()
                .map(|event| serde_json::to_value(event).expect("event"))
                .collect());
        }
        self.inner.query_all(filter).await
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        if event.kind.as_u16() == 44221 {
            let payload=beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content).expect("stop payload");
            if let CodingSessionLifecycleAction::SessionStop { session, .. } = payload.action {
                let receipt = if self.absent.load(Ordering::SeqCst) {
                    beekeeper_core::coding_session_payload::LifecycleReceipt::stopped(
                        &payload.command_id,
                        &session,
                    )
                } else {
                    beekeeper_core::coding_session_payload::LifecycleReceipt::failed(
                        &payload.command_id,
                        "SESSION_QUIESCENCE_UNCONFIRMED",
                        "physical group remains present",
                    )
                };
                let channel = event
                    .tags
                    .iter()
                    .find_map(|tag| {
                        let values = tag.as_slice();
                        (values.first().map(String::as_str) == Some("h"))
                            .then(|| values.get(1).and_then(|v| Uuid::parse_str(v).ok()))
                            .flatten()
                    })
                    .expect("channel");
                let signed = beekeeper_sdk::build_coding_session_lifecycle_receipt(
                    channel,
                    &payload.command_id,
                    &serde_json::to_string(&receipt).expect("receipt"),
                )
                .expect("builder")
                .sign_with_keys(&self.provider)
                .expect("signature");
                self.outcomes.lock().expect("outcomes").push(signed);
            }
        }
        self.inner.submit_event(event).await
    }
}
#[tokio::test]
async fn same_management_stop_retries_only_verified_quiescence_failure_with_journaled_new_delivery()
{
    let (mut inner, request) = fixture();
    let provider = Keys::generate();
    inner
        .profiles
        .push(attested_profile(&inner.owner, &provider));
    let target = beekeeper_core::coding_session_payload::decode_coding_session_metadata(
        &inner.observation.content,
    )
    .expect("metadata")
    .session;
    inner.observation = beekeeper_sdk::build_coding_session_metadata(
        request.channels[0],
        &target,
        &inner.observation.content,
    )
    .expect("metadata")
    .sign_with_keys(&provider)
    .expect("provider signature");
    let client = QuiescenceRetryRelay {
        inner,
        provider,
        absent: std::sync::atomic::AtomicBool::new(false),
        outcomes: Mutex::new(vec![]),
    };
    let home = tempfile::tempdir().expect("second client home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Production,
        native_home: None,
    };
    let first = stop(&request, &client, &context)
        .await
        .expect("pending first delivery");
    assert_eq!(first["terminal"], false);
    let first_events = client.inner.published.lock().expect("events").clone();
    assert_eq!(first_events.len(), 1, "one attempt per invocation");
    client.absent.store(true, Ordering::SeqCst);
    let retry = stop(&request, &client, &context)
        .await
        .expect("same logical operation after verified physical exit");
    assert_eq!(retry["terminal"], true);
    assert_eq!(retry["operation_id"], request.operation_id.to_string());
    let events = client.inner.published.lock().expect("events").clone();
    assert_eq!(events.len(), 2);
    let first =
        beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &events[0].content,
        )
        .expect("first");
    let next =
        beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(
            &events[1].content,
        )
        .expect("renewed");
    assert_ne!(first.command_id, next.command_id);
    assert_eq!(
        first.action, next.action,
        "same cumulative exact target and provider"
    );
    let again = stop(&request, &client, &context)
        .await
        .expect("settled replay");
    assert_eq!(again["terminal"], true);
    assert_eq!(
        client.inner.published.lock().expect("events").len(),
        2,
        "no extra delivery after terminal proof"
    );
}
