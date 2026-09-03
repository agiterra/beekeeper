use buzz_core::{
    git_perms::{parse_protection_tag, parse_protection_tags, RefPattern},
    kind::KIND_GIT_REPO_ANNOUNCEMENT,
};
use buzz_sdk::build_delete_addressable;
use nostr::{Event, EventBuilder, Tag, Timestamp};

use crate::client::BuzzClient;
use crate::commands::parse_write_response;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_repo_id};

fn parse_events(json: &str) -> Result<Vec<Event>, CliError> {
    serde_json::from_str(json)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))
}

async fn fetch_own_repo_announcement(
    client: &BuzzClient,
    repo_id: &str,
) -> Result<Option<Event>, CliError> {
    let filter = serde_json::json!({
        "kinds": [KIND_GIT_REPO_ANNOUNCEMENT],
        "authors": [client.keys().public_key().to_hex()],
        "#d": [repo_id],
        "limit": 1,
    });
    let raw = client.query(&filter).await?;
    let mut events = parse_events(&raw)?;
    events.sort_by_key(|event| std::cmp::Reverse(event.created_at));
    Ok(events.into_iter().next())
}

fn repo_id_from_event(event: &Event) -> Result<&str, CliError> {
    event
        .tags
        .iter()
        .find_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some("d"))
                .then(|| values.get(1).map(String::as_str))
                .flatten()
        })
        .ok_or_else(|| CliError::Other("repository announcement is missing its d tag".into()))
}

fn tag_error(error: impl std::fmt::Display) -> CliError {
    CliError::Other(format!("failed to build protection tag: {error}"))
}

fn protection_pattern(tag: &Tag) -> Option<&str> {
    let values = tag.as_slice();
    (values.first().map(String::as_str) == Some("buzz-protect"))
        .then(|| values.get(1).map(String::as_str))
        .flatten()
}

fn has_tag_name(tag: &Tag, name: &str) -> bool {
    tag.as_slice().first().map(String::as_str) == Some(name)
}

/// The rule flags one `bee repos protect set` writes, in wire order.
#[derive(Debug, Clone, Copy, Default)]
struct ProtectionFlags {
    no_force_push: bool,
    no_delete: bool,
    require_patch: bool,
    /// Admit an update only when an approved mission verdict names the pushed
    /// commit. Enforced by the relay serving the repository; one that predates
    /// the rule parses the token into `unknown_rules` and ignores it, which
    /// `bee repos protect list` shows.
    require_verdict: bool,
}

fn build_protection_tag(
    ref_pattern: &str,
    push_role: Option<&str>,
    flags: ProtectionFlags,
) -> Result<Tag, CliError> {
    let mut values = vec!["buzz-protect".to_string(), ref_pattern.to_string()];
    if let Some(role) = push_role {
        values.push(format!("push:{role}"));
    }
    if flags.no_force_push {
        values.push("no-force-push".into());
    }
    if flags.no_delete {
        values.push("no-delete".into());
    }
    if flags.require_patch {
        values.push("require-patch".into());
    }
    if flags.require_verdict {
        values.push("require-verdict".into());
    }
    let rule_values: Vec<&str> = values[1..].iter().map(String::as_str).collect();
    parse_protection_tag(&rule_values)
        .map_err(|error| CliError::Usage(format!("invalid protection rule: {error}")))?;
    Tag::parse(values).map_err(tag_error)
}

enum RepoChange {
    SetProtection(Box<Tag>),
    RemoveProtection(String),
    /// Bind (or rebind) the repo to a channel: replaces every existing
    /// `buzz-channel` tag with exactly one carrying the validated UUID.
    BindChannel(String),
    /// Link (or relink) the repo into a project: replaces every existing
    /// `project` tag with exactly one carrying the normalized coordinate.
    LinkProject(String),
}

fn build_updated_repo_announcement(
    existing: &Event,
    change: RepoChange,
) -> Result<EventBuilder, CliError> {
    let repo_id = repo_id_from_event(existing)?;
    // What to strip beyond `auth` (always stripped), and what to append. The
    // stripped tag name is carried explicitly so a rebind replaces duplicates
    // rather than stacking a second, ambiguous binding — the relay resolves
    // `buzz-channel` first-tag-wins and fails closed on ambiguity.
    let (removed_pattern, removed_tag_name, replacement) = match change {
        RepoChange::SetProtection(tag) => {
            let pattern = protection_pattern(&tag)
                .ok_or_else(|| CliError::Other("replacement is not a protection tag".into()))?
                .to_string();
            (Some(pattern), None, Some(*tag))
        }
        RepoChange::RemoveProtection(pattern) => {
            RefPattern::parse(&pattern)
                .map_err(|error| CliError::Usage(format!("invalid ref pattern: {error}")))?;
            (Some(pattern), None, None)
        }
        RepoChange::BindChannel(channel) => {
            crate::validate::validate_uuid(&channel)?;
            let tag = Tag::parse(["buzz-channel", channel.as_str()]).map_err(tag_error)?;
            (None, Some("buzz-channel"), Some(tag))
        }
        RepoChange::LinkProject(project) => {
            let coordinate = validate_project_coordinate(&project)?;
            let tag = Tag::parse(["project", coordinate.as_str()]).map_err(tag_error)?;
            (None, Some("project"), Some(tag))
        }
    };

    let mut tags: Vec<Tag> = existing
        .tags
        .iter()
        .filter(|tag| {
            if has_tag_name(tag, "auth") {
                return false;
            }
            if let Some(name) = removed_tag_name {
                if has_tag_name(tag, name) {
                    return false;
                }
            }
            removed_pattern.is_none() || protection_pattern(tag) != removed_pattern.as_deref()
        })
        .cloned()
        .collect();
    if let Some(tag) = replacement {
        tags.push(tag);
    }

    let raw_tags: Vec<Vec<String>> = tags.iter().map(|tag| tag.as_slice().to_vec()).collect();
    parse_protection_tags(&raw_tags).map_err(|error| {
        CliError::Other(format!(
            "repository contains invalid protection rules; refusing update: {error}"
        ))
    })?;

    // Advance only the observed head. Using wall-clock time here would let a
    // delayed writer leapfrog an intervening update and silently erase metadata.
    let next_created_at = existing
        .created_at
        .as_secs()
        .checked_add(1)
        .ok_or_else(|| CliError::Other("repository timestamp cannot be advanced".into()))?;
    buzz_sdk::build_repo_announcement_with_tags(repo_id, &existing.content, tags)
        .map_err(|error| CliError::Other(format!("failed to build repository update: {error}")))
        .map(|builder| builder.custom_created_at(Timestamp::from(next_created_at)))
}

fn protection_rules_json(event: &Event) -> Result<serde_json::Value, CliError> {
    let raw_tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    let (unknown_rules, validation_error) = match parse_protection_tags(&raw_tags) {
        Ok(parsed) => (parsed.unknown_rules, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };
    let protections: Vec<serde_json::Value> = event
        .tags
        .iter()
        .filter_map(|tag| {
            let values = tag.as_slice();
            (values.first().map(String::as_str) == Some("buzz-protect")).then(|| {
                serde_json::json!({
                    "ref": values.get(1).map(String::as_str).unwrap_or(""),
                    "rules": values.get(2..).unwrap_or_default(),
                })
            })
        })
        .collect();

    Ok(serde_json::json!({
        "repo_id": repo_id_from_event(event)?,
        "protections": protections,
        "unknown_rules": unknown_rules,
        "validation_error": validation_error,
    }))
}

fn validate_write_response(raw: &str) -> Result<String, CliError> {
    parse_write_response(
        raw,
        "repository changed concurrently; fetch the latest rules and retry",
    )
}

async fn submit_repo_update(client: &BuzzClient, builder: EventBuilder) -> Result<(), CliError> {
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!("{}", validate_write_response(&raw)?);
    Ok(())
}

/// Validate a `30621:<owner-hex>:<dtag>` project coordinate and return it in
/// the normalized form the relay stores.
///
/// Shares [`buzz_core::kind::normalize_project_coordinate`] with the relay's
/// ingest validation and gate lookups, so a coordinate this CLI accepts is
/// exactly one they can resolve.
fn validate_project_coordinate(coordinate: &str) -> Result<String, CliError> {
    buzz_core::kind::normalize_project_coordinate(coordinate).ok_or_else(|| {
        CliError::Usage(format!(
            "invalid project coordinate {coordinate:?}; expected 30621:<64-hex-owner>:<project-d>"
        ))
    })
}

/// Build the kind:30617 announcement for `repos create`, including the
/// `buzz-channel` binding and/or `project` back-reference when requested.
///
/// Pure (no I/O) so the emitted tags are unit-testable. A repository is
/// reachable through either ACL — its project's roster or its bound
/// channel's membership — and the relay grants the more permissive of the
/// two. With neither, the relay 404s every clone/fetch/push (issue #3527),
/// so both values are shape-validated here; their existence and the caller's
/// membership are the relay's authority at git-access time, the same posture
/// as `repos bind`.
#[allow(clippy::too_many_arguments)]
fn build_create_announcement(
    repo_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    clone_urls: &[String],
    web_url: Option<&str>,
    relays: &[String],
    channel: Option<&str>,
    project: Option<&str>,
) -> Result<EventBuilder, CliError> {
    validate_repo_id(repo_id)?;

    let clone_refs: Vec<&str> = clone_urls.iter().map(|s| s.as_str()).collect();
    let relay_refs: Vec<&str> = relays.iter().map(|s| s.as_str()).collect();

    let mut builder = buzz_sdk::build_repo_announcement(
        repo_id,
        name,
        description,
        &clone_refs,
        web_url,
        &relay_refs,
    )
    .map_err(|e| CliError::Other(format!("build_repo_announcement failed: {e}")))?;

    if let Some(channel) = channel {
        crate::validate::validate_uuid(channel)?;
        builder = builder.tag(Tag::parse(["buzz-channel", channel]).map_err(tag_error)?);
    }
    if let Some(project) = project {
        let coordinate = validate_project_coordinate(project)?;
        builder = builder.tag(Tag::parse(["project", &coordinate]).map_err(tag_error)?);
    }
    Ok(builder)
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_create_repo(
    client: &BuzzClient,
    repo_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    clone_urls: &[String],
    web_url: Option<&str>,
    relays: &[String],
    channel: Option<&str>,
    project: Option<&str>,
) -> Result<(), CliError> {
    let builder = build_create_announcement(
        repo_id,
        name,
        description,
        clone_urls,
        web_url,
        relays,
        channel,
        project,
    )?;
    let event = client.sign_event(builder)?;
    let owner = event.pubkey.to_hex();
    let resp = client.submit_event(event).await?;
    // `link` renders as a rich preview card in Beekeeper Desktop when included in
    // a chat message — agents announce repos with it (see base_prompt.md).
    let link = crate::links::repo_link(&owner, repo_id);
    crate::client::print_create_response(&resp, "link", &link);
    Ok(())
}

pub async fn cmd_get_repo(
    client: &BuzzClient,
    repo_id: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_repo_id(repo_id)?;

    let mut filter = serde_json::json!({
        "kinds": [30617],
        "#d": [repo_id]
    });

    // If owner specified, filter by author pubkey; otherwise return any match.
    // Note: without --owner, multiple repos with the same name (different owners) may be returned.
    if let Some(pk) = owner {
        crate::validate::validate_hex64(pk)?;
        filter["authors"] = serde_json::json!([pk]);
    }

    let resp = client.query(&filter).await?;
    println!("{resp}");
    Ok(())
}

pub async fn cmd_list_repos(
    client: &BuzzClient,
    owner: Option<&str>,
    limit: Option<u32>,
) -> Result<(), CliError> {
    // Default to self if no owner specified.
    let pubkey = match owner {
        Some(pk) => {
            crate::validate::validate_hex64(pk)?;
            pk.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };

    let mut filter = serde_json::json!({
        "kinds": [30617],
        "authors": [pubkey]
    });

    if let Some(n) = limit {
        filter["limit"] = serde_json::json!(n);
    }

    let resp = client.query(&filter).await?;
    println!("{resp}");
    Ok(())
}

async fn current_repo(client: &BuzzClient, repo_id: &str) -> Result<Event, CliError> {
    validate_repo_id(repo_id)?;
    fetch_own_repo_announcement(client, repo_id)
        .await?
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "repository {repo_id:?} was not found for the current identity"
            ))
        })
}

async fn cmd_protect_list(client: &BuzzClient, repo_id: &str) -> Result<(), CliError> {
    let event = current_repo(client, repo_id).await?;
    let mut listing = protection_rules_json(&event)?;
    // `unknown_rules` above is what THIS build does not recognise. A relay
    // predating a rule has its own unknown list and ignores the token, and a
    // person reading this listing would otherwise never learn that the two can
    // disagree. Disclosed here rather than in the docs alone.
    if let Some(object) = listing.as_object_mut() {
        object.insert(
            "evaluated_by".to_string(),
            serde_json::Value::String(
                crate::commands::git_setup::ENFORCEMENT_DISCLOSURE.to_string(),
            ),
        );
        object.insert(
            "serving_relay".to_string(),
            serde_json::Value::String(
                crate::commands::git_setup::serving_relay_build(client.relay_url()).await,
            ),
        );
    }
    println!("{listing}");
    Ok(())
}

async fn cmd_protect_set(
    client: &BuzzClient,
    repo_id: &str,
    ref_pattern: &str,
    push_role: Option<crate::RepoPushRole>,
    flags: ProtectionFlags,
) -> Result<(), CliError> {
    let push_role = push_role.map(|role| match role {
        crate::RepoPushRole::Owner => "owner",
        crate::RepoPushRole::Admin => "admin",
        crate::RepoPushRole::Member => "member",
    });
    let tag = build_protection_tag(ref_pattern, push_role, flags)?;
    let event = current_repo(client, repo_id).await?;
    let builder =
        build_updated_repo_announcement(&event, RepoChange::SetProtection(Box::new(tag)))?;
    submit_repo_update(client, builder).await
}

async fn cmd_protect_remove(
    client: &BuzzClient,
    repo_id: &str,
    ref_pattern: &str,
) -> Result<(), CliError> {
    RefPattern::parse(ref_pattern)
        .map_err(|error| CliError::Usage(format!("invalid ref pattern: {error}")))?;
    let event = current_repo(client, repo_id).await?;
    if !event
        .tags
        .iter()
        .any(|tag| protection_pattern(tag) == Some(ref_pattern))
    {
        return Err(CliError::NotFound(format!(
            "repository {repo_id:?} has no protection rule for {ref_pattern:?}"
        )));
    }
    let builder = build_updated_repo_announcement(
        &event,
        RepoChange::RemoveProtection(ref_pattern.to_string()),
    )?;
    submit_repo_update(client, builder).await
}

/// Give a repository an ACL — the fix path for issue #3527's permanently-404
/// repos. Publishes a read-modify-write update of the caller's own
/// kind:30617 with exactly one `buzz-channel` tag, one `project` tag, or
/// both; all other metadata (protections, name, description, future tags) is
/// preserved by the same machinery `repos protect` uses.
///
/// Each update is applied as its own read-modify-write so the second reads
/// the head the first published — otherwise the second would rebuild from a
/// stale head and drop the first tag.
///
/// Values are validated for *shape* only — deliberately. Channel/project
/// existence and the caller's membership are the relay's authority at
/// git-access time; a CLI-side network pre-check would just be TOCTOU with
/// extra latency.
async fn cmd_bind_repo(
    client: &BuzzClient,
    repo_id: &str,
    channel: Option<&str>,
    project: Option<&str>,
) -> Result<(), CliError> {
    if channel.is_none() && project.is_none() {
        return Err(CliError::Usage(
            "specify --channel, --project, or both: a repository with neither is unreachable"
                .into(),
        ));
    }
    if let Some(channel) = channel {
        let event = current_repo(client, repo_id).await?;
        let builder =
            build_updated_repo_announcement(&event, RepoChange::BindChannel(channel.to_string()))?;
        submit_repo_update(client, builder).await?;
    }
    if let Some(project) = project {
        let event = current_repo(client, repo_id).await?;
        let builder =
            build_updated_repo_announcement(&event, RepoChange::LinkProject(project.to_string()))?;
        submit_repo_update(client, builder).await?;
    }
    Ok(())
}

/// `bee repos delete` — tombstone a repository's kind:30617 announcement.
///
/// A kind:5 carrying `["a", "30617:<owner>:<repo-id>"]`. The relay's
/// addressable-deletion path soft-deletes the announcement, clears the
/// repo's project link, soft-deletes the relay-signed kind:30618 ref state,
/// and removes the object-store pointer — so the repository stops being
/// listed *and* stops being cloneable. Deleting only the announcement would
/// have left it fully cloneable by anyone who still had access.
///
/// The announcement is not fetched first, for the same reason `terminals
/// delete` does not fetch: requiring it to be readable would fail for
/// exactly the case a project Owner needs, and the relay is the authority
/// on whether the coordinate exists.
///
/// Two survivors, both deliberate and both reported by the surfaces that
/// offer this: the name reservation in `git_repo_names` (deletion never
/// frees a name to squat) and the content-addressed pack objects (shared
/// with forks and identical trees, so reclaiming them here could destroy a
/// neighbour's history).
pub async fn cmd_delete_repo(
    client: &BuzzClient,
    repo_id: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_repo_id(repo_id)?;
    let owner_hex = match owner {
        Some(owner) => {
            validate_lower_hex64("--owner", owner)?;
            owner.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };

    let builder = build_delete_addressable(KIND_GIT_REPO_ANNOUNCEMENT, &owner_hex, repo_id)
        .map_err(|e| CliError::Usage(e.to_string()))?;
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "no live announcement matched that coordinate")?
    );
    Ok(())
}

pub async fn dispatch(cmd: crate::ReposCmd, client: &BuzzClient) -> Result<(), CliError> {
    use crate::{ReposCmd, ReposProtectCmd};
    match cmd {
        ReposCmd::Create {
            id,
            name,
            description,
            clone_urls,
            web,
            relays,
            channel,
            project,
        } => {
            cmd_create_repo(
                client,
                &id,
                name.as_deref(),
                description.as_deref(),
                &clone_urls,
                web.as_deref(),
                &relays,
                channel.as_deref(),
                project.as_deref(),
            )
            .await
        }
        ReposCmd::Get { id, owner } => cmd_get_repo(client, &id, owner.as_deref()).await,
        ReposCmd::List { owner, limit } => cmd_list_repos(client, owner.as_deref(), limit).await,
        ReposCmd::Bind {
            id,
            channel,
            project,
        } => cmd_bind_repo(client, &id, channel.as_deref(), project.as_deref()).await,
        ReposCmd::Delete { id, owner } => cmd_delete_repo(client, &id, owner.as_deref()).await,
        ReposCmd::Protect(command) => match command {
            ReposProtectCmd::List { id } => cmd_protect_list(client, &id).await,
            ReposProtectCmd::Set {
                id,
                ref_pattern,
                push,
                no_force_push,
                no_delete,
                require_patch,
                require_verdict,
            } => {
                cmd_protect_set(
                    client,
                    &id,
                    &ref_pattern,
                    push,
                    ProtectionFlags {
                        no_force_push,
                        no_delete,
                        require_patch,
                        require_verdict,
                    },
                )
                .await
            }
            ReposProtectCmd::Remove { id, ref_pattern } => {
                cmd_protect_remove(client, &id, &ref_pattern).await
            }
        },
    }
}

#[cfg(test)]
mod tests {
    use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

    use super::{
        build_create_announcement, build_delete_addressable, build_protection_tag,
        build_updated_repo_announcement, protection_rules_json, validate_write_response,
        ProtectionFlags, RepoChange, KIND_GIT_REPO_ANNOUNCEMENT,
    };

    const OWNER_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    /// The tombstone `bee repos delete` signs. The relay routes an
    /// addressable deletion entirely on this `a` tag, and refuses any kind:5
    /// carrying both an `a` and an `e` tag, so the shape is the contract.
    #[test]
    fn delete_builds_a_kind_5_naming_the_announcement_coordinate() {
        let event = build_delete_addressable(KIND_GIT_REPO_ANNOUNCEMENT, OWNER_HEX, "myrepo")
            .expect("delete builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign tombstone");
        assert_eq!(event.kind, Kind::Custom(5));
        let tags: Vec<Vec<String>> = event.tags.iter().map(|t| t.as_slice().to_vec()).collect();
        assert_eq!(
            tags,
            vec![vec!["a".to_string(), format!("30617:{OWNER_HEX}:myrepo")]]
        );
    }

    /// Deleting somebody else's repository is the project-Owner case, and
    /// the coordinate has to name *their* key — a tombstone addressed to the
    /// caller names a coordinate that does not exist, which the relay
    /// accepts and which deletes nothing.
    #[test]
    fn delete_addresses_the_named_owner_not_the_caller() {
        let other = "b".repeat(64);
        let event = build_delete_addressable(KIND_GIT_REPO_ANNOUNCEMENT, &other, "myrepo")
            .expect("builder")
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        let coord = event
            .tags
            .iter()
            .find_map(|t| (t.as_slice()[0] == "a").then(|| t.as_slice()[1].clone()));
        assert_eq!(coord, Some(format!("30617:{other}:myrepo")));
    }

    fn signed_repo(tags: Vec<Tag>, content: &str, created_at: u64) -> nostr::Event {
        EventBuilder::new(Kind::Custom(30617), content)
            .tags(tags)
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(&Keys::generate())
            .expect("sign repository event")
    }

    fn tag(parts: &[&str]) -> Tag {
        Tag::parse(parts.iter().copied()).expect("valid test tag")
    }

    #[test]
    fn protection_update_preserves_metadata_and_replaces_only_matching_pattern() {
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["name", "Demo"]),
                tag(&["buzz-channel", "channel-id"]),
                tag(&["future-metadata", "preserve-me"]),
                tag(&["auth", &"a".repeat(64), "kind=30617", &"b".repeat(128)]),
                tag(&["buzz-protect", "refs/heads/main", "push:member"]),
                tag(&["buzz-protect", "refs/tags/*", "no-delete"]),
            ],
            "repository content",
            100,
        );
        let replacement = build_protection_tag(
            "refs/heads/main",
            Some("admin"),
            ProtectionFlags {
                no_force_push: true,
                no_delete: true,
                ..ProtectionFlags::default()
            },
        )
        .expect("valid replacement");

        let updated = build_updated_repo_announcement(
            &existing,
            RepoChange::SetProtection(Box::new(replacement)),
        )
        .expect("build update")
        .sign_with_keys(&Keys::generate())
        .expect("sign update");

        assert_eq!(updated.content, "repository content");
        assert_eq!(updated.created_at.as_secs(), 101);
        assert!(!updated
            .tags
            .iter()
            .any(|tag| tag.as_slice().first().map(String::as_str) == Some("auth")));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-channel", "channel-id"]));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["future-metadata", "preserve-me"]));
        assert!(updated.tags.iter().any(|tag| {
            tag.as_slice()
                == [
                    "buzz-protect",
                    "refs/heads/main",
                    "push:admin",
                    "no-force-push",
                    "no-delete",
                ]
        }));
        assert!(updated
            .tags
            .iter()
            .any(|tag| { tag.as_slice() == ["buzz-protect", "refs/tags/*", "no-delete"] }));
        assert_eq!(
            updated
                .tags
                .iter()
                .filter(|tag| {
                    let values = tag.as_slice();
                    values.first().map(String::as_str) == Some("buzz-protect")
                        && values.get(1).map(String::as_str) == Some("refs/heads/main")
                })
                .count(),
            1
        );
    }

    #[test]
    fn protection_remove_preserves_other_patterns() {
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["buzz-protect", "refs/heads/main", "no-delete"]),
                tag(&["buzz-protect", "refs/heads/release", "push:owner"]),
            ],
            "",
            10,
        );

        let updated = build_updated_repo_announcement(
            &existing,
            RepoChange::RemoveProtection("refs/heads/main".into()),
        )
        .expect("build removal")
        .sign_with_keys(&Keys::generate())
        .expect("sign removal");

        assert!(!updated
            .tags
            .iter()
            .any(|tag| tag.as_slice().get(1).map(String::as_str) == Some("refs/heads/main")));
        assert!(updated
            .tags
            .iter()
            .any(|tag| { tag.as_slice() == ["buzz-protect", "refs/heads/release", "push:owner"] }));
    }

    #[test]
    fn protection_set_requires_at_least_one_rule() {
        assert!(build_protection_tag("refs/heads/main", None, ProtectionFlags::default()).is_err());
    }

    #[test]
    fn protection_update_rejects_malformed_existing_rules() {
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["buzz-protect", "refs/heads/main"]),
            ],
            "",
            10,
        );
        let replacement = build_protection_tag(
            "refs/heads/release",
            Some("admin"),
            ProtectionFlags::default(),
        )
        .expect("valid replacement");

        let error = build_updated_repo_announcement(
            &existing,
            RepoChange::SetProtection(Box::new(replacement)),
        )
        .expect_err("malformed existing rule must fail closed");

        assert!(error
            .to_string()
            .contains("repository contains invalid protection rules"));
    }

    #[test]
    fn protection_update_enforces_repository_rule_limit() {
        let mut tags = vec![tag(&["d", "demo"])];
        for index in 0..50 {
            tags.push(tag(&[
                "buzz-protect",
                &format!("refs/heads/branch-{index}"),
                "push:member",
            ]));
        }
        let existing = signed_repo(tags, "", 10);
        let replacement =
            build_protection_tag("refs/heads/main", Some("admin"), ProtectionFlags::default())
                .expect("valid replacement");

        let error = build_updated_repo_announcement(
            &existing,
            RepoChange::SetProtection(Box::new(replacement)),
        )
        .expect_err("the 51st rule must be rejected");

        assert!(error.to_string().contains("exceeds max 50"));
    }

    #[test]
    fn protection_list_keeps_unknown_rules_visible() {
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&[
                    "buzz-protect",
                    "refs/heads/main",
                    "push:admin",
                    "future-rule",
                ]),
            ],
            "",
            10,
        );

        let json = protection_rules_json(&existing).expect("list protections");
        assert_eq!(json["repo_id"], "demo");
        assert_eq!(json["protections"][0]["ref"], "refs/heads/main");
        assert_eq!(
            json["protections"][0]["rules"],
            serde_json::json!(["push:admin", "future-rule"])
        );
        assert_eq!(json["validation_error"], serde_json::Value::Null);
    }

    #[test]
    fn protection_list_surfaces_malformed_rules_for_recovery() {
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["buzz-protect", "refs/heads/main"]),
            ],
            "",
            10,
        );

        let json = protection_rules_json(&existing).expect("list malformed protections");
        assert_eq!(json["protections"][0]["ref"], "refs/heads/main");
        assert!(json["validation_error"]
            .as_str()
            .is_some_and(|error| error.contains("needs pattern + at least one rule")));
    }

    #[test]
    fn bind_channel_replaces_duplicates_and_preserves_everything_else() {
        let channel = uuid::Uuid::new_v4().to_string();
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["name", "Demo"]),
                // Two stale bindings — e.g. from a buggy or vanilla client.
                tag(&["buzz-channel", "old-and-broken"]),
                tag(&["buzz-channel", &uuid::Uuid::new_v4().to_string()]),
                tag(&["auth", &"a".repeat(64), "kind=30617", &"b".repeat(128)]),
                tag(&["buzz-protect", "refs/heads/main", "push:admin"]),
                tag(&["future-metadata", "preserve-me"]),
            ],
            "repository content",
            100,
        );

        let updated =
            build_updated_repo_announcement(&existing, RepoChange::BindChannel(channel.clone()))
                .expect("build bind update")
                .sign_with_keys(&Keys::generate())
                .expect("sign bind update");

        assert_eq!(updated.content, "repository content");
        assert_eq!(updated.created_at.as_secs(), 101);
        // Exactly one binding remains, and it is the requested one.
        let bindings: Vec<_> = updated
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("buzz-channel"))
            .collect();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].as_slice(), ["buzz-channel", channel.as_str()]);
        // Auth stripped (relay re-stamps); everything else preserved.
        assert!(!updated
            .tags
            .iter()
            .any(|tag| tag.as_slice().first().map(String::as_str) == Some("auth")));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-protect", "refs/heads/main", "push:admin"]));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["future-metadata", "preserve-me"]));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["name", "Demo"]));
    }

    #[test]
    fn bind_channel_adds_binding_to_unbound_repo() {
        let channel = uuid::Uuid::new_v4().to_string();
        let existing = signed_repo(vec![tag(&["d", "demo"])], "", 10);

        let updated =
            build_updated_repo_announcement(&existing, RepoChange::BindChannel(channel.clone()))
                .expect("build bind update")
                .sign_with_keys(&Keys::generate())
                .expect("sign bind update");

        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-channel", channel.as_str()]));
    }

    #[test]
    fn bind_channel_rejects_malformed_uuid() {
        let existing = signed_repo(vec![tag(&["d", "demo"])], "", 10);

        let error =
            build_updated_repo_announcement(&existing, RepoChange::BindChannel("nope".into()))
                .expect_err("malformed channel id must not build an update");

        assert!(matches!(error, crate::error::CliError::Usage(_)));
    }

    #[test]
    fn link_project_replaces_duplicates_and_leaves_the_channel_binding_alone() {
        let channel = uuid::Uuid::new_v4().to_string();
        let owner = Keys::generate().public_key().to_hex();
        let coordinate = format!("30621:{owner}:new-project");
        let existing = signed_repo(
            vec![
                tag(&["d", "demo"]),
                tag(&["project", &format!("30621:{owner}:stale-one")]),
                tag(&["project", &format!("30621:{owner}:stale-two")]),
                tag(&["buzz-channel", &channel]),
                tag(&["buzz-protect", "refs/heads/main", "push:admin"]),
            ],
            "",
            10,
        );

        let updated =
            build_updated_repo_announcement(&existing, RepoChange::LinkProject(coordinate.clone()))
                .expect("build link update")
                .sign_with_keys(&Keys::generate())
                .expect("sign link update");

        let links: Vec<_> = updated
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("project"))
            .collect();
        assert_eq!(links.len(), 1, "exactly one project tag survives");
        assert_eq!(links[0].as_slice(), ["project", coordinate.as_str()]);
        // The two ACLs are independent: relinking must not silently revoke
        // the channel's access.
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-channel", channel.as_str()]));
        assert!(updated
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-protect", "refs/heads/main", "push:admin"]));
    }

    #[test]
    fn link_project_rejects_a_malformed_coordinate() {
        let existing = signed_repo(vec![tag(&["d", "demo"])], "", 10);

        let error =
            build_updated_repo_announcement(&existing, RepoChange::LinkProject("nope".into()))
                .expect_err("malformed coordinate must not build an update");

        assert!(matches!(error, crate::error::CliError::Usage(_)));
    }

    /// Issue #3527: `repos create --channel` must emit exactly one
    /// `buzz-channel` tag so the primary create command stops producing
    /// repos the relay 404s forever.
    #[test]
    fn create_with_channel_emits_exactly_one_binding_tag() {
        let channel = uuid::Uuid::new_v4().to_string();
        let event = build_create_announcement(
            "demo",
            Some("Demo"),
            None,
            &["https://relay.example/git/owner/demo".to_string()],
            None,
            &[],
            Some(&channel),
            None,
        )
        .expect("build create announcement")
        .sign_with_keys(&Keys::generate())
        .expect("sign create announcement");

        assert_eq!(event.kind, Kind::Custom(30617));
        let bindings: Vec<_> = event
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("buzz-channel"))
            .collect();
        assert_eq!(bindings.len(), 1, "exactly one buzz-channel tag");
        assert_eq!(bindings[0].as_slice(), ["buzz-channel", channel.as_str()]);
        // The standard metadata still rides along.
        assert!(event.tags.iter().any(|tag| tag.as_slice() == ["d", "demo"]));
        assert!(event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["name", "Demo"]));
    }

    #[test]
    fn create_without_channel_emits_no_binding_tag() {
        let event = build_create_announcement("demo", None, None, &[], None, &[], None, None)
            .expect("build create announcement")
            .sign_with_keys(&Keys::generate())
            .expect("sign create announcement");

        assert!(
            !event
                .tags
                .iter()
                .any(|tag| tag.as_slice().first().map(String::as_str) == Some("buzz-channel")),
            "no --channel means no binding tag (vanilla NIP-34 stays possible)"
        );
    }

    #[test]
    fn create_rejects_malformed_channel_uuid() {
        let error =
            build_create_announcement("demo", None, None, &[], None, &[], Some("nope"), None)
                .expect_err("malformed channel id must not build an announcement");
        assert!(matches!(error, crate::error::CliError::Usage(_)));
    }

    /// `repos create --project` is the channel-less path: the project's
    /// roster is the ACL, so no `buzz-channel` tag is emitted at all.
    #[test]
    fn create_with_project_emits_one_normalized_project_tag_and_no_channel() {
        let owner = Keys::generate().public_key().to_hex();
        // Mixed case in: the relay stores and compares lowercase, so the tag
        // must go out normalized or the coordinate never matches an ACL row.
        let coordinate = format!("30621:{}:Demo-Project", owner.to_uppercase());
        let event =
            build_create_announcement("demo", None, None, &[], None, &[], None, Some(&coordinate))
                .expect("build create announcement")
                .sign_with_keys(&Keys::generate())
                .expect("sign create announcement");

        let links: Vec<_> = event
            .tags
            .iter()
            .filter(|tag| tag.as_slice().first().map(String::as_str) == Some("project"))
            .collect();
        assert_eq!(links.len(), 1, "exactly one project tag");
        assert_eq!(
            links[0].as_slice(),
            ["project", &format!("30621:{owner}:Demo-Project")]
        );
        assert!(
            !event
                .tags
                .iter()
                .any(|tag| tag.as_slice().first().map(String::as_str) == Some("buzz-channel")),
            "a project-scoped repo needs no channel binding"
        );
    }

    #[test]
    fn create_accepts_both_acls_together() {
        let channel = uuid::Uuid::new_v4().to_string();
        let coordinate = format!("30621:{}:proj", Keys::generate().public_key().to_hex());
        let event = build_create_announcement(
            "demo",
            None,
            None,
            &[],
            None,
            &[],
            Some(&channel),
            Some(&coordinate),
        )
        .expect("build create announcement")
        .sign_with_keys(&Keys::generate())
        .expect("sign create announcement");

        assert!(event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["buzz-channel", channel.as_str()]));
        assert!(event
            .tags
            .iter()
            .any(|tag| tag.as_slice() == ["project", coordinate.as_str()]));
    }

    #[test]
    fn create_rejects_malformed_project_coordinate() {
        for bad in [
            "not-a-coordinate",
            "30621:short:proj",
            // 30617 is a repo announcement, not a project.
            "30617:0000000000000000000000000000000000000000000000000000000000000000:proj",
            // Empty d-tag.
            "30621:0000000000000000000000000000000000000000000000000000000000000000:",
        ] {
            let error =
                build_create_announcement("demo", None, None, &[], None, &[], None, Some(bad))
                    .expect_err("malformed coordinate must not build an announcement");
            assert!(
                matches!(error, crate::error::CliError::Usage(_)),
                "{bad:?} must be a usage error"
            );
        }
    }

    #[test]
    fn duplicate_write_response_is_a_conflict() {
        let error = validate_write_response(
            r#"{"event_id":"abc","accepted":true,"message":"duplicate: superseded"}"#,
        )
        .expect_err("dominated writes must not report success");

        assert!(matches!(error, crate::error::CliError::Conflict(_)));
    }

    #[test]
    fn successful_write_response_is_normalized() {
        let output = validate_write_response(
            r#"{"event_id":"abc","accepted":true,"message":"saved","extra":"ignored"}"#,
        )
        .expect("accepted write");

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&output).expect("normalized JSON"),
            serde_json::json!({
                "event_id": "abc",
                "accepted": true,
                "message": "saved",
            })
        );
    }

    /// `--require-verdict` writes the token the relay's gate reads. The rule
    /// is disclosed rather than glossed: a relay predating it parses the token
    /// into `unknown_rules` and ignores it, and `protect list` shows both.
    #[test]
    fn require_verdict_is_written_as_a_rule_token_and_listed() {
        let tag = build_protection_tag(
            "refs/heads/main",
            None,
            ProtectionFlags {
                require_verdict: true,
                ..ProtectionFlags::default()
            },
        )
        .expect("the rule is valid");
        assert_eq!(
            tag.as_slice(),
            ["buzz-protect", "refs/heads/main", "require-verdict"]
        );

        let event = EventBuilder::new(Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
            .tags([Tag::parse(["d", "beekeeper"]).unwrap(), tag])
            .sign_with_keys(&Keys::generate())
            .expect("sign");
        let listed = protection_rules_json(&event).expect("rules render");
        assert_eq!(
            listed["protections"][0]["rules"][0], "require-verdict",
            "protect list shows the rule: {listed}"
        );
        assert_eq!(
            listed["unknown_rules"].as_array().map(Vec::len),
            Some(0),
            "this build knows the rule; an older relay is the one that ignores it"
        );
    }
}
