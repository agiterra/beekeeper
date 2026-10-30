use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Mutex,
};

struct DisappearingRelay {
    transport_url: Option<String>,
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
        self.transport_url
            .as_deref()
            .unwrap_or("ws://127.0.0.1:31415")
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
        credential_source: None,
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
            transport_url: None,
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
    let saved: LifecycleOperation = serde_json::from_slice(
        &std::fs::read(
            local::operation_path(&context, request.operation_id).expect("delete journal path"),
        )
        .expect("retained delete journal"),
    )
    .expect("journal");
    assert_eq!(
        saved.events.len(),
        1,
        "pending work has only its retirement reservation"
    );
    assert_eq!(saved.events[0].kind.as_u16(), 9035);
    assert!(
        saved.events.iter().all(|event| event.kind.as_u16() != 5),
        "no tombstone is signed while all-target Stop remains unproved"
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

struct StartAvailabilityRelay {
    base: DisappearingRelay,
    lease: Option<Event>,
    membership: Event,
}
impl RelayTransport for StartAvailabilityRelay {
    fn keys(&self) -> &Keys {
        self.base.keys()
    }
    fn relay_url(&self) -> &str {
        self.base.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused scope".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        self.base.relay_identity().await
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        match filter["kinds"][0].as_u64() {
            Some(39002) => {
                let tags = &self.membership.tags;
                let matches = ["d", "p"].iter().all(|name| {
                    filter.get(format!("#{name}")).is_none_or(|values| {
                        tags.iter().any(|tag| {
                            tag.as_slice().first().is_some_and(|key| key == name)
                                && tag
                                    .as_slice()
                                    .get(1)
                                    .is_some_and(|value| values[0] == *value)
                        })
                    })
                });
                Ok(if matches {
                    vec![serde_json::to_value(&self.membership).expect("signed membership")]
                } else {
                    vec![]
                })
            }
            Some(44223) => Ok(vec![
                serde_json::to_value(&self.base.observation).expect("signed metadata")
            ]),
            Some(24223) => Ok(self
                .lease
                .iter()
                .map(|event| serde_json::to_value(event).expect("signed lease"))
                .collect()),
            _ => self.base.query_all(filter).await,
        }
    }
    async fn submit_event(&self, _: Event) -> Result<Value, Error> {
        panic!("availability observation/replay must not publish or create an execution")
    }
}

#[tokio::test]
async fn created_start_observes_exact_live_lease_and_adopted_replay_never_creates_again() {
    use beekeeper_core::coding_session_lease::{CodingSessionLease, CodingSessionLeaseState};
    let (mut base, scope) = fixture();
    let provider = Keys::generate();
    base.profiles.push(attested_profile(&base.owner, &provider));
    base.observation = EventBuilder::new(Kind::Custom(44223), base.observation.content.clone())
        .tags(base.observation.tags.iter().cloned())
        .sign_with_keys(&provider)
        .expect("provider metadata");
    let metadata: beekeeper_core::coding_session_payload::SessionMetadata =
        serde_json::from_str(&base.observation.content).expect("metadata");
    let runtime_alias = metadata
        .provider
        .as_ref()
        .expect("advertised runtime alias")
        .as_str()
        .to_owned();
    let target = metadata.session;
    let channel = scope.channels[0];
    let lease =
        CodingSessionLease::new(target.clone(), CodingSessionLeaseState::Live, 1).expect("lease");
    let live = beekeeper_sdk::build_coding_session_lease(channel, "accepted-create", &lease)
        .expect("lease builder")
        .sign_with_keys(&provider)
        .expect("provider lease");
    let membership = EventBuilder::new(Kind::Custom(39002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).expect("channel"),
            Tag::parse(["p", &base.owner.public_key().to_hex()]).expect("caller"),
            Tag::parse(["p", &scope.agent_pubkey]).expect("actor"),
            Tag::parse(["p", &provider.public_key().to_hex()]).expect("provider"),
        ])
        .sign_with_keys(&base.relay)
        .expect("relay-signed current membership");
    let mut relay = StartAvailabilityRelay {
        base,
        lease: Some(live.clone()),
        membership,
    };
    let remote_home = tempfile::tempdir().expect("empty second-client home");
    let remote_context = Context {
        home: remote_home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let remote_args = crate::RemoteStartRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: scope.agent_pubkey.clone(),
        expected_revision: relay.base.definition.id.to_hex(),
        channel,
        provider_authority: provider.public_key().to_hex(),
        provider_instance: runtime_alias,
        project: None,
        wait_secs: 1,
    };
    let remote_live = crate::start_remote(&remote_args, &relay, &remote_context)
        .await
        .expect("active remote start observes only");
    assert_eq!(remote_live["runtime_available"], true);
    assert_eq!(remote_live["execution_created"], false);
    let accepted = json!({"agent_pubkey":scope.agent_pubkey,"target":target,"operation_id":Uuid::new_v4(),"host_admitted":true,"receipt_event_id":"exact-accepted-receipt"});
    let observed = observe_created_start(
        accepted.clone(),
        channel,
        &provider.public_key().to_hex(),
        &relay,
    )
    .await
    .expect("accepted start observation");
    assert_eq!(observed["runtime_available"], true);
    assert_eq!(
        observed["availability"],
        "verified_unexpired_provider_lease"
    );
    assert_eq!(observed["receipt_event_id"], accepted["receipt_event_id"]);
    assert_eq!(observed["operation_id"], accepted["operation_id"]);
    for event in [
        None,
        Some(
            EventBuilder::new(Kind::Custom(24223), live.content.clone())
                .tags(live.tags.iter().cloned())
                .custom_created_at(nostr::Timestamp::from(
                    nostr::Timestamp::now().as_secs() - 181,
                ))
                .sign_with_keys(&provider)
                .expect("expired lease"),
        ),
    ] {
        relay.lease = event;
        let missing = observe_created_start(
            accepted.clone(),
            channel,
            &provider.public_key().to_hex(),
            &relay,
        )
        .await
        .expect("missing liveness is truthful");
        assert_eq!(missing["runtime_available"], false);
        assert_eq!(missing["pending"], true);
        assert_eq!(missing["host_admitted"], true);
        let remote_pending = crate::start_remote(&remote_args, &relay, &remote_context)
            .await
            .expect("missing remote lease does not create");
        assert_eq!(remote_pending["runtime_available"], false);
        assert_eq!(remote_pending["pending"], true);
    }
    relay.lease = Some(live);
    let home = tempfile::tempdir().expect("owned home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let operation = Uuid::new_v4();
    beekeeper_host_core::ownership::record_host_enrollment(
        &context.home,
        context.instance,
        &scope.agent_pubkey,
        relay.relay_url(),
        &operation.to_string(),
        &relay.base.definition.id.to_hex(),
        Some(target.clone()),
    )
    .expect("accepted canonical ownership");
    let args = crate::CreateRequest {
        operation_id: operation,
        agent_pubkey: Some(scope.agent_pubkey),
        channel,
        display_name: "fixture".into(),
        system_prompt: "ordinary work".into(),
        activate: true,
        ..Default::default()
    };
    for _ in 0..2 {
        let replay = start(&args, &relay, &context)
            .await
            .expect("accepted start replay");
        assert_eq!(replay["runtime_available"], true);
        assert_eq!(replay["execution_created"], false);
        assert_eq!(
            replay["target"],
            serde_json::to_value(&target).expect("target")
        );
    }
}

#[test]
fn linked_start_compares_canonical_projection_and_exact_retained_private_inputs() {
    let (base, _) = fixture();
    let mut public: beekeeper_core::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&base.definition.content).expect("definition");
    public.persona_id = Some("private-persona".into());
    public.system_prompt = Some("retained native instructions".into());
    public.model = Some("configured-model".into());
    public.persona_source_version = Some("ab".repeat(32));
    let private = crate::PrivateConfiguration {
        record: json!({"effective_command":"codex-acp","system_prompt":"retained native instructions","model":"configured-model","effective_env":{"OPTION":"preserved"}}),
    };
    let make = || crate::CreateRequest {
        system_prompt: "retained native instructions".into(),
        model: Some("configured-model".into()),
        private_config: Some(private.clone()),
        public_definition: Some(public.clone()),
        ..Default::default()
    };
    let args = make();
    let request = json!({"prompt":args.system_prompt,"model":args.model,"privateConfig":private});
    let projected = public.public_projection();
    assert_ne!(public, projected);
    assert_eq!(
        args.public_definition
            .as_ref()
            .expect("public")
            .public_projection(),
        projected
    );
    assert!(linked_start_inputs_match(&args, &request));
    let mut changed = make();
    changed.system_prompt = "changed native instructions".into();
    assert!(!linked_start_inputs_match(&changed, &request));
    changed = make();
    changed.model = Some("other-model".into());
    assert!(!linked_start_inputs_match(&changed, &request));
    changed = make();
    changed.private_config.as_mut().expect("private").record["effective_env"] =
        json!({"OPTION":"changed"});
    assert!(!linked_start_inputs_match(&changed, &request));
    for field in ["policy", "persona"] {
        let mut changed = public.clone();
        if field == "policy" {
            changed.respond_to = beekeeper_core::managed_agent::RespondTo::Anyone;
        } else {
            changed.persona_id = Some("different-persona".into());
        }
        assert_ne!(changed.public_projection(), projected);
    }
}

#[tokio::test]
async fn linked_adopted_guard_reads_exact_protected_custody_and_accepted_journal() {
    use beekeeper_host_core::{
        config::{HostConfig, HOST_CONFIG_VERSION},
        layout, provision,
    };
    let (mut relay, scope) = fixture();
    let home = tempfile::tempdir().expect("isolated custody");
    let context = Context {
        home: home.path().into(),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    let provider =
        provision::mint_provider_record(&relay.owner, relay.relay_url()).expect("provider");
    let actor = provision::mint_agent_identity(&relay.owner).expect("actor");
    let config = HostConfig {
        version: HOST_CONFIG_VERSION,
        instance: context.instance,
        relay_url: relay.relay_url().into(),
        provider_pubkey: provider.provider_pubkey.clone(),
        session_provider_base_dir: home.path().join("provider"),
        provider_state_dir: home.path().join("state"),
        runtimes: vec![],
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        checkpoint_on_stop: false,
        capabilities: Vec::new(),
        provider_command: None,
        written_at: String::new(),
    };
    provision::write_commissioning(&context.home, &config, &provider).expect("commission");
    local::actor_custody(
        &config.provider_state_dir,
        &actor.pubkey,
        Some((&actor.private_key_nsec, &actor.auth_tag)),
    )
    .expect("scoped custody");
    let mut content: beekeeper_core::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&relay.definition.content).expect("definition");
    content.persona_id = Some("private-persona".into());
    content.system_prompt = Some("native instructions".into());
    content.model = Some("configured-model".into());
    let definition =
        beekeeper_sdk::managed_agent::build_managed_agent_event(&actor.pubkey, &content)
            .expect("slim builder")
            .sign_with_keys(&relay.owner)
            .expect("signed");
    let private = crate::PrivateConfiguration {
        record: json!({"pubkey":actor.pubkey,"effective_command":"bash","system_prompt":"native instructions","model":"configured-model","effective_env":{"OPTION":"preserved"}}),
    };
    let dir = config.provider_state_dir.join("actors").join(&actor.pubkey);
    let enrollment = json!({"actorPubkey":actor.pubkey,"definition":definition,"channelId":scope.channels[0],"cwd":home.path(),"desiredRunning":true});
    local::save(&dir.join("enrollment.json"), &enrollment).expect("accepted enrollment");
    local::save(
        &dir.join("configuration.json"),
        &json!({
            "schema":"beekeeper-private-agent-config/v1", "actorPubkey":actor.pubkey,
            "relayUrl":relay.relay_url(), "definition":definition, "record":private.record,
            "channelId":scope.channels[0],"cwd":home.path(),"desiredRunning":true,
        }),
    )
    .expect("protected accepted snapshot fixture");
    let target: CodingSessionTarget = serde_json::from_str::<
        beekeeper_core::coding_session_payload::SessionMetadata,
    >(&relay.observation.content)
    .expect("target")
    .session;
    let operation = Uuid::new_v4();
    let args = crate::CreateRequest {
        operation_id: operation,
        channel: scope.channels[0],
        agent_pubkey: Some(actor.pubkey.clone()),
        display_name: content.name.clone(),
        system_prompt: "native instructions".into(),
        model: Some("configured-model".into()),
        public_definition: Some(content.clone()),
        private_config: Some(private.clone()),
        activate: true,
        ..Default::default()
    };
    let original = local::Operation {
        hire: None,
        hire_decision: None,
        request: json!({"owner":relay.keys().public_key().to_hex(),"relay":relay.relay_url(),"prompt":args.system_prompt,"model":args.model,"privateConfig":private}),
        actor_pubkey: actor.pubkey.clone(),
        events: vec![definition.clone()],
        target: Some(target.clone()),
        receipt_event_id: Some("ab".repeat(32)),
        predecessor: None,
        custody_handoff: None,
    };
    local::save(
        &local::operation_path(&context, operation).expect("path"),
        &original,
    )
    .expect("accepted journal");
    beekeeper_host_core::ownership::record_host_enrollment(
        &context.home,
        context.instance,
        &actor.pubkey,
        relay.relay_url(),
        &operation.to_string(),
        &definition.id.to_hex(),
        Some(target),
    )
    .expect("accepted ownership");
    let held = beekeeper_host_core::ownership::for_pair(
        &context.home,
        context.instance,
        &actor.pubkey,
        relay.relay_url(),
    )
    .expect("read")
    .expect("owned");
    let slim = content.public_projection();
    verify_adopted_start(&args, &slim, &held, &relay, &context)
        .expect("exact retained linked request");
    relay.definition = definition;
    relay.profiles.push(attested_profile(
        &relay.owner,
        &Keys::parse(&actor.private_key_nsec).expect("fixture actor"),
    ));
    relay.transport_url = Some("http://127.0.0.1:31415".into());
    for _ in 0..2 {
        let result = start(&args, &relay, &context)
            .await
            .expect("HTTP observes exact adopted linked operation");
        assert_eq!(result["execution_created"], false);
        assert_eq!(
            result["target"],
            serde_json::to_value(&held.target).expect("target")
        );
        assert!(
            relay
                .published
                .lock()
                .expect("publication capture")
                .is_empty(),
            "adopted HTTP replay publishes no Create or lifecycle operation"
        );
    }
    let bytes = std::fs::read(dir.join("configuration.json")).expect("original bytes");
    let mut altered: Value = serde_json::from_slice(&bytes).expect("snapshot");
    altered["record"]["system_prompt"] = json!("new private instructions");
    local::save(&dir.join("configuration.json"), &altered).expect("concurrent private update");
    assert!(matches!(
        verify_adopted_start(&args, &slim, &held, &relay, &context),
        Err(Error::Conflict(_))
    ));
    assert_eq!(
        std::fs::read(local::operation_path(&context, operation).expect("path"))
            .expect("retained operation"),
        serde_json::to_vec_pretty(&original).expect("original operation bytes")
    );
}

#[test]
fn retirement_delta_accepts_exact_owner_or_admin_consent_and_refuses_foreign_scope() {
    let owner = Keys::generate();
    let relay = Keys::generate();
    let other = Keys::generate();
    let actor = Keys::generate().public_key().to_hex();
    let request = EventBuilder::new(Kind::Custom(9035), "")
        .sign_with_keys(&owner)
        .expect("request");
    let delta = |signer: &Keys, actor: &str, source: &str, path: &str, consent_signer: &str| {
        EventBuilder::new(Kind::Custom(8002), "")
            .tags([
                Tag::parse(["p", actor]).expect("actor"),
                Tag::parse(["e", source]).expect("request"),
                Tag::parse(["reason", "retired"]).expect("reason"),
                Tag::parse(["consent", path, consent_signer]).expect("consent"),
            ])
            .sign_with_keys(signer)
            .expect("signed delta")
    };
    let relay_pubkey = relay.public_key().to_hex();
    let owner_pubkey = owner.public_key().to_hex();
    let source = request.id.to_hex();
    for path in ["owner", "admin"] {
        let valid = delta(&relay, &actor, &source, path, &owner_pubkey);
        assert!(retirement_delta_matches(
            &valid,
            &relay_pubkey,
            &actor,
            &request
        ));
        let mut tampered = valid;
        tampered.content = "forged".into();
        assert!(!retirement_delta_matches(
            &tampered,
            &relay_pubkey,
            &actor,
            &request
        ));
    }
    for invalid in [
        delta(&other, &actor, &source, "admin", &owner_pubkey),
        delta(&relay, &"ab".repeat(32), &source, "admin", &owner_pubkey),
        delta(&relay, &actor, &"cd".repeat(32), "admin", &owner_pubkey),
        delta(
            &relay,
            &actor,
            &source,
            "admin",
            &other.public_key().to_hex(),
        ),
        delta(&relay, &actor, &source, "self", &owner_pubkey),
    ] {
        assert!(!retirement_delta_matches(
            &invalid,
            &relay_pubkey,
            &actor,
            &request
        ));
    }
}

// Helper executes only at delete's already-proved all-target Stop boundary.
struct LateTombstoneRelay {
    base: DisappearingRelay,
    expired_id: String,
    stored: Mutex<Vec<Event>>,
    posts: AtomicUsize,
    drop_reply: std::sync::atomic::AtomicBool,
    stale_refusal: bool,
}
impl RelayTransport for LateTombstoneRelay {
    fn keys(&self) -> &Keys {
        self.base.keys()
    }
    fn relay_url(&self) -> &str {
        self.base.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused".into()))
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        if filter["kinds"][0] == 5 {
            return Ok(self
                .stored
                .lock()
                .expect("stored")
                .iter()
                .filter(|event| {
                    filter.get("ids").is_none() || filter["ids"][0] == event.id.to_hex()
                })
                .map(|event| serde_json::to_value(event).expect("event"))
                .collect());
        }
        self.base.query_all(filter).await
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        assert_eq!(event.kind.as_u16(), 5);
        self.posts.fetch_add(1, Ordering::SeqCst);
        let count = |key: &str| {
            event
                .tags
                .iter()
                .filter(|tag| tag.as_slice().first().is_some_and(|value| value == key))
                .count()
        };
        let e_count = count("e");
        let a_count = count("a");
        if e_count + a_count != 1 {
            return Err(Error::Refused(format!("invalid: deletion events must reference exactly one target via e or a tag (got e={e_count}, a={a_count})")));
        }
        if event.id.to_hex() == self.expired_id {
            return if self.stale_refusal {
                Err(Error::Relay {
                    status: 400,
                    body: "invalid: event timestamp too far from server time".into(),
                })
            } else {
                Err(Error::Unconfirmed("submission outcome unknown".into()))
            };
        }
        self.stored.lock().expect("stored").push(event);
        if self.drop_reply.swap(false, Ordering::SeqCst) {
            return Err(Error::Other("accepted reply lost".into()));
        }
        Ok(json!({"accepted":true,"message":"stored"}))
    }
}
#[tokio::test]
async fn late_tombstone_renews_only_explicit_expired_absent_delivery_and_preserves_lost_reply_journal(
) {
    for _ in [()] {
        let (base, scope) = fixture();
        let args = DeleteRequest {
            operation_id: Uuid::new_v4(),
            agent_pubkey: scope.agent_pubkey,
            expected_revision: base.definition.id.to_hex(),
            channels: scope.channels,
            wait_secs: 1,
            remote_deployment: false,
        };
        let builder = nostr::EventBuilder::delete(
            nostr::nips::nip09::EventDeletionRequest::new().id(base.definition.id),
        );
        let builder = builder.custom_created_at(nostr::Timestamp::from(
            nostr::Timestamp::now().as_secs() - 901,
        ));
        let expired = builder
            .sign_with_keys(base.keys())
            .expect("original signed candidate");
        let archive = EventBuilder::new(Kind::Custom(9035), "")
            .sign_with_keys(base.keys())
            .expect("original retirement");
        let relay = LateTombstoneRelay {
            base,
            expired_id: expired.id.to_hex(),
            stored: Mutex::new(vec![]),
            posts: AtomicUsize::new(0),
            drop_reply: std::sync::atomic::AtomicBool::new(true),
            stale_refusal: true,
        };
        let mut operation = LifecycleOperation {
            request: json!({"original":"immutable request"}),
            events: vec![archive.clone(), expired.clone()],
            inventory: vec![],
        };
        let dir = tempfile::tempdir().expect("journal");
        let path = dir.path().join("delete.json");
        local::save(&path, &operation).expect("original operation");
        assert!(
            finish_delete_tombstone(&mut operation, &path, &args, &relay)
                .await
                .is_err()
        );
        assert_eq!(operation.events.len(), 3);
        assert_eq!(operation.events[0], archive);
        assert_eq!(operation.events[1], expired);
        let committed = std::fs::read(&path).expect("renewed intent before publication");
        let mut recovered: LifecycleOperation =
            serde_json::from_slice(&committed).expect("same logical operation");
        let stored = finish_delete_tombstone(&mut recovered, &path, &args, &relay)
            .await
            .expect("exact accepted lost-reply event reused");
        assert_eq!(stored, recovered.events[2]);
        assert!(
            stored
                .tags
                .iter()
                .all(|tag| tag.as_slice().first().is_none_or(|key| key != "a")),
            "renewed legal target is e-only"
        );
        assert_eq!(relay.posts.load(Ordering::SeqCst), 2);
        assert_eq!(std::fs::read(&path).expect("retained bytes"), committed);
    }
}
#[tokio::test]
async fn unknown_tombstone_delivery_never_renews_or_rewrites_the_original_operation() {
    let (base, scope) = fixture();
    let args = DeleteRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: scope.agent_pubkey,
        expected_revision: base.definition.id.to_hex(),
        channels: scope.channels,
        wait_secs: 1,
        remote_deployment: false,
    };
    let expired = nostr::EventBuilder::delete(
        nostr::nips::nip09::EventDeletionRequest::new().id(base.definition.id),
    )
    .custom_created_at(nostr::Timestamp::from(
        nostr::Timestamp::now().as_secs() - 901,
    ))
    .sign_with_keys(base.keys())
    .expect("old signed candidate");
    let relay = LateTombstoneRelay {
        base,
        expired_id: expired.id.to_hex(),
        stored: Mutex::new(vec![]),
        posts: AtomicUsize::new(0),
        drop_reply: std::sync::atomic::AtomicBool::new(false),
        stale_refusal: false,
    };
    let mut operation = LifecycleOperation {
        request: json!({"original":"immutable"}),
        events: vec![expired.clone()],
        inventory: vec![],
    };
    let dir = tempfile::tempdir().expect("journal");
    let path = dir.path().join("delete.json");
    local::save(&path, &operation).expect("original");
    let bytes = std::fs::read(&path).expect("bytes");
    assert!(matches!(
        finish_delete_tombstone(&mut operation, &path, &args, &relay).await,
        Err(Error::Unconfirmed(_))
    ));
    assert_eq!(operation.events.len(), 1);
    assert_eq!(operation.events[0], expired);
    assert_eq!(std::fs::read(&path).expect("retained original"), bytes);
}

struct ConcurrentDeleteRelay {
    inner: LateTombstoneRelay,
    next: Event,
    raced: std::sync::atomic::AtomicBool,
}
impl RelayTransport for ConcurrentDeleteRelay {
    fn keys(&self) -> &Keys {
        self.inner.keys()
    }
    fn relay_url(&self) -> &str {
        self.inner.relay_url()
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused".into()))
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        if filter["kinds"][0] == 30177 && self.raced.load(Ordering::SeqCst) {
            return Ok(vec![
                serde_json::to_value(&self.next).expect("concurrent signed head")
            ]);
        }
        self.inner.query_all(filter).await
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        if event.id.to_hex() != self.inner.expired_id {
            // Current ingest accepts exactly one e OR a target. A renewed
            // exact-ID request must never carry the rejected legacy locator.
            assert!(event
                .tags
                .iter()
                .all(|tag| tag.as_slice().first().is_none_or(|key| key != "a")));
            assert!(event.tags.iter().any(
                |tag| tag.as_slice() == ["e", self.inner.base.definition.id.to_hex().as_str()]
            ));
            assert!(!event
                .tags
                .iter()
                .any(|tag| tag.as_slice() == ["e", self.next.id.to_hex().as_str()]));
            self.raced.store(true, Ordering::SeqCst);
        }
        self.inner.submit_event(event).await
    }
}
#[tokio::test]
async fn concurrent_owner_head_after_final_cas_survives_and_delete_returns_pending_with_stop_proof()
{
    let (base, scope) = fixture();
    let args = DeleteRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: scope.agent_pubkey,
        expected_revision: base.definition.id.to_hex(),
        channels: scope.channels,
        wait_secs: 1,
        remote_deployment: false,
    };
    let mut content: beekeeper_core::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&base.definition.content).expect("content");
    content.name = "new owner revision".into();
    let next =
        beekeeper_sdk::managed_agent::build_managed_agent_event(&args.agent_pubkey, &content)
            .expect("new builder")
            .custom_created_at(nostr::Timestamp::from(
                base.definition.created_at.as_secs() + 1,
            ))
            .sign_with_keys(base.keys())
            .expect("signed concurrent head");
    let old = nostr::EventBuilder::delete(
        nostr::nips::nip09::EventDeletionRequest::new().id(base.definition.id),
    )
    .custom_created_at(nostr::Timestamp::from(
        nostr::Timestamp::now().as_secs() - 901,
    ))
    .sign_with_keys(base.keys())
    .expect("expired original");
    let relay = ConcurrentDeleteRelay {
        inner: LateTombstoneRelay {
            base,
            expired_id: old.id.to_hex(),
            stored: Mutex::new(vec![]),
            posts: AtomicUsize::new(0),
            drop_reply: std::sync::atomic::AtomicBool::new(false),
            stale_refusal: true,
        },
        next: next.clone(),
        raced: std::sync::atomic::AtomicBool::new(false),
    };
    let mut operation = LifecycleOperation {
        request: json!({"original":"immutable"}),
        events: vec![old.clone()],
        inventory: vec![],
    };
    let dir = tempfile::tempdir().expect("journal");
    let path = dir.path().join("delete.json");
    local::save(&path, &operation).expect("original");
    let accepted = finish_delete_tombstone(&mut operation, &path, &args, &relay)
        .await
        .expect("renewed exact-ID tombstone");
    let stopped = json!({"terminal":true,"targets":"exact verified stopped witness fixture"});
    let outcome = confirm_delete_outcome(&args, &accepted, stopped.clone(), &relay)
        .await
        .expect("truthful postreadback");
    assert_eq!(outcome["terminal"], false);
    assert_eq!(outcome["definition_removed"], false);
    assert_eq!(outcome["conflict"], true);
    assert_eq!(outcome["effectiveRevision"], next.id.to_hex());
    assert_eq!(outcome["stop"], stopped);
    assert_eq!(operation.events[0], old);
    let current = management::configuration::current_head(&relay, &args.agent_pubkey)
        .await
        .expect("concurrent head survived");
    assert_eq!(current, next);
}

#[tokio::test]
async fn retained_invalid_coordinate_tombstone_is_refused_without_delivery_or_journal_rewrite() {
    let (base, scope) = fixture();
    let args = DeleteRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: scope.agent_pubkey,
        expected_revision: base.definition.id.to_hex(),
        channels: scope.channels,
        wait_secs: 1,
        remote_deployment: false,
    };
    let invalid = EventBuilder::delete(
        nostr::nips::nip09::EventDeletionRequest::new().id(base.definition.id),
    )
    .tags([Tag::parse([
        "a",
        &format!(
            "30177:{}:{}",
            base.keys().public_key().to_hex(),
            args.agent_pubkey
        ),
    ])
    .expect("retained invalid coordinate")])
    .sign_with_keys(base.keys())
    .expect("signed invalid historical event");
    let relay = LateTombstoneRelay {
        base,
        expired_id: invalid.id.to_hex(),
        stored: Mutex::new(vec![]),
        posts: AtomicUsize::new(0),
        drop_reply: std::sync::atomic::AtomicBool::new(false),
        stale_refusal: true,
    };
    let mut operation = LifecycleOperation {
        request: json!({"original":"immutable"}),
        events: vec![invalid],
        inventory: vec![],
    };
    let dir = tempfile::tempdir().expect("journal");
    let path = dir.path().join("delete.json");
    local::save(&path, &operation).expect("retained request");
    let original = std::fs::read(&path).expect("journal");
    assert!(matches!(
        finish_delete_tombstone(&mut operation, &path, &args, &relay).await,
        Err(Error::Auth(_))
    ));
    assert_eq!(relay.posts.load(Ordering::SeqCst), 0);
    assert_eq!(operation.events.len(), 1);
    assert_eq!(std::fs::read(path).expect("unchanged"), original);
}

#[tokio::test]
async fn historical_adopted_start_is_refused_without_publication_or_journal_mutation() {
    use beekeeper_host_core::{
        atomic_write, layout,
        ownership::{Phase, Transfer},
    };
    let (relay, scope) = fixture();
    let home = tempfile::tempdir().expect("historical custody");
    let context = Context {
        home: home.path().into(),
        instance: layout::Instance::Dev,
        native_home: None,
    };
    let target = serde_json::from_str::<beekeeper_core::coding_session_payload::SessionMetadata>(
        &relay.observation.content,
    )
    .expect("metadata")
    .session;
    let retained = Transfer {
        operation_id: Uuid::new_v4().to_string(),
        actor_pubkey: scope.agent_pubkey.clone(),
        relay_url: relay.relay_url().into(),
        legacy_nonce: "old-import-generation".into(),
        definition_event_id: relay.definition.id.to_hex(),
        target: Some(target),
        phase: Phase::Adopted,
    };
    let host = layout::host_dir(&context.home, context.instance);
    atomic_write::create_dir_all_restricted(&host).expect("custody");
    let path = host.join("managed-ownership.json");
    let original =
        serde_json::to_vec(&json!({"transfers":[retained],"history":[]})).expect("history");
    atomic_write::atomic_write_json_restricted(&path, &original).expect("retained journal");
    let args = crate::CreateRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: Some(scope.agent_pubkey),
        channel: scope.channels[0],
        display_name: "fixture".into(),
        system_prompt: "ordinary work".into(),
        activate: true,
        ..Default::default()
    };
    let result = start(&args, &relay, &context).await;
    assert!(
        matches!(result,Err(Error::Refused(message)) if message.contains("historical setup is retired"))
    );
    assert_eq!(relay.published.lock().expect("publications").len(), 0);
    assert_eq!(relay.metadata_reads.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(path).expect("unchanged history"), original);
    assert!(!host
        .join("management")
        .join("operations")
        .join(format!("{}.json", args.operation_id))
        .exists());
}

#[path = "lifecycle_empty_tests.rs"]
mod empty_tests;

#[test]
fn typed_tombstone_refusal_only_recognizes_explicit_http400_expiry_of_old_event() {
    let (relay, scope) = fixture();
    let builder = || {
        nostr::EventBuilder::delete(
            nostr::nips::nip09::EventDeletionRequest::new().id(relay.definition.id),
        )
    };
    let expired = builder()
        .custom_created_at(nostr::Timestamp::from(
            nostr::Timestamp::now().as_secs() - 901,
        ))
        .sign_with_keys(relay.keys())
        .expect("old signed event");
    let fresh = builder()
        .sign_with_keys(relay.keys())
        .expect("fresh signed event");
    let _ = scope;
    let expiry = "invalid: event timestamp too far from server time";
    assert!(pre_ingest_tombstone_refusal(
        &Error::Relay {
            status: 400,
            body: expiry.into()
        },
        &expired
    ));
    assert!(!pre_ingest_tombstone_refusal(
        &Error::Relay {
            status: 400,
            body: expiry.into()
        },
        &fresh
    ));
    for status in [401, 403, 429, 500, 503] {
        assert!(!pre_ingest_tombstone_refusal(
            &Error::Relay {
                status,
                body: expiry.into()
            },
            &expired
        ));
    }
    assert!(!pre_ingest_tombstone_refusal(
        &Error::Relay {
            status: 400,
            body: "invalid: unrelated request failure".into()
        },
        &expired
    ));
    assert!(!pre_ingest_tombstone_refusal(
        &Error::Unconfirmed(expiry.into()),
        &expired
    ));
}
