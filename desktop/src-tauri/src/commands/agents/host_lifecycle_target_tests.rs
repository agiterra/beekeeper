//! Targeted configuration must not inspect unrelated retained agents or guess target inactivity.
use super::*;
use beekeeper_agent_management::{Error, RelayTransport};
use nostr::{Event, EventBuilder, Keys, Kind, Tag};
use std::sync::Mutex;

struct Relay {
    owner: Keys,
    relay: Keys,
    actor: String,
    actor_keys: Keys,
    membership_visible: bool,
    queried_actors: Mutex<Vec<String>>,
}
impl RelayTransport for Relay {
    fn keys(&self) -> &Keys {
        &self.owner
    }
    fn relay_url(&self) -> &str {
        "ws://localhost:3037"
    }
    fn verified_auth_tag_owner_hex(&self) -> Result<Option<String>, Error> {
        Ok(None)
    }
    fn scoped(&self, _: Keys, _: String) -> Result<Self, Error> {
        Err(Error::Other("unused".into()))
    }
    async fn relay_identity(&self) -> Result<String, Error> {
        Ok(self.relay.public_key().to_hex())
    }
    async fn query_all(&self, filter: Value) -> Result<Vec<Value>, Error> {
        if filter["kinds"][0] == 39002 {
            let actor = filter["#p"][0].as_str().expect("actor filter");
            self.queried_actors
                .lock()
                .expect("queries")
                .push(actor.into());
            if actor != self.actor {
                return Err(Error::Unconfirmed(
                    "unrelated retained actor inventory unavailable".into(),
                ));
            }
            if !self.membership_visible {
                return Ok(vec![]);
            }
            let event = EventBuilder::new(Kind::Custom(39002), "")
                .tags([
                    Tag::parse(["d", "6fe303b4-e11a-471e-9a27-c040549ffa70"]).expect("channel"),
                    Tag::parse(["p", actor, "", "member"]).expect("member"),
                ])
                .sign_with_keys(&self.relay)
                .expect("membership");
            return Ok(vec![serde_json::to_value(event).expect("event")]);
        }
        if filter["kinds"][0] == 0 {
            assert_eq!(filter["authors"][0], self.actor);
            let auth = beekeeper_sdk_pkg::nip_oa::compute_auth_tag(
                &self.owner,
                &self.actor_keys.public_key(),
                "",
            )
            .expect("actor attestation");
            let values: Vec<String> = serde_json::from_str(&auth).expect("auth tag");
            let event = EventBuilder::new(Kind::Metadata, "{}")
                .tags([Tag::parse(values).expect("auth")])
                .sign_with_keys(&self.actor_keys)
                .expect("profile");
            return Ok(vec![serde_json::to_value(event).expect("profile event")]);
        }
        Ok(vec![])
    }
    async fn submit_event(&self, _: Event) -> Result<Value, Error> {
        panic!("status must not write")
    }
}
fn fixture(visible: bool) -> Relay {
    let actor_keys = Keys::generate();
    Relay {
        owner: Keys::generate(),
        relay: Keys::generate(),
        actor: actor_keys.public_key().to_hex(),
        actor_keys,
        membership_visible: visible,
        queried_actors: Mutex::new(vec![]),
    }
}
#[tokio::test]
async fn edited_actor_status_does_not_scan_unrelated_unadmitted_agents() {
    let relay = fixture(true);
    assert!(!actor_is_active(&relay.actor, &relay)
        .await
        .expect("target membership is visible"));
    assert_eq!(
        *relay.queried_actors.lock().expect("queries"),
        vec![relay.actor.clone()]
    );
    let unrelated = Keys::generate().public_key().to_hex();
    assert!(actor_is_active(&unrelated, &relay).await.is_err());
}
#[tokio::test]
async fn edited_actor_missing_inventory_remains_unconfirmed() {
    let relay = fixture(false);
    let error = actor_is_active(&relay.actor, &relay)
        .await
        .expect_err("absence is not inactive");
    assert!(error.contains("zero work cannot be certified"));
}
#[test]
fn one_refused_agent_is_isolated_from_the_rest_of_the_roster() {
    let folded = fold_host_agent_availability([
        ("a".repeat(64), "Active".to_string(), Ok(true)),
        ("b".repeat(64), "Idle".to_string(), Ok(false)),
        (
            "c".repeat(64),
            "Scout".to_string(),
            Err("actor channel inventory unavailable; zero work cannot be certified".to_string()),
        ),
        ("d".repeat(64), "Also active".to_string(), Ok(true)),
    ]);
    assert_eq!(
        folded.active,
        HashSet::from(["a".repeat(64), "d".repeat(64)]),
        "agents after the refusal are still observed"
    );
    assert_eq!(
        folded.unobserved,
        vec![UnobservedHostAgent {
            pubkey: "c".repeat(64),
            name: "Scout".into(),
            reason: "actor channel inventory unavailable; zero work cannot be certified".into(),
        }]
    );
    assert_eq!(
        fold_host_agent_availability([]),
        HostAgentAvailability::default()
    );
}

/// A Start that never finishes ends with a truthful, retryable outcome instead
/// of "Starting…" forever (ledger 403); a finished Start passes through.
#[tokio::test]
async fn a_start_that_never_finishes_ends_with_a_truthful_retryable_outcome() {
    let stuck = bounded_start(
        std::time::Duration::from_millis(20),
        std::future::pending::<Result<(), String>>(),
    )
    .await
    .unwrap_err();
    assert!(stuck.contains("outcome is unconfirmed"), "{stuck}");
    assert!(stuck.contains("resume the same operation"), "{stuck}");
    assert_eq!(
        bounded_start(std::time::Duration::from_secs(5), async {
            Err("refused".to_string())
        })
        .await,
        Err("refused".to_string())
    );
    assert!(
        bounded_start(std::time::Duration::from_secs(5), async { Ok(()) })
            .await
            .is_ok()
    );
}
