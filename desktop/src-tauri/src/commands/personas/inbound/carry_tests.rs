//! Inbound kind:30177 project-digest carry (review 2026-09-14, finding 3) and
//! withdrawal (review 2026-09-15): a same-owner event's digest is carried onto
//! a record that cannot decide the association itself, a same-owner marker
//! withdraws, and nothing else is changed.

use super::*;
use crate::managed_agents::agent_events::managed_agent_content_from_event;

const AGENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";
const PROJECT: &str =
    "30621:abababababababababababababababababababababababababababababababab:tank-loop";

fn record() -> ManagedAgentRecord {
    serde_json::from_value(serde_json::json!({
        "pubkey": AGENT,
        "name": "Builder",
        "relay_url": "wss://relay.example",
        "acp_command": "beekeeper-acp",
        "agent_command": "goose",
        "agent_args": [],
        "mcp_command": "",
        "turn_timeout_seconds": 320,
        "home_role": "builder",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "last_started_at": null,
        "last_stopped_at": null,
        "last_exit_code": null,
        "last_error": null
    }))
    .expect("record fixture")
}

fn digest() -> String {
    beekeeper_core_pkg::project_agent_association::project_agent_digest(PROJECT).expect("digest")
}

/// Parse inbound content through the real parser, as the apply path does.
fn inbound(content: serde_json::Value) -> ManagedAgentEventContent {
    use nostr::{EventBuilder, Keys, Kind, Tag};
    let event = EventBuilder::new(Kind::Custom(30177), content.to_string())
        .tags(vec![Tag::parse(["d", AGENT]).expect("d tag")])
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    managed_agent_content_from_event(&event).expect("parse")
}

/// Run the apply path's managed-agent steps in order: projected-field apply,
/// then the same-owner carry.
fn apply(agents: &mut [ManagedAgentRecord], content: serde_json::Value, same_owner: bool) -> bool {
    let content = inbound(content);
    let (digest, withdrawn) = (content.project_digest.clone(), content.project_withdrawn);
    apply_inbound_managed_agent(agents, AGENT, content);
    carry_same_owner_project_digest(agents, AGENT, same_owner, digest.as_deref(), withdrawn)
}

fn with_digest(digest: &str) -> serde_json::Value {
    serde_json::json!({
        "name": "Builder", "parallelism": 1, "respond_to": "owner-only",
        "home_role": "verifier", "project_digest": digest,
    })
}

fn without_digest() -> serde_json::Value {
    serde_json::json!({ "name": "Builder", "parallelism": 1, "respond_to": "owner-only" })
}

#[test]
fn same_owner_inbound_digest_is_carried_onto_an_unassociated_record() {
    let mut agents = vec![record()];
    assert!(apply(&mut agents, with_digest(&digest()), true));
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
    assert_eq!(agents[0].project_ref, None, "carry never sets project_ref");
    assert_eq!(agents[0].project_public, None);
    assert_eq!(
        agents[0].home_role.as_deref(),
        Some("builder"),
        "an inbound home_role never changes the local role"
    );
    // Persisted on the record, so the next retain publishes it.
    assert_eq!(
        crate::managed_agents::agent_events::agent_event_content(&agents[0]).project_digest,
        Some(digest())
    );
    assert!(
        !apply(&mut agents, with_digest(&digest()), true),
        "re-receiving the same digest changes nothing"
    );
}

#[test]
fn same_owner_inbound_digest_is_carried_onto_an_unverified_project() {
    let mut agents = vec![record()];
    agents[0].project_ref = Some(PROJECT.replace("tank-loop", "other"));
    assert!(apply(&mut agents, with_digest(&digest()), true));
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
    assert_eq!(
        agents[0].project_ref.as_deref(),
        Some(PROJECT.replace("tank-loop", "other").as_str()),
        "carry never changes project_ref"
    );
}

#[test]
fn digest_less_inbound_never_clears_a_carried_digest() {
    let mut agents = vec![record()];
    agents[0].carried_project_digest = Some(digest());
    assert!(!apply(&mut agents, without_digest(), true));
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
    assert!(!apply(&mut agents, with_digest("not-a-digest"), true));
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
}

#[test]
fn a_known_private_local_project_ignores_the_inbound_digest() {
    let mut agents = vec![record()];
    agents[0].project_ref = Some(PROJECT.to_string());
    agents[0].project_public = Some(false);
    assert!(!apply(&mut agents, with_digest(&digest()), true));
    assert_eq!(agents[0].carried_project_digest, None);
    assert_eq!(agents[0].project_ref.as_deref(), Some(PROJECT));
    assert_eq!(agents[0].project_public, Some(false));
}

#[test]
fn a_verified_public_local_project_keeps_its_own_answer() {
    let mut agents = vec![record()];
    agents[0].project_ref = Some(PROJECT.to_string());
    agents[0].project_public = Some(true);
    assert!(!apply(&mut agents, with_digest(&"e".repeat(64)), true));
    assert_eq!(agents[0].carried_project_digest, None);
}

#[test]
fn another_authors_digest_is_never_carried() {
    let mut agents = vec![record()];
    assert!(!apply(&mut agents, with_digest(&digest()), false));
    assert_eq!(agents[0].carried_project_digest, None);
}

#[test]
fn an_inbound_digest_for_an_agent_not_held_here_changes_nothing() {
    let mut agents = vec![record()];
    agents[0].pubkey = "5".repeat(64);
    assert!(!apply(&mut agents, with_digest(&digest()), true));
    assert_eq!(agents[0].carried_project_digest, None);
}

fn withdrawal() -> serde_json::Value {
    let mut value = without_digest();
    value[beekeeper_core_pkg::project_agent_association::PROJECT_AGENT_WITHDRAWN_CONTENT_KEY] =
        serde_json::json!(true);
    value
}

#[test]
fn same_owner_marker_withdraws_and_drops_the_carried_digest() {
    let mut agents = vec![record()];
    agents[0].carried_project_digest = Some(digest());
    assert!(apply(&mut agents, withdrawal(), true));
    assert!(agents[0].project_publication_withdrawn);
    assert_eq!(agents[0].carried_project_digest, None);
    let published = crate::managed_agents::agent_events::agent_event_content(&agents[0]);
    assert_eq!(published.project_digest, None, "never republished");
    assert!(published.project_withdrawn, "the marker is republished");
    assert!(
        !apply(&mut agents, with_digest(&digest()), true),
        "a later digest from a stale copy is not carried onto a withdrawn record"
    );
    assert_eq!(agents[0].carried_project_digest, None);
}

#[test]
fn digest_less_marker_less_inbound_never_clears_a_withdrawal() {
    let mut agents = vec![record()];
    agents[0].project_publication_withdrawn = true;
    assert!(!apply(&mut agents, without_digest(), true));
    assert!(agents[0].project_publication_withdrawn);
    let mut carrying = vec![record()];
    carrying[0].carried_project_digest = Some(digest());
    assert!(!apply(&mut carrying, without_digest(), true));
    assert_eq!(carrying[0].carried_project_digest, Some(digest()));
    assert!(!carrying[0].project_publication_withdrawn);
}

#[test]
fn another_authors_marker_is_never_honored() {
    let mut agents = vec![record()];
    agents[0].carried_project_digest = Some(digest());
    assert!(!apply(&mut agents, withdrawal(), false));
    assert!(!agents[0].project_publication_withdrawn);
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
}
