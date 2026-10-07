//! `bee sessions whoami` — who this signer is, per the relay.
//!
//! Prints one JSON object: the signer's pubkey, its relay display name (or
//! `null`), the relay URL, the relay's own disclosed build commit (or
//! `"unknown"` — finding 32, `review-2026-09-01/LIVE-RUN-TeamRolesV1.md`),
//! and the role slug of any active team seat it holds (or `null`). See
//! `crates/beekeeper-cli/TESTING.md` for the runbook entry.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use beekeeper_core::kind::KIND_CODING_SESSION_GENESIS;

use super::operations::fetch_projected_authority;
use crate::client::{extract_d_tag, BeekeeperClient};
use crate::error::CliError;

/// `bee sessions whoami` — print the signer's identity as one JSON object.
///
/// `--format compact` and `--format json` print the same five keys: this
/// command's whole output is already the minimal shape `compact` reduces
/// other reads to, so there is nothing left for it to drop.
pub async fn cmd_whoami(
    client: &BeekeeperClient,
    _format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let pubkey = client.keys().public_key().to_hex();
    let display_name = fetch_display_name(client, &pubkey).await?;
    let role = resolve_active_role(client, &pubkey).await?;
    let relay_commit = crate::commands::git_setup::serving_relay_commit(client.relay_url()).await;

    println!(
        "{}",
        whoami_json(
            &pubkey,
            display_name.as_deref(),
            client.relay_url(),
            &relay_commit,
            role.as_deref(),
        )
    );
    Ok(())
}

/// Build the wire object. A pure function so the "always four keys, `null`
/// rather than omitted" contract is tested on the serialized text, not on a
/// struct that could drift from what `serde_json::json!` actually emits.
fn whoami_json(
    pubkey: &str,
    display_name: Option<&str>,
    relay_url: &str,
    relay_commit: &str,
    role: Option<&str>,
) -> Value {
    json!({
        "pubkey": pubkey,
        "display_name": display_name,
        "relay_url": relay_url,
        "relay_commit": relay_commit,
        "role": role,
    })
}

/// Fetch this pubkey's kind:0 display name.
///
/// `Ok(None)` is the claim "the relay holds no name for this pubkey" — a
/// query that fails propagates as `Err` via `?` before it ever reaches this
/// return, so it can never be misread as `None`.
async fn fetch_display_name(
    client: &BeekeeperClient,
    pubkey: &str,
) -> Result<Option<String>, CliError> {
    let filter = json!({ "kinds": [0], "authors": [pubkey], "limit": 1 });
    let raw = client.query(&filter).await?;
    let events: Vec<Value> = serde_json::from_str(&raw)
        .map_err(|error| CliError::Other(format!("failed to parse profile query: {error}")))?;
    Ok(parse_display_name(&events))
}

/// Pull `display_name` (falling back to `name`) out of the newest kind:0
/// profile event, if any.
fn parse_display_name(events: &[Value]) -> Option<String> {
    let content = events.first()?.get("content")?.as_str()?;
    let profile: Value = serde_json::from_str(content).ok()?;
    profile
        .get("display_name")
        .or_else(|| profile.get("name"))?
        .as_str()
        .map(str::to_owned)
}

/// The role slug of the active team seat(s) this identity holds, discovered
/// from the relay by identity alone.
///
/// There is no channel or session ref in a seat's process environment — the
/// ACP harness injects only `BEEKEEPER_RELAY_URL`, `BEEKEEPER_PRIVATE_KEY`,
/// `BEEKEEPER_AUTH_TAG`, and optionally `BEEKEEPER_ACP_DISPLAY_NAME`
/// (`crates/beekeeper-acp/src/lib.rs:5174-5209`; confirmed empirically against
/// this lane's own process env, which carried none of the four) — so this
/// walks outward from the identity instead of starting from a channel it
/// does not have: every channel this pubkey is a NIP-29 member of
/// (kind:39002, `#p`-gated per `crates/beekeeper-relay/src/api/bridge.rs:1910`),
/// every 44226 genesis in each such channel, and that genesis's projected,
/// receipt-backed seat roster (`fetch_projected_authority`,
/// `crates/beekeeper-cli/src/commands/sessions/operations_authority.rs:64`). A
/// genesis whose chain does not project cleanly reads as "no evidence of a
/// seat here", the same way an unresolvable founder already reads as
/// unknown elsewhere in this module tree, rather than failing the whole
/// command over a record this identity may have nothing to do with.
async fn resolve_active_role(
    client: &BeekeeperClient,
    pubkey: &str,
) -> Result<Option<String>, CliError> {
    let membership = client
        .query_all(json!({ "kinds": [39002], "#p": [pubkey] }))
        .await?;
    let channel_ids = membership_channel_ids(&membership);

    let mut roles: BTreeSet<String> = BTreeSet::new();
    for channel_id in &channel_ids {
        let genesis_events = client
            .query_all(json!({ "kinds": [KIND_CODING_SESSION_GENESIS], "#h": [channel_id] }))
            .await?;
        for (genesis_id, founder) in genesis_founder_pairs(&genesis_events) {
            let Ok(authority) =
                fetch_projected_authority(client, channel_id, &genesis_id, &founder).await
            else {
                continue;
            };
            roles.extend(
                authority
                    .seats
                    .iter()
                    .filter(|seat| seat.actor_pubkey == pubkey)
                    .map(|seat| seat.role.clone()),
            );
        }
    }
    finish_role(roles)
}

/// Every distinct, non-empty `d`-tag channel id a batch of kind:39002
/// membership events names.
fn membership_channel_ids(events: &[Value]) -> Vec<String> {
    let mut ids: Vec<String> = events
        .iter()
        .map(extract_d_tag)
        .filter(|id| !id.is_empty())
        .collect();
    ids.sort();
    ids.dedup();
    ids
}

/// `(event id, signer pubkey)` for every 44226 genesis in a batch — the
/// signer is the founder by construction (a genesis names no other founder
/// field; whoever signs it founds it).
fn genesis_founder_pairs(events: &[Value]) -> Vec<(String, String)> {
    events
        .iter()
        .filter_map(|event| {
            let id = event.get("id")?.as_str()?.to_owned();
            let founder = event.get("pubkey")?.as_str()?.to_owned();
            Some((id, founder))
        })
        .collect()
}

/// Collapse the distinct role slugs found across every active seat into the
/// wire's `role` field: none held, one held, or — never silently picked — a
/// hard refusal naming every slug found when two disagree.
fn finish_role(roles: BTreeSet<String>) -> Result<Option<String>, CliError> {
    match roles.len() {
        0 => Ok(None),
        1 => Ok(roles.into_iter().next()),
        _ => Err(CliError::Other(format!(
            "signer holds conflicting active seat roles: {}",
            roles.into_iter().collect::<Vec<_>>().join(", ")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::exit_code;

    #[test]
    fn whoami_json_always_has_five_keys_with_null_for_missing() {
        let value = whoami_json("abc", None, "wss://relay", "unknown", None);
        let text = serde_json::to_string(&value).unwrap();
        assert_eq!(
            value.as_object().map(|obj| obj.len()),
            Some(5),
            "expected exactly 5 keys, got {text}"
        );
        assert!(text.contains(r#""display_name":null"#), "{text}");
        assert!(text.contains(r#""role":null"#), "{text}");
        assert!(text.contains(r#""pubkey":"abc""#), "{text}");
        assert!(text.contains(r#""relay_url":"wss://relay""#), "{text}");
        assert!(text.contains(r#""relay_commit":"unknown""#), "{text}");
    }

    #[test]
    fn whoami_json_carries_display_name_role_and_relay_commit_when_present() {
        let value = whoami_json(
            "abc",
            Some("Honey"),
            "wss://relay",
            "42dd921d831c483e6e16111491b39947b4cf1f86",
            Some("builder"),
        );
        let text = serde_json::to_string(&value).unwrap();
        assert!(text.contains(r#""display_name":"Honey""#), "{text}");
        assert!(text.contains(r#""role":"builder""#), "{text}");
        assert!(
            text.contains(r#""relay_commit":"42dd921d831c483e6e16111491b39947b4cf1f86""#),
            "{text}"
        );
    }

    #[test]
    fn parse_display_name_is_none_when_relay_holds_no_profile() {
        assert_eq!(parse_display_name(&[]), None);
    }

    #[test]
    fn parse_display_name_reads_display_name_field() {
        let events = vec![json!({ "content": "{\"display_name\":\"Honey\"}" })];
        assert_eq!(parse_display_name(&events), Some("Honey".to_string()));
    }

    #[test]
    fn parse_display_name_falls_back_to_name_field() {
        let events = vec![json!({ "content": "{\"name\":\"Honeybee\"}" })];
        assert_eq!(parse_display_name(&events), Some("Honeybee".to_string()));
    }

    #[test]
    fn parse_display_name_ignores_malformed_content() {
        let events = vec![json!({ "content": "not json" })];
        assert_eq!(parse_display_name(&events), None);
    }

    #[test]
    fn finish_role_is_none_with_no_active_seat() {
        assert_eq!(finish_role(BTreeSet::new()).unwrap(), None);
    }

    #[test]
    fn finish_role_returns_the_one_slug_when_seats_agree() {
        let mut roles = BTreeSet::new();
        roles.insert("builder".to_string());
        assert_eq!(finish_role(roles).unwrap(), Some("builder".to_string()));
    }

    #[test]
    fn finish_role_refuses_to_pick_between_disagreeing_slugs() {
        let mut roles = BTreeSet::new();
        roles.insert("builder".to_string());
        roles.insert("lead".to_string());
        let error = finish_role(roles).unwrap_err();
        assert_eq!(exit_code(&error), 4);
        let message = error.to_string();
        assert!(message.contains("builder"), "{message}");
        assert!(message.contains("lead"), "{message}");
    }

    #[test]
    fn membership_channel_ids_dedupes_and_drops_blank_d_tags() {
        let events = vec![
            json!({ "tags": [["d", "chan-1"]] }),
            json!({ "tags": [["d", "chan-1"]] }),
            json!({ "tags": [["d", "chan-2"]] }),
            json!({ "tags": [["d", ""]] }),
            json!({ "tags": [] }),
        ];
        assert_eq!(
            membership_channel_ids(&events),
            vec!["chan-1".to_string(), "chan-2".to_string()]
        );
    }

    #[test]
    fn genesis_founder_pairs_reads_id_and_signer() {
        let events = vec![json!({ "id": "g1", "pubkey": "founder1" })];
        assert_eq!(
            genesis_founder_pairs(&events),
            vec![("g1".to_string(), "founder1".to_string())]
        );
    }

    #[test]
    fn genesis_founder_pairs_skips_events_missing_id_or_pubkey() {
        let events = vec![json!({ "pubkey": "founder1" }), json!({ "id": "g1" })];
        assert!(genesis_founder_pairs(&events).is_empty());
    }

    /// `fetch_display_name`/`resolve_active_role` propagate `client.query*`
    /// failures with `?` before any JSON is built — a compile-time guarantee
    /// from `?`'s early-return, not a runtime branch this crate's `BeekeeperClient`
    /// can be made to take without an HTTP test double (buzz-cli has no
    /// mocking harness; see the report's residuals). This pins the exit-code
    /// contract those propagated errors carry instead.
    #[test]
    fn a_failed_relay_lookup_maps_to_exit_2_never_null() {
        assert_eq!(
            exit_code(&CliError::Relay {
                status: 500,
                body: String::new(),
            }),
            2
        );
        assert_eq!(
            exit_code(&CliError::Network(
                reqwest::Client::new().get("not-a-url").build().unwrap_err()
            )),
            2
        );
        assert_eq!(
            exit_code(&CliError::Relay {
                status: 401,
                body: String::new(),
            }),
            3,
            "401/403 are auth failures, distinct from a name-lookup miss"
        );
    }
}
