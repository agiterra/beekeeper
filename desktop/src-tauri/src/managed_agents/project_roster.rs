//! A project's agents are project members (NIP-MP; ledger 173).
//!
//! The relay admits a Pulse or to-do write only from a pubkey on the private
//! project's roster as owner or collaborator, and a seat runs under the
//! agent's own key — so an agent that is merely *associated* with a project
//! on this computer is refused the moment it writes. Creation and **Finish
//! repository setup** put every project agent on the roster as a
//! collaborator ([`ensure_project_agents_on_roster`]), and the Agents tab's
//! association does the same for one agent ([`put_project_agents`]).
//!
//! The ops are the ones `bee projects add-member` and the desktop roster
//! editor publish: kind 9010 with `["a", coord]` and one
//! `["p", <hex>, "", <role>]` (arity exactly 4) per member; kind 9011 with
//! `["a", coord]` and `["p", <hex>]` per removed member
//! (`crates/buzz-cli/src/commands/projects.rs::cmd_put_member`,
//! `desktop/src/features/projects-container/lib/projectMembers.ts`). The
//! relay accepts them only from the project's creator or a roster owner, so
//! the check is made here first and its refusal is reported in words rather
//! than as a relay error.

use nostr::{Event, EventBuilder, Keys, Kind, Tag};

use crate::app_state::AppState;
use crate::managed_agents::project_agent_association::normalize_project_ref;
use buzz_core_pkg::kind::{
    is_private_project_event, is_valid_project_role, KIND_PROJECT, KIND_PROJECT_MEMBERS,
    KIND_PROJECT_PUT_MEMBER, KIND_PROJECT_REMOVE_MEMBER, PROJECT_ROLE_COLLABORATOR,
    PROJECT_ROLE_OWNER,
};

/// One roster row: `(pubkey, role)`, the pubkey lowercase hex.
pub(crate) type RosterRow = (String, String);

/// `(owner-hex, dtag)` of a normalized coordinate.
fn split_coordinate(coordinate: &str) -> Result<(String, String), String> {
    let normalized = normalize_project_ref(coordinate)
        .ok_or_else(|| format!("{coordinate:?} is not a project coordinate"))?;
    let mut parts = normalized.splitn(3, ':');
    parts.next();
    match (parts.next(), parts.next()) {
        (Some(owner), Some(dtag)) => Ok((owner.to_string(), dtag.to_string())),
        _ => Err(format!("{coordinate:?} is not a project coordinate")),
    }
}

fn lower_hex64(value: &str) -> Result<String, String> {
    let lower = value.trim().to_ascii_lowercase();
    if lower.len() == 64 && lower.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(lower)
    } else {
        Err(format!("{value:?} is not a 64-hex pubkey"))
    }
}

fn tag(parts: Vec<String>) -> Result<Tag, String> {
    Tag::parse(parts).map_err(|error| format!("invalid tag: {error}"))
}

/// The kind 9010 op putting `members` on `project_ref`'s roster, signed by
/// `keys`: `["a", coord]` then one `["p", <hex>, "", <role>]` per member.
pub(crate) fn build_put_members_event(
    keys: &Keys,
    project_ref: &str,
    members: &[(String, &str)],
) -> Result<Event, String> {
    let coordinate = normalize_project_ref(project_ref)
        .ok_or_else(|| format!("{project_ref:?} is not a project coordinate"))?;
    if members.is_empty() {
        return Err("a put-member op names at least one member".to_string());
    }
    let mut tags = vec![tag(vec!["a".to_string(), coordinate])?];
    for (pubkey, role) in members {
        if !is_valid_project_role(role) {
            return Err(format!("{role:?} is not a project role"));
        }
        tags.push(tag(vec![
            "p".to_string(),
            lower_hex64(pubkey)?,
            String::new(),
            (*role).to_string(),
        ])?);
    }
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PUT_MEMBER as u16), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the put-member op: {error}"))
}

/// The kind 9011 op removing `pubkey` from `project_ref`'s roster:
/// `["a", coord]` and `["p", <hex>]`.
pub(crate) fn build_remove_member_event(
    keys: &Keys,
    project_ref: &str,
    pubkey: &str,
) -> Result<Event, String> {
    let coordinate = normalize_project_ref(project_ref)
        .ok_or_else(|| format!("{project_ref:?} is not a project coordinate"))?;
    let tags = vec![
        tag(vec!["a".to_string(), coordinate])?,
        tag(vec!["p".to_string(), lower_hex64(pubkey)?])?,
    ];
    EventBuilder::new(Kind::Custom(KIND_PROJECT_REMOVE_MEMBER as u16), "")
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the remove-member op: {error}"))
}

/// Publish one kind 9010 putting `members` on the roster as `keys`, which
/// must be the project's creator or a roster owner (the relay refuses any
/// other signer). `Ok(None)` when `members` is empty: nothing to publish.
/// Returns the op's event id.
pub(crate) async fn put_project_agents(
    state: &AppState,
    keys: &Keys,
    project_ref: &str,
    members: &[(String, &str)],
) -> Result<Option<String>, String> {
    if members.is_empty() {
        return Ok(None);
    }
    let event = build_put_members_event(keys, project_ref, members)?;
    crate::relay::submit_signed_event_with_keys(&event, state, keys, None).await?;
    Ok(Some(event.id.to_hex()))
}

/// Publish one kind 9011 removing `pubkey` from the roster as `keys`.
/// Returns the op's event id.
///
/// No command removes an agent from a project in this build (there is no
/// disassociate counterpart to `associate_managed_agent_with_project`), so
/// nothing calls this yet; it is the twin of [`put_project_agents`] for the
/// day one does.
#[allow(dead_code)]
pub(crate) async fn remove_project_agent(
    state: &AppState,
    keys: &Keys,
    project_ref: &str,
    pubkey: &str,
) -> Result<String, String> {
    let event = build_remove_member_event(keys, project_ref, pubkey)?;
    crate::relay::submit_signed_event_with_keys(&event, state, keys, None).await?;
    Ok(event.id.to_hex())
}

/// `(pubkey, role)` rows from an event's `p` tags. A missing or unknown role
/// element is a legacy collaborator, exactly as `bee projects members` reads
/// it (`roster_from_event_json`).
pub(crate) fn roster_rows(event: &Event) -> Vec<RosterRow> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            let parts = tag.as_slice();
            if parts.first().map(String::as_str) != Some("p") {
                return None;
            }
            let pubkey = lower_hex64(parts.get(1)?).ok()?;
            let role = parts
                .get(3)
                .map(String::as_str)
                .filter(|role| is_valid_project_role(role))
                .unwrap_or(PROJECT_ROLE_COLLABORATOR);
            Some((pubkey, role.to_string()))
        })
        .collect()
}

fn newest(events: Vec<Event>) -> Option<Event> {
    events
        .into_iter()
        .max_by_key(|event| (event.created_at, event.id))
}

/// The project's current roster as the relay holds it: the newest kind
/// 39010 projection for the coordinate, or — when the roster is still
/// head-sourced — the newest kind 30621 head's own `p` tags. The creator is
/// not a row (a membership op naming them is refused) and is not added
/// here. `Err` when neither exists or the relay could not be read: an
/// unreadable project is never an empty roster.
pub(crate) async fn read_project_roster(
    state: &AppState,
    project_ref: &str,
) -> Result<Vec<RosterRow>, String> {
    let (owner, dtag) = split_coordinate(project_ref)?;
    let coordinate = format!("{KIND_PROJECT}:{owner}:{dtag}");
    let projections = crate::relay::query_relay(
        state,
        &[serde_json::json!({
            "kinds": [KIND_PROJECT_MEMBERS],
            "#d": [coordinate],
            "limit": 8,
        })],
    )
    .await
    .map_err(|error| format!("could not read the project roster: {error}"))?;
    if let Some(projection) = newest(
        projections
            .into_iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_PROJECT_MEMBERS)
            .collect(),
    ) {
        return Ok(roster_rows(&projection));
    }
    let head = read_project_head(state, &owner, &dtag)
        .await?
        .ok_or_else(|| format!("{coordinate} has no project head on the relay"))?;
    Ok(roster_rows(&head))
}

/// The newest kind 30621 by `owner` with `d` = `dtag` whose signature
/// verifies, or `None` when the relay holds none.
pub(crate) async fn read_project_head(
    state: &AppState,
    owner: &str,
    dtag: &str,
) -> Result<Option<Event>, String> {
    let heads = crate::relay::query_relay(
        state,
        &[serde_json::json!({
            "kinds": [KIND_PROJECT],
            "authors": [owner],
            "#d": [dtag],
            "limit": 8,
        })],
    )
    .await
    .map_err(|error| format!("could not read the project head: {error}"))?;
    Ok(newest(
        heads
            .into_iter()
            .filter(|event| {
                u32::from(event.kind.as_u16()) == KIND_PROJECT
                    && event.pubkey.to_hex() == owner
                    && event.tags.iter().any(|tag| {
                        let parts = tag.as_slice();
                        parts.first().map(String::as_str) == Some("d")
                            && parts.get(1).map(String::as_str) == Some(dtag)
                    })
                    && event.verify().is_ok()
            })
            .collect(),
    ))
}

/// The project's display name from its head's `name` tag, or the slug when
/// the head has none.
pub(crate) fn project_name(head: Option<&Event>, dtag: &str) -> String {
    head.and_then(|event| {
        event.tags.iter().find_map(|tag| {
            let parts = tag.as_slice();
            (parts.first().map(String::as_str) == Some("name"))
                .then(|| parts.get(1).map(|name| name.trim().to_string()))
                .flatten()
                .filter(|name| !name.is_empty())
        })
    })
    .unwrap_or_else(|| dtag.to_string())
}

/// Why `viewer` may not write `project_ref`'s roster, given the roster as
/// read: only the creator (the coordinate's owner) or a roster owner may.
pub(crate) fn roster_write_refusal(
    project_ref: &str,
    viewer: &str,
    roster: &[RosterRow],
) -> Option<String> {
    let Ok((owner, _)) = split_coordinate(project_ref) else {
        return Some(format!("{project_ref:?} is not a project coordinate"));
    };
    let viewer = viewer.to_ascii_lowercase();
    if viewer == owner {
        return None;
    }
    let role = roster
        .iter()
        .find(|(pubkey, _)| *pubkey == viewer)
        .map(|(_, role)| role.as_str());
    if role == Some(PROJECT_ROLE_OWNER) {
        return None;
    }
    Some(format!(
        "only the project's creator or an owner can put agents on its roster; {}… is {}",
        &viewer[..8.min(viewer.len())],
        role.map(|role| format!("a {role}"))
            .unwrap_or_else(|| "not a member".to_string())
    ))
}

/// Put `provider_pubkey` on `project_ref`'s roster as a collaborator, but
/// only when the project is private.
///
/// The host does the project's work — host steps, worktrees, verify results
/// — under this key, never the seat's, so a private project's roster must
/// name it or the relay's read gate withholds every repository event
/// (relay-signed ref state included) from it
/// (`crates/buzz-db/src/git_repo.rs::hidden_repos_for_reader`,
/// `crates/buzz-core/src/kind.rs::repo_event_hidden_from`; proven live in
/// kettle-control-6, 2026-09-24). A public project's repositories are not
/// gated on roster membership, so this is skipped there — adding the host
/// would be roster noise the relay never checks.
///
/// `None` when the project's head could not be read or is public: nothing
/// was attempted. `Some` otherwise, carrying the same report
/// [`ensure_project_agents_on_roster`] would (idempotent, infallible for the
/// caller).
pub(crate) async fn ensure_host_on_private_roster(
    state: &AppState,
    keys: &Keys,
    project_ref: &str,
    provider_pubkey: &str,
) -> Option<RosterOutcome> {
    let (owner, dtag) = split_coordinate(project_ref).ok()?;
    let head = read_project_head(state, &owner, &dtag).await.ok()??;
    if !is_private_project_event(&head) {
        return None;
    }
    Some(
        ensure_project_agents_on_roster(state, keys, project_ref, &[provider_pubkey.to_string()])
            .await,
    )
}

/// What [`ensure_project_agents_on_roster`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct RosterOutcome {
    /// The pubkeys this run put on the roster (one kind 9010 for all).
    pub added: Vec<String>,
    /// The op's event id when one was published.
    pub event_id: Option<String>,
    /// Why some or all of `agents` are not on the roster, in words.
    pub error: Option<String>,
}

/// Put every pubkey in `agents` that is not already on `project_ref`'s
/// roster there as a collaborator, in one kind 9010, signed by `keys`.
///
/// Idempotent: an agent already on the roster (any role) is left alone. The
/// roster is read first; when it cannot be read, or when `keys` is neither
/// the creator nor an owner, nothing is published and `error` says why.
/// Never fails the caller: the outcome is a report.
pub(crate) async fn ensure_project_agents_on_roster(
    state: &AppState,
    keys: &Keys,
    project_ref: &str,
    agents: &[String],
) -> RosterOutcome {
    let mut outcome = RosterOutcome::default();
    if agents.is_empty() {
        return outcome;
    }
    let roster = match read_project_roster(state, project_ref).await {
        Ok(roster) => roster,
        Err(error) => {
            outcome.error = Some(error);
            return outcome;
        }
    };
    let viewer = keys.public_key().to_hex();
    if let Some(refusal) = roster_write_refusal(project_ref, &viewer, &roster) {
        outcome.error = Some(refusal);
        return outcome;
    }
    let mut missing: Vec<(String, &str)> = Vec::new();
    for agent in agents {
        let Ok(pubkey) = lower_hex64(agent) else {
            outcome.error = Some(format!("{agent:?} is not a 64-hex pubkey"));
            return outcome;
        };
        if pubkey == viewer
            || roster.iter().any(|(member, _)| *member == pubkey)
            || missing.iter().any(|(member, _)| *member == pubkey)
        {
            continue;
        }
        missing.push((pubkey, PROJECT_ROLE_COLLABORATOR));
    }
    match put_project_agents(state, keys, project_ref, &missing).await {
        Ok(event_id) => {
            outcome.event_id = event_id;
            outcome.added = missing.into_iter().map(|(pubkey, _)| pubkey).collect();
        }
        Err(error) => outcome.error = Some(error),
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags_of(event: &Event) -> Vec<Vec<String>> {
        event.tags.iter().map(|t| t.as_slice().to_vec()).collect()
    }

    #[test]
    fn the_put_op_carries_the_coordinate_and_one_arity_four_p_per_member() {
        let keys = Keys::generate();
        let owner = keys.public_key().to_hex();
        let coord = format!("30621:{owner}:demo");
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let event = build_put_members_event(
            &keys,
            &coord,
            &[(a.clone(), "collaborator"), (b.clone(), "owner")],
        )
        .expect("builds");
        assert_eq!(event.kind, Kind::Custom(9010));
        assert_eq!(
            tags_of(&event),
            vec![
                vec!["a".to_string(), coord.clone()],
                vec![
                    "p".to_string(),
                    a,
                    String::new(),
                    "collaborator".to_string()
                ],
                vec![
                    "p".to_string(),
                    b.to_ascii_lowercase(),
                    String::new(),
                    "owner".to_string()
                ],
            ]
        );
        assert!(event.verify().is_ok());
        assert!(build_put_members_event(&keys, &coord, &[]).is_err());
        assert!(build_put_members_event(&keys, &coord, &[("nope".to_string(), "owner")]).is_err());
        assert!(build_put_members_event(&keys, &coord, &[("a".repeat(64), "admin")]).is_err());
        assert!(build_put_members_event(&keys, "30617:x:y", &[("a".repeat(64), "owner")]).is_err());
    }

    #[test]
    fn the_remove_op_carries_the_coordinate_and_a_bare_p() {
        let keys = Keys::generate();
        let owner = keys.public_key().to_hex();
        let coord = format!("30621:{owner}:demo");
        let event = build_remove_member_event(&keys, &coord, &"C".repeat(64)).expect("builds");
        assert_eq!(event.kind, Kind::Custom(9011));
        assert_eq!(
            tags_of(&event),
            vec![
                vec!["a".to_string(), coord],
                vec!["p".to_string(), "c".repeat(64)],
            ]
        );
    }

    #[test]
    fn roster_rows_read_the_role_and_default_to_collaborator() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(39010), "")
            .tags(vec![
                Tag::parse(vec!["d".to_string(), "30621:x:y".to_string()]).unwrap(),
                Tag::parse(vec![
                    "p".to_string(),
                    "A".repeat(64),
                    String::new(),
                    "owner".to_string(),
                ])
                .unwrap(),
                Tag::parse(vec!["p".to_string(), "b".repeat(64)]).unwrap(),
                Tag::parse(vec![
                    "p".to_string(),
                    "c".repeat(64),
                    String::new(),
                    "admin".to_string(),
                ])
                .unwrap(),
                Tag::parse(vec!["p".to_string(), "short".to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys)
            .unwrap();
        assert_eq!(
            roster_rows(&event),
            vec![
                ("a".repeat(64), "owner".to_string()),
                ("b".repeat(64), "collaborator".to_string()),
                ("c".repeat(64), "collaborator".to_string()),
            ]
        );
    }

    #[test]
    fn only_the_creator_or_a_roster_owner_may_write() {
        let creator = "a".repeat(64);
        let coord = format!("30621:{creator}:demo");
        let roster = vec![
            ("b".repeat(64), "owner".to_string()),
            ("c".repeat(64), "collaborator".to_string()),
        ];
        assert_eq!(roster_write_refusal(&coord, &creator, &roster), None);
        assert_eq!(roster_write_refusal(&coord, &"B".repeat(64), &roster), None);
        let collaborator = roster_write_refusal(&coord, &"c".repeat(64), &roster).expect("refused");
        assert!(collaborator.contains("a collaborator"), "{collaborator}");
        let stranger = roster_write_refusal(&coord, &"d".repeat(64), &roster).expect("refused");
        assert!(stranger.contains("not a member"), "{stranger}");
        assert!(stranger.starts_with("only the project's creator or an owner"));
    }

    #[test]
    fn the_project_name_comes_from_the_head_or_falls_back_to_the_slug() {
        let keys = Keys::generate();
        let head = EventBuilder::new(Kind::Custom(30621), "")
            .tags(vec![
                Tag::parse(vec!["d".to_string(), "rpg-test".to_string()]).unwrap(),
                Tag::parse(vec!["name".to_string(), " RPG Test ".to_string()]).unwrap(),
            ])
            .sign_with_keys(&keys)
            .unwrap();
        assert_eq!(project_name(Some(&head), "rpg-test"), "RPG Test");
        assert_eq!(project_name(None, "rpg-test"), "rpg-test");
    }
}
