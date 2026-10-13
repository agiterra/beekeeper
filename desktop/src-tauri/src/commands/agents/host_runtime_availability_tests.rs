//! Availability projection exercises shared signature, authority and lease validation.
use super::*;
use beekeeper_agent_management::Error;
use beekeeper_core_pkg::{
    coding_session_command::CodingSessionTarget,
    coding_session_lease::{CodingSessionLease, CodingSessionLeaseState},
    coding_session_payload::{Capabilities, SessionMetadata, SessionStatus, METADATA_SCHEMA},
};
use nostr::{EventBuilder, Keys, Kind, Tag};

struct Relay {
    owner: Keys,
    relay: Keys,
    events: Vec<Event>,
    lease: Option<Event>,
    unreadable: bool,
}
impl RelayTransport for Relay {
    fn keys(&self) -> &Keys {
        &self.owner
    }
    fn relay_url(&self) -> &str {
        "ws://127.0.0.1:31339"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused scoped writes".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        Ok(self.relay.public_key().to_hex())
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        let kind = filter["kinds"][0].as_u64().expect("explicit kind");
        if self.unreadable && kind == 24223 {
            return Err(Error::Other("isolated lease query unavailable".into()));
        }
        Ok(self
            .events
            .iter()
            .chain(self.lease.iter())
            .filter(|event| {
                event.kind.as_u16() as u64 == kind
                    && filter
                        .get("authors")
                        .is_none_or(|authors| authors[0] == event.pubkey.to_hex())
                    && ["h", "p", "d"].iter().all(|tag| {
                        filter.get(format!("#{tag}")).is_none_or(|values| {
                            event.tags.iter().any(|actual| {
                                actual.as_slice().first().is_some_and(|key| key == tag)
                                    && actual
                                        .as_slice()
                                        .get(1)
                                        .is_some_and(|value| values[0] == *value)
                            })
                        })
                    })
            })
            .map(|event| serde_json::to_value(event).expect("event"))
            .collect())
    }
    async fn submit_event(&self, _: Event) -> Result<Value, Error> {
        panic!("availability observation never publishes")
    }
}
fn profile(owner: &Keys, key: &Keys) -> Event {
    let auth =
        beekeeper_sdk_pkg::nip_oa::compute_auth_tag(owner, &key.public_key(), "").expect("auth");
    EventBuilder::new(Kind::Metadata, "{}")
        .tags([beekeeper_sdk_pkg::nip_oa::parse_auth_tag(&auth).expect("auth tag")])
        .sign_with_keys(key)
        .expect("profile")
}
fn fixture() -> (Relay, Keys, Keys, Uuid, CodingSessionTarget) {
    let owner = Keys::generate();
    let actor = Keys::generate();
    let provider = Keys::generate();
    let relay = Keys::generate();
    let channel = Uuid::new_v4();
    let target = CodingSessionTarget {
        driver: "claude-acp".into(),
        instance_id: "primary".into(),
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
    let membership = EventBuilder::new(Kind::Custom(39002), "")
        .tags([
            Tag::parse(["d", &channel.to_string()]).expect("d"),
            Tag::parse(["p", &actor.public_key().to_hex()]).expect("actor"),
        ])
        .sign_with_keys(&relay)
        .expect("relay membership");
    let observed = beekeeper_sdk_pkg::build_coding_session_metadata(
        channel,
        &target,
        &serde_json::to_string(&metadata).expect("metadata"),
    )
    .expect("builder")
    .sign_with_keys(&provider)
    .expect("provider metadata");
    let client = Relay {
        events: vec![
            profile(&owner, &actor),
            profile(&owner, &provider),
            membership,
            observed,
        ],
        owner,
        relay,
        lease: None,
        unreadable: false,
    };
    (client, actor, provider, channel, target)
}
fn lease(provider: &Keys, channel: Uuid, target: &CodingSessionTarget, age: u64) -> Event {
    let payload =
        CodingSessionLease::new(target.clone(), CodingSessionLeaseState::Live, 1).expect("lease");
    let builder =
        beekeeper_sdk_pkg::build_coding_session_lease(channel, "original-create", &payload)
            .expect("lease builder");
    // Preserve the actual protocol builder's ordered tags and signed payload.
    let current = builder.sign_with_keys(provider).expect("current lease");
    EventBuilder::new(current.kind, current.content)
        .tags(current.tags)
        .custom_created_at(nostr::Timestamp::from(
            nostr::Timestamp::now().as_secs().saturating_sub(age),
        ))
        .sign_with_keys(provider)
        .expect("signed lease")
}
#[tokio::test]
async fn signed_lease_projection_distinguishes_live_expired_missing_wrong_scope_and_unreadable() {
    let (mut client, actor, provider, channel, target) = fixture();
    let actor = actor.public_key().to_hex();
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false),
        "idle signed metadata does not imply availability"
    );
    client.lease = Some(lease(&provider, channel, &target, 0));
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(true)
    );
    client.lease = Some(lease(&provider, channel, &target, 181));
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false)
    );
    client.lease = Some(lease(&provider, Uuid::new_v4(), &target, 0));
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false)
    );
    let mut other = target.clone();
    other.generation += 1;
    client.lease = Some(lease(&provider, channel, &other, 0));
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false)
    );
    let mut invalid = lease(&provider, channel, &target, 0);
    invalid.content.push(' '); // Signature no longer covers the serialized event.
    client.lease = Some(invalid);
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false)
    );
    client.lease = Some(lease(&Keys::generate(), channel, &target, 0));
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        Some(false)
    );
    client.lease = Some(lease(&provider, channel, &target, 0));
    client.unreadable = true;
    assert_eq!(
        availability_from_transport(&actor, &client).await,
        None,
        "unreadable evidence differs from observed absence"
    );
}
