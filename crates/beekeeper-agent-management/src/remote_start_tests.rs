use super::*;
use nostr::{EventBuilder, Keys, Kind, Tag};
use std::sync::Mutex;
struct Relay {
    owner: Keys,
    provider: Keys,
    actor: Keys,
    definition: Event,
    events: Mutex<Vec<Event>>,
    members: Mutex<std::collections::BTreeMap<Uuid, Vec<String>>>,
    reject_membership: bool,
}
fn profile(owner: &Keys, key: &Keys) -> Event {
    let auth = beekeeper_sdk::nip_oa::compute_auth_tag(owner, &key.public_key(), "").expect("auth");
    EventBuilder::new(Kind::Metadata, "{}")
        .tags([
            Tag::parse(serde_json::from_str::<Vec<String>>(&auth).expect("auth values"))
                .expect("tag"),
        ])
        .sign_with_keys(key)
        .expect("profile")
}
impl RelayTransport for Relay {
    fn keys(&self) -> &Keys {
        &self.owner
    }
    fn relay_url(&self) -> &str {
        "ws://127.0.0.1:31337"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("no actor transport required".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        Ok(self.provider.public_key().to_hex())
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        let rows = match filter["kinds"][0].as_u64().expect("explicit kind") {
            30177 => vec![self.definition.clone()],
            39002 => self
                .members
                .lock()
                .expect("members")
                .iter()
                .filter(|(channel, members)| {
                    filter.get("#d").is_none_or(|d| d[0] == channel.to_string())
                        && filter
                            .get("#p")
                            .is_none_or(|p| members.iter().any(|key| p[0] == *key))
                })
                .map(|(channel, members)| {
                    let mut tags = vec![Tag::parse(["d", &channel.to_string()]).expect("d")];
                    tags.extend(members.iter().map(|key| Tag::parse(["p", key]).expect("p")));
                    EventBuilder::new(Kind::Custom(39002), "")
                        .allow_self_tagging()
                        .tags(tags)
                        .sign_with_keys(&self.provider)
                        .expect("relay membership")
                })
                .collect(),
            0 => vec![profile(
                &self.owner,
                if filter["authors"][0] == self.actor.public_key().to_hex() {
                    &self.actor
                } else {
                    &self.provider
                },
            )],
            9000 | 44221 | 44223 | 44224 => self
                .events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| {
                    event.kind.as_u16() as u64 == filter["kinds"][0].as_u64().expect("kind")
                        && filter
                            .get("ids")
                            .is_none_or(|ids| ids[0] == event.id.to_hex())
                        && filter.get("#h").is_none_or(|h| {
                            event.tags.iter().any(|tag| {
                                tag.as_slice().first().is_some_and(|key| key == "h")
                                    && tag.as_slice().get(1).is_some_and(|value| h[0] == *value)
                            })
                        })
                })
                .cloned()
                .collect(),
            _ => vec![],
        };
        Ok(rows
            .into_iter()
            .map(|event| serde_json::to_value(event).expect("event"))
            .collect())
    }
    async fn submit_event(&self, event: Event) -> Result<Value, Error> {
        if event.kind.as_u16() == 9000 {
            if self.reject_membership {
                return Err(Error::Refused(
                    "caller lacks channel add-member authority".into(),
                ));
            }
            let channel = event
                .tags
                .iter()
                .find_map(|tag| {
                    (tag.as_slice().first().is_some_and(|key| key == "h"))
                        .then(|| {
                            tag.as_slice()
                                .get(1)
                                .and_then(|value| Uuid::parse_str(value).ok())
                        })
                        .flatten()
                })
                .expect("channel");
            let participant = event
                .tags
                .iter()
                .find_map(|tag| {
                    (tag.as_slice().first().is_some_and(|key| key == "p"))
                        .then(|| tag.as_slice().get(1).cloned())
                        .flatten()
                })
                .expect("member");
            let mut members = self.members.lock().expect("members");
            let keys = members.get_mut(&channel).expect("channel");
            if !keys.contains(&participant) {
                keys.push(participant);
            }
            self.events.lock().expect("events").push(event);
            return Ok(json!({"accepted":true}));
        }
        assert_eq!(event.kind.as_u16(), 44221);
        assert_eq!(event.pubkey, self.owner.public_key());
        let payload=beekeeper_core::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&event.content).expect("payload");
        let target = beekeeper_core::coding_session_command::CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "fixture".into(),
            session_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        let key = beekeeper_sdk::coding_session::coding_session_lifecycle_receipt_semantic_key(
            &payload.command_id,
        );
        let receipt = EventBuilder::new(
            Kind::Custom(44224),
            serde_json::to_string(
                &beekeeper_core::coding_session_payload::LifecycleReceipt::created(
                    &payload.command_id,
                    &target,
                ),
            )
            .expect("receipt"),
        )
        .tags([
            Tag::parse([
                "h",
                event
                    .tags
                    .iter()
                    .find(|tag| tag.as_slice()[0] == "h")
                    .expect("h")
                    .as_slice()[1]
                    .as_str(),
            ])
            .expect("h"),
            Tag::parse([
                "cslr-v",
                beekeeper_sdk::coding_session::CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
            ])
            .expect("version"),
            Tag::parse(["csl-command", &payload.command_id]).expect("command"),
            Tag::parse(["csl-key", &key]).expect("key"),
        ])
        .sign_with_keys(&self.provider)
        .expect("signed receipt");
        self.events.lock().expect("events").extend([event, receipt]);
        Ok(json!({"accepted":true,"message":"stored"}))
    }
}
#[tokio::test]
async fn owner_remote_start_uses_only_caller_journal_and_exact_signed_retry() {
    let owner = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let content = beekeeper_core::managed_agent::ManagedAgentEventContent {
        name: "Remote".into(),
        system_prompt: Some("ordinary work".into()),
        model: Some("default".into()),
        persona_id: None,
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
    .expect("definition")
    .sign_with_keys(&owner)
    .expect("signed head");
    let args = RemoteStartRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: actor.public_key().to_hex(),
        expected_revision: definition.id.to_hex(),
        channel: Uuid::new_v4(),
        provider_authority: provider.public_key().to_hex(),
        provider_instance: "codex-primary".into(),
        project: None,
        wait_secs: 1,
    };
    let members = vec![
        owner.public_key().to_hex(),
        actor.public_key().to_hex(),
        provider.public_key().to_hex(),
    ];
    let client = Relay {
        owner,
        provider,
        actor,
        definition,
        events: Mutex::new(vec![]),
        members: Mutex::new([(args.channel, members)].into_iter().collect()),
        reject_membership: false,
    };
    let home = tempfile::tempdir().expect("caller home");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    assert_eq!(
        channel_members(&client, args.channel)
            .await
            .expect("members")
            .len(),
        3
    );
    let first = start_remote(&args, &client, &context)
        .await
        .expect("remote accepted");
    assert_eq!(first["host_admitted"], true);
    assert!(!home.path().join("host/config.json").exists());
    assert!(!home.path().join("actors").exists());
    assert_eq!(
        start_remote(&args, &client, &context).await.expect("retry"),
        first
    );
    assert_eq!(
        client
            .events
            .lock()
            .expect("events")
            .iter()
            .filter(|event| event.kind.as_u16() == 44221)
            .count(),
        1
    );
    let mut changed = args.clone();
    changed.provider_instance = "different".into();
    assert!(matches!(
        start_remote(&changed, &client, &context).await,
        Err(Error::Conflict(_))
    ));
}

fn additional_fixture() -> (Relay, RemoteStartRequest, Uuid) {
    let owner = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let definition=EventBuilder::new(Kind::Custom(30177),json!({"name":"Conversation actor","system_prompt":"ordinary work","parallelism":1,"respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d",&actor.public_key().to_hex()]).expect("d")]).sign_with_keys(&owner).expect("head");
    let channel = Uuid::new_v4();
    let primary = Uuid::new_v4();
    let members = vec![
        owner.public_key().to_hex(),
        actor.public_key().to_hex(),
        provider.public_key().to_hex(),
    ];
    let args = RemoteStartRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: actor.public_key().to_hex(),
        expected_revision: definition.id.to_hex(),
        channel,
        provider_authority: provider.public_key().to_hex(),
        provider_instance: "codex-primary".into(),
        project: None,
        wait_secs: 1,
    };
    let client = Relay {
        owner,
        actor,
        provider,
        definition,
        events: Mutex::new(vec![]),
        members: Mutex::new(
            [
                (primary, members),
                (channel, vec![args.agent_pubkey.clone()]),
            ]
            .into_iter()
            .collect(),
        ),
        reject_membership: false,
    };
    client
        .members
        .lock()
        .expect("members")
        .insert(channel, vec![client.owner.public_key().to_hex()]);
    (client, args, primary)
}
fn metadata(
    client: &Relay,
    channel: Uuid,
    target: beekeeper_core::coding_session_command::CodingSessionTarget,
    status: beekeeper_core::coding_session_payload::SessionStatus,
) -> Event {
    use beekeeper_core::coding_session_payload::{Capabilities, SessionMetadata, METADATA_SCHEMA};
    let content = SessionMetadata {
        schema: METADATA_SCHEMA.into(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: Some(client.actor.public_key().to_hex()),
        role: None,
        provider: Some("codex-primary".try_into().expect("alias")),
        runtime: Some("codex".try_into().expect("runtime")),
        model: None,
        status,
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
    beekeeper_sdk::build_coding_session_metadata(
        channel,
        &target,
        &serde_json::to_string(&content).expect("metadata"),
    )
    .expect("builder")
    .sign_with_keys(&client.provider)
    .expect("signature")
}
fn target() -> beekeeper_core::coding_session_command::CodingSessionTarget {
    beekeeper_core::coding_session_command::CodingSessionTarget {
        driver: "codex-acp".into(),
        instance_id: "fixture".into(),
        session_id: Uuid::new_v4().to_string(),
        generation: 1,
    }
}
#[tokio::test]
async fn active_primary_does_not_skip_explicit_joined_stream_and_membership_retries_are_exact() {
    let (client, args, primary) = additional_fixture();
    let primary_target = target();
    client.events.lock().expect("events").push(metadata(
        &client,
        primary,
        primary_target.clone(),
        beekeeper_core::coding_session_payload::SessionStatus::Idle,
    ));
    let home = tempfile::tempdir().expect("caller");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    let result = start_remote(&args, &client, &context)
        .await
        .expect("additional stream accepted");
    assert_eq!(result["host_admitted"], true);
    let events = client.events.lock().expect("events").clone();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind.as_u16() == 9000)
            .count(),
        2
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| event.kind.as_u16() == 44221)
            .count(),
        1
    );
    assert!(events
        .iter()
        .find(|event| event.kind.as_u16() == 44221)
        .expect("create")
        .tags
        .iter()
        .any(|tag| tag.as_slice() == ["h", &args.channel.to_string()]));
    assert_eq!(
        start_remote(&args, &client, &context)
            .await
            .expect("exact retry"),
        result
    );
    assert_eq!(client.events.lock().expect("events").len(), events.len());
    assert!(client
        .events
        .lock()
        .expect("events")
        .iter()
        .any(|event| event.kind.as_u16() == 44223
            && event.content.contains(&primary_target.session_id)));
}
#[tokio::test]
async fn unjoined_caller_and_refused_membership_never_publish_create() {
    let (mut client, args, _) = additional_fixture();
    let home = tempfile::tempdir().expect("caller");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    client
        .members
        .lock()
        .expect("members")
        .insert(args.channel, vec![client.actor.public_key().to_hex()]);
    assert!(matches!(
        start_remote(&args, &client, &context).await,
        Err(Error::Refused(_))
    ));
    assert!(client.events.lock().expect("events").is_empty());
    client
        .members
        .lock()
        .expect("members")
        .insert(args.channel, vec![client.owner.public_key().to_hex()]);
    client.reject_membership = true;
    assert!(matches!(
        start_remote(&args, &client, &context).await,
        Err(Error::Refused(_))
    ));
    let operation = local::operation_path(&context, args.operation_id)
        .expect("operation")
        .with_file_name(format!("remote-start-{}.json", args.operation_id));
    let held: Operation =
        serde_json::from_slice(&std::fs::read(&operation).expect("retained operation"))
            .expect("JSON");
    assert_eq!(held.memberships.len(), 2);
    client.reject_membership = false;
    start_remote(&args, &client, &context)
        .await
        .expect("same operation after authorization repair");
    let events = client.events.lock().expect("events");
    for member in held.memberships {
        assert!(events.iter().any(|event| event == &member));
    }
    assert!(events.iter().any(|event| event == &held.event));
}
#[tokio::test]
async fn observed_active_target_survives_settings_revision_without_create_or_cas_mutation() {
    let (mut client, args, _) = additional_fixture();
    client.members.lock().expect("members").insert(
        args.channel,
        vec![
            client.owner.public_key().to_hex(),
            client.actor.public_key().to_hex(),
            client.provider.public_key().to_hex(),
        ],
    );
    let held_target = target();
    client.events.lock().expect("events").push(metadata(
        &client,
        args.channel,
        held_target.clone(),
        beekeeper_core::coding_session_payload::SessionStatus::Idle,
    ));
    client.definition=EventBuilder::new(Kind::Custom(30177),json!({"name":"Changed settings","system_prompt":"new instructions","parallelism":1,"respond_to":"owner-only"}).to_string())
        .tags([Tag::parse(["d",&client.actor.public_key().to_hex()]).expect("d")]).sign_with_keys(&client.owner).expect("new owner head");
    assert_ne!(client.definition.id.to_hex(), args.expected_revision);
    let before = client.events.lock().expect("events").clone();
    let prior = json!({"target":held_target,"host_admitted":true});
    let observed = observe_remote_start(&args, &prior, &client)
        .await
        .expect("read-only observation")
        .expect("exact active responsibility");
    assert_eq!(observed["execution_created"], false);
    assert_eq!(observed["configuration_application"], "unconfirmed");
    assert_eq!(client.events.lock().expect("events").len(), before.len());
    let mut other = args.clone();
    other.provider_instance = "different-runtime".into();
    assert!(matches!(
        observe_remote_start(&other, &prior, &client).await,
        Err(Error::Conflict(_))
    ));
    let unknown = json!({"target":target()});
    assert!(observe_remote_start(&args, &unknown, &client)
        .await
        .expect("unknown exact target")
        .is_none());
    assert_eq!(client.events.lock().expect("events").len(), before.len());
}

#[tokio::test]
async fn uncertain_other_channel_blocks_additional_create_and_membership_effects() {
    let (client, args, primary) = additional_fixture();
    client.events.lock().expect("events").push(metadata(
        &client,
        primary,
        target(),
        beekeeper_core::coding_session_payload::SessionStatus::Disconnected,
    ));
    let home = tempfile::tempdir().expect("caller");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    assert!(matches!(
        start_remote(&args, &client, &context).await,
        Err(Error::Unconfirmed(_))
    ));
    assert_eq!(client.events.lock().expect("events").len(), 1);
    assert!(!local::operation_path(&context, args.operation_id)
        .expect("operation")
        .with_file_name(format!("remote-start-{}.json", args.operation_id))
        .exists());
}

#[tokio::test]
async fn active_requested_channel_does_not_advertise_other_runtime_or_project() {
    let (client, mut args, _) = additional_fixture();
    client.members.lock().expect("members").insert(
        args.channel,
        vec![
            client.owner.public_key().to_hex(),
            client.actor.public_key().to_hex(),
            client.provider.public_key().to_hex(),
        ],
    );
    client.events.lock().expect("events").push(metadata(
        &client,
        args.channel,
        target(),
        beekeeper_core::coding_session_payload::SessionStatus::Idle,
    ));
    let home = tempfile::tempdir().expect("caller");
    let context = Context {
        home: home.path().into(),
        instance: beekeeper_host_core::layout::Instance::Dev,
        native_home: None,
    };
    args.provider_instance = "different-runtime".into();
    assert!(matches!(
        start_remote(&args, &client, &context).await,
        Err(Error::Conflict(_))
    ));
    assert_eq!(client.events.lock().expect("events").len(), 1);
    args.provider_instance = "codex-primary".into();
    let observed = start_remote(&args, &client, &context)
        .await
        .expect("exact active contract");
    assert_eq!(observed["execution_created"], false);
    assert_eq!(
        observed["runtime_available"], false,
        "metadata alone never claims runtime availability"
    );
    assert_eq!(observed["pending"], true);
    assert_eq!(client.events.lock().expect("events").len(), 1);
}
