use beekeeper_core::{
    git_perms::{parse_protection_tag, parse_protection_tags, RefPattern},
    kind::KIND_GIT_REPO_ANNOUNCEMENT,
    repository_founders::RepositoryFounders,
};
use beekeeper_sdk::build_delete_addressable;
use nostr::{Event, EventBuilder, Tag, Timestamp};

use crate::client::BeekeeperClient;
use crate::commands::parse_write_response;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_repo_id};

fn parse_events(json: &str) -> Result<Vec<Event>, CliError> {
    serde_json::from_str(json)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))
}

async fn fetch_own_repo_announcement(
    client: &BeekeeperClient,
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
    /// Replace the NIP-34 `maintainers` tag with exactly one carrying these
    /// pubkeys — or, given none, remove it. Every listed key becomes a
    /// **founder** of the repository (finding 33), so this is an authority
    /// change and never a merge: the tag as written is the whole list.
    SetMaintainers(Vec<String>),
}

/// The NIP-34 tag naming co-maintainers, re-exported from `buzz-core` so the
/// writer and the reader cannot spell it differently.
const MAINTAINERS_TAG: &str = beekeeper_core::repository_founders::REPOSITORY_MAINTAINERS_TAG;

/// One `["maintainers", <hex>, …]` tag, or `None` for "remove the tag".
///
/// Every value is validated as lower 64-hex here rather than at the gate: a
/// mistyped key would silently not be a founder, and the CLI is the last place
/// that can say so out loud.
fn build_maintainers_tag(maintainers: &[String]) -> Result<Option<Tag>, CliError> {
    let mut values: Vec<String> = vec![MAINTAINERS_TAG.to_string()];
    for maintainer in maintainers {
        let normalized = maintainer.trim().to_ascii_lowercase();
        validate_lower_hex64("maintainer", &normalized)?;
        if !values.iter().skip(1).any(|value| value == &normalized) {
            values.push(normalized);
        }
    }
    if values.len() == 1 {
        return Ok(None);
    }
    Ok(Some(Tag::parse(values).map_err(tag_error)?))
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
        RepoChange::SetMaintainers(maintainers) => {
            let tag = build_maintainers_tag(&maintainers)?;
            (None, Some(MAINTAINERS_TAG), tag)
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

    let next_created_at =
        next_replaceable_created_at(existing.created_at.as_secs(), Timestamp::now().as_secs())
            .ok_or_else(|| CliError::Other("repository timestamp cannot be advanced".into()))?;
    beekeeper_sdk::build_repo_announcement_with_tags(repo_id, &existing.content, tags)
        .map_err(|error| CliError::Other(format!("failed to build repository update: {error}")))
        .map(|builder| builder.custom_created_at(Timestamp::from(next_created_at)))
}

/// The `created_at` for a rewrite of an addressable event whose observed head
/// was stamped `head_secs`.
///
/// The rewrite must sort after the head, or NIP-33 last-write-wins keeps the
/// old copy, so it is never below `head_secs + 1`. It must also sit inside the
/// relay's ±15-minute ingest window, or the relay refuses it as "event
/// timestamp too far from server time" — which a bare `head + 1` fails for
/// every head older than fifteen minutes. So the answer is the later of the
/// two. A head stamped in the future (a delayed writer racing an intervening
/// update) is still advanced past, never leapfrogged.
pub(crate) fn next_replaceable_created_at(head_secs: u64, now_secs: u64) -> Option<u64> {
    head_secs.checked_add(1).map(|bumped| bumped.max(now_secs))
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

async fn submit_repo_update(
    client: &BeekeeperClient,
    builder: EventBuilder,
) -> Result<(), CliError> {
    submit_repo_update_with(client, builder, None).await
}

/// Submit a repository update, optionally disclosing the founder set the
/// change leaves behind.
///
/// `founders` is added as an extra key on the write response rather than
/// folded into `message`: the `{event_id, accepted, message}` shape every
/// agent parses is unchanged, and the disclosure is additive.
async fn submit_repo_update_with(
    client: &BeekeeperClient,
    builder: EventBuilder,
    founders: Option<String>,
) -> Result<(), CliError> {
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    let response = validate_write_response(&raw)?;
    let Some(founders) = founders else {
        println!("{response}");
        return Ok(());
    };
    let mut value: serde_json::Value = serde_json::from_str(&response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("founders".to_string(), serde_json::Value::String(founders));
    }
    println!("{value}");
    Ok(())
}

/// Validate a `30621:<owner-hex>:<dtag>` project coordinate and return it in
/// the normalized form the relay stores.
///
/// Shares [`beekeeper_core::kind::normalize_project_coordinate`] with the relay's
/// ingest validation and gate lookups, so a coordinate this CLI accepts is
/// exactly one they can resolve.
fn validate_project_coordinate(coordinate: &str) -> Result<String, CliError> {
    beekeeper_core::kind::normalize_project_coordinate(coordinate).ok_or_else(|| {
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
pub(crate) fn build_create_announcement(
    repo_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    clone_urls: &[String],
    web_url: Option<&str>,
    relays: &[String],
    channel: Option<&str>,
    project: Option<&str>,
    maintainers: &[String],
) -> Result<EventBuilder, CliError> {
    validate_repo_id(repo_id)?;

    let clone_refs: Vec<&str> = clone_urls.iter().map(|s| s.as_str()).collect();
    let relay_refs: Vec<&str> = relays.iter().map(|s| s.as_str()).collect();

    let mut builder = beekeeper_sdk::build_repo_announcement(
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
    // Co-founders, in the standard NIP-34 tag. Buzz's announcement builder
    // never emitted one before finding 33, so every repository announced
    // before this had exactly one founder whether or not it had one owner.
    if let Some(tag) = build_maintainers_tag(maintainers)? {
        builder = builder.tag(tag);
    }
    Ok(builder)
}

/// The announcement `bee packs init` (and the app's *Create packs repository*)
/// publishes for a project's packs repository.
///
/// Written here rather than in `packs.rs` so a packs repository is announced by
/// the *same* builder as every other repository — a second builder would be a
/// second place for the `project` back-reference to be spelled, and that tag is
/// what puts the repository inside the project's ACL and founder set.
///
/// # Errors
/// [`CliError::Usage`] for an invalid repository id or project coordinate.
pub fn build_packs_repo_announcement(
    repo_id: &str,
    name: &str,
    clone_url: &str,
    project: &str,
) -> Result<EventBuilder, CliError> {
    build_create_announcement(
        repo_id,
        Some(name),
        Some("Role packs for this project (NIP-PK, kind 30624)"),
        &[clone_url.to_string()],
        None,
        &[],
        None,
        Some(project),
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub async fn cmd_create_repo(
    client: &BeekeeperClient,
    repo_id: &str,
    name: Option<&str>,
    description: Option<&str>,
    clone_urls: &[String],
    web_url: Option<&str>,
    relays: &[String],
    channel: Option<&str>,
    project: Option<&str>,
    maintainers: &[String],
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
        maintainers,
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
    client: &BeekeeperClient,
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
    // Each announcement is returned exactly as the relay served it, plus the
    // derived `founders` list and the sentence that says who may rewrite the
    // rules and whether the project roster was readable from here. Finding 33
    // was invisible precisely because nothing ever printed this set.
    let mut rows: Vec<serde_json::Value> = serde_json::from_str(&resp)
        .map_err(|error| CliError::Other(format!("failed to parse relay response: {error}")))?;
    for row in &mut rows {
        let signer = row
            .get("pubkey")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let tags: Vec<Vec<String>> = row
            .get("tags")
            .and_then(serde_json::Value::as_array)
            .map(|tags| {
                tags.iter()
                    .filter_map(|tag| {
                        tag.as_array().map(|values| {
                            values
                                .iter()
                                .filter_map(|value| {
                                    value.as_str().map(std::string::ToString::to_string)
                                })
                                .collect()
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let founders = repository_founders_from_parts(client, &signer, &tags).await;
        if let Some(object) = row.as_object_mut() {
            object.insert(
                "founders".to_string(),
                serde_json::Value::Array(
                    founders
                        .pubkeys()
                        .iter()
                        .map(|pubkey| serde_json::Value::String(pubkey.clone()))
                        .collect(),
                ),
            );
            object.insert(
                "founders_note".to_string(),
                serde_json::Value::String(founders.rules_sentence()),
            );
        }
    }
    println!("{}", serde_json::Value::Array(rows));
    Ok(())
}

pub async fn cmd_list_repos(
    client: &BeekeeperClient,
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

/// Who founds this repository, as far as this client can read it.
///
/// Finding 33: the announcement's signer is one founder, not the founder. The
/// signed half (signer ∪ NIP-34 `maintainers`) needs nothing but the event;
/// the roster half needs the project the `["project", …]` back-reference names
/// and is read over the wire. A roster this client could not read leaves the
/// set marked unread, and
/// [`beekeeper_core::repository_founders::RepositoryFounders::rules_sentence`] says
/// so — a partial set presented as whole is exactly the shape of finding 33.
pub(crate) async fn repository_founders(
    client: &BeekeeperClient,
    announcement: &Event,
) -> RepositoryFounders {
    let tags: Vec<Vec<String>> = announcement
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    repository_founders_from_parts(client, &announcement.pubkey.to_hex(), &tags).await
}

/// The same derivation for a caller holding the announcement as raw JSON.
///
/// `bee repos get` prints the relay's own bytes and must not re-serialize a
/// parsed event to do it — the relay strips signatures on reads, and a
/// round-trip through `nostr::Event` would put one back.
pub(crate) async fn repository_founders_from_parts(
    client: &BeekeeperClient,
    signer_pubkey: &str,
    tags: &[Vec<String>],
) -> RepositoryFounders {
    let founders = RepositoryFounders::from_parts(signer_pubkey, tags);
    let coordinate = tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value, ..] if name == "project" => {
            beekeeper_core::kind::normalize_project_coordinate(value)
        }
        _ => None,
    });
    let Some(coordinate) = coordinate else {
        // No project back-reference: there is no roster, so nothing is missing.
        return founders.with_roster_owners(Vec::new());
    };
    match crate::commands::projects::project_owner_pubkeys(client, &coordinate).await {
        Ok(owners) => founders.with_roster_owners(owners),
        Err(_) => founders,
    }
}

async fn current_repo(client: &BeekeeperClient, repo_id: &str) -> Result<Event, CliError> {
    validate_repo_id(repo_id)?;
    fetch_own_repo_announcement(client, repo_id)
        .await?
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "repository {repo_id:?} was not found for the current identity"
            ))
        })
}

/// `bee repos protect list` — the rules that govern the repository, and which
/// record carries each one.
///
/// Reads by repository id whoever announced it (a co-founder's listing used to
/// be a `NotFound`), resolves the announcement's own rows against every
/// founder's rule record, and prints the decision per pattern: the rule, the
/// record, and the key that signed it. A repository with no rule record prints
/// exactly what it always did, with `"record": "announcement"` on every row.
async fn cmd_protect_list(client: &BeekeeperClient, repo_id: &str) -> Result<(), CliError> {
    let rules = crate::commands::repos_protection::read_repository_rules(client, repo_id).await?;
    let event = rules.announcement.clone();
    let mut listing = protection_rules_json(&event)?;
    if let Some(object) = listing.as_object_mut() {
        // The *governing* rules, resolved across records — as against
        // `protections`, which stays what the announcement's own tags say so
        // nothing that parsed it loses its shape.
        object.insert(
            "governing".to_string(),
            serde_json::Value::Array(
                rules
                    .resolved
                    .decisions()
                    .iter()
                    .map(|decision| {
                        crate::commands::repos_protection::decision_json(&rules, decision)
                    })
                    .collect(),
            ),
        );
        object.insert(
            "rule_records_read".to_string(),
            serde_json::Value::from(rules.records_read),
        );
        object.insert(
            "rule_records_ignored_non_founder".to_string(),
            serde_json::Value::from(rules.records_from_non_founders),
        );
        if rules.records_read >= crate::commands::repos_protection::PROTECTION_MAX_RECORDS {
            object.insert(
                "rule_records_bound".to_string(),
                serde_json::Value::String(format!(
                    "the newest {} rule records were read; an older founder's record may be off \
                     this page",
                    crate::commands::repos_protection::PROTECTION_MAX_RECORDS
                )),
            );
        }
    }
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
        // Printed beside `relay_url` (finding 32,
        // review-2026-09-01/LIVE-RUN-TeamRolesV1.md): "unknown" for a relay
        // predating NIP-11's software_commit field is itself the disclosure.
        object.insert(
            "relay_commit".to_string(),
            serde_json::Value::String(
                crate::commands::git_setup::serving_relay_commit(client.relay_url()).await,
            ),
        );
        // Rules and founders are two different authorities and the difference
        // bites: the founder set governs which missions rule and who may land,
        // while the rules themselves live on the announcement and only its
        // signer can rewrite them in v1.
        object.insert(
            "founders".to_string(),
            serde_json::Value::String(rules.founders.rules_sentence()),
        );
    }
    println!("{listing}");
    Ok(())
}

/// `bee repos protect set` — set a rule on a repository you founded.
///
/// The announcement's signer rewrites the announcement, exactly as before.
/// Any other founder signs a rule record instead (finding 33 R2): before lane
/// L26 this command answered them with `NotFound`, or — given the same
/// repository id under their own key — silently published a second repository.
async fn cmd_protect_set(
    client: &BeekeeperClient,
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
    // L24's `relay_commit` disclosure and L26's two records, together: which
    // record the rule landed in, and which relay build was serving when it
    // did. Fetched once, before either branch, because both print it.
    let relay_commit = crate::commands::git_setup::serving_relay_commit(client.relay_url()).await;
    let rules = crate::commands::repos_protection::read_repository_rules(client, repo_id).await?;
    let me = client.keys().public_key().to_hex();
    guard_founder(&rules, &me)?;
    let founders = rules.founders.rules_sentence();
    match rules.writable_record(&me) {
        crate::commands::repos_protection::WritableRecord::Announcement => {
            let builder = build_updated_repo_announcement(
                &rules.announcement,
                RepoChange::SetProtection(Box::new(tag)),
            )?;
            submit_repo_update_with_record(
                client,
                builder,
                founders,
                crate::commands::repos_protection::WritableRecord::Announcement.label(),
                relay_commit,
            )
            .await
        }
        crate::commands::repos_protection::WritableRecord::RuleRecord => {
            let row: Vec<String> = tag.as_slice()[1..].to_vec();
            write_rule_record(
                client,
                &rules,
                ref_pattern,
                Some(row),
                founders,
                relay_commit,
            )
            .await
        }
    }
}

/// Refuse early, and say who may, rather than letting the relay's 403 be the
/// first news that this key does not found the repository.
fn guard_founder(
    rules: &crate::commands::repos_protection::RepositoryRules,
    pubkey: &str,
) -> Result<(), CliError> {
    if rules.may_set_rules(pubkey) {
        return Ok(());
    }
    Err(CliError::Usage(format!(
        "only a founder of this repository may set its rules; {}",
        rules.founders.rules_sentence()
    )))
}

/// Publish the caller's own rule record, stamped past whatever currently wins
/// the pattern so the write actually takes effect.
async fn write_rule_record(
    client: &BeekeeperClient,
    rules: &crate::commands::repos_protection::RepositoryRules,
    ref_pattern: &str,
    row: Option<Vec<String>>,
    founders: String,
    relay_commit: String,
) -> Result<(), CliError> {
    let owner_hex = rules.announcement.pubkey.to_hex();
    let repo_id = repo_id_from_event(&rules.announcement)?.to_string();
    let current =
        crate::commands::repos_protection::fetch_own_rule_record(client, &owner_hex, &repo_id)
            .await?;
    let rows = crate::commands::repos_protection::next_rule_record_rows(
        current.as_ref(),
        ref_pattern,
        row,
    );
    let head = rules
        .winning_created_at(ref_pattern)
        .max(current.map(|event| event.created_at.as_secs()).unwrap_or(0));
    let created_at = next_replaceable_created_at(head, Timestamp::now().as_secs())
        .ok_or_else(|| CliError::Other("rule record timestamp cannot be advanced".into()))?;
    let raw = crate::commands::repos_protection::publish_rule_record(
        client, &owner_hex, &repo_id, &rows, created_at,
    )
    .await?;
    let response = validate_write_response(&raw)?;
    print_with_record(
        &response,
        founders,
        crate::commands::repos_protection::WritableRecord::RuleRecord.label(),
        relay_commit,
    )
}

/// Print a write response with the founder sentence, the record it landed
/// in, and the relay build that accepted it. Which record it was is not
/// decoration: "your rule is live" and "your rule is live in a second record
/// the relay resolves against the announcement" are different facts — and
/// `relay_commit` (finding 32) says which build did the resolving, `unknown`
/// included.
fn print_with_record(
    response: &str,
    founders: String,
    record: &'static str,
    relay_commit: String,
) -> Result<(), CliError> {
    let mut value: serde_json::Value = serde_json::from_str(response)
        .map_err(|error| CliError::Other(format!("relay response is not JSON: {error}")))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("founders".to_string(), serde_json::Value::String(founders));
        object.insert(
            "record".to_string(),
            serde_json::Value::String(record.to_string()),
        );
        object.insert(
            "relay_commit".to_string(),
            serde_json::Value::String(relay_commit),
        );
    }
    println!("{value}");
    Ok(())
}

/// `submit_repo_update_with`, plus the record the write landed in and the
/// relay build that took it.
async fn submit_repo_update_with_record(
    client: &BeekeeperClient,
    builder: EventBuilder,
    founders: String,
    record: &'static str,
    relay_commit: String,
) -> Result<(), CliError> {
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    let response = validate_write_response(&raw)?;
    print_with_record(&response, founders, record, relay_commit)
}

/// `bee repos update` — change who co-founds one of your repositories.
///
/// Writing the `maintainers` tag is an authority change: every listed key
/// becomes a founder, whose missions can rule on this repository and who may
/// land a verdict-gated ref. The list is replaced whole, never merged, so what
/// the command prints is what the announcement now says.
pub async fn cmd_update_repo(
    client: &BeekeeperClient,
    repo_id: &str,
    maintainers: &[String],
    clear_maintainers: bool,
) -> Result<(), CliError> {
    if maintainers.is_empty() && !clear_maintainers {
        return Err(CliError::Usage(
            "nothing to update: pass --maintainer <hex> (repeatable) or --clear-maintainers".into(),
        ));
    }
    if !maintainers.is_empty() && clear_maintainers {
        return Err(CliError::Usage(
            "--clear-maintainers cannot be combined with --maintainer".into(),
        ));
    }
    let event = current_repo(client, repo_id).await?;
    let builder =
        build_updated_repo_announcement(&event, RepoChange::SetMaintainers(maintainers.to_vec()))?;
    // The sentence is derived from what is being written, not from the
    // announcement that is being replaced.
    let mut updated_tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .filter(|tag| tag.first().map(String::as_str) != Some(MAINTAINERS_TAG))
        .collect();
    if let Some(tag) = build_maintainers_tag(maintainers)? {
        updated_tags.push(tag.as_slice().to_vec());
    }
    let founders = beekeeper_core::repository_founders::RepositoryFounders::from_parts(
        &event.pubkey.to_hex(),
        &updated_tags,
    );
    submit_repo_update_with(client, builder, Some(founders.rules_sentence())).await
}

/// `bee repos protect remove` — take a rule off a ref.
///
/// The signer drops the tag from the announcement. Any other founder signs a
/// rule record carrying the `none` token for that pattern, which wins the
/// pattern and leaves the ref governed by the built-in defaults. Dropping
/// their own row instead would fall back to the announcement's rule, which is
/// the opposite of what "remove" was asked to do.
async fn cmd_protect_remove(
    client: &BeekeeperClient,
    repo_id: &str,
    ref_pattern: &str,
) -> Result<(), CliError> {
    RefPattern::parse(ref_pattern)
        .map_err(|error| CliError::Usage(format!("invalid ref pattern: {error}")))?;
    let rules = crate::commands::repos_protection::read_repository_rules(client, repo_id).await?;
    let me = client.keys().public_key().to_hex();
    guard_founder(&rules, &me)?;
    let governed = rules
        .resolved
        .decision_for(ref_pattern)
        .is_some_and(|decision| !decision.cleared);
    if !governed {
        return Err(CliError::NotFound(format!(
            "repository {repo_id:?} has no protection rule for {ref_pattern:?}"
        )));
    }
    let founders = rules.founders.rules_sentence();
    // Same disclosure `protect set` carries: removing a rule is as much a
    // signed act as setting one, and which relay build resolved it is the
    // same question (finding 32).
    let relay_commit = crate::commands::git_setup::serving_relay_commit(client.relay_url()).await;
    match rules.writable_record(&me) {
        crate::commands::repos_protection::WritableRecord::Announcement => {
            let builder = build_updated_repo_announcement(
                &rules.announcement,
                RepoChange::RemoveProtection(ref_pattern.to_string()),
            )?;
            submit_repo_update_with_record(
                client,
                builder,
                founders,
                crate::commands::repos_protection::WritableRecord::Announcement.label(),
                relay_commit,
            )
            .await
        }
        crate::commands::repos_protection::WritableRecord::RuleRecord => {
            write_rule_record(client, &rules, ref_pattern, None, founders, relay_commit).await
        }
    }
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
    client: &BeekeeperClient,
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
    client: &BeekeeperClient,
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

pub async fn dispatch(cmd: crate::ReposCmd, client: &BeekeeperClient) -> Result<(), CliError> {
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
            maintainers,
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
                &maintainers,
            )
            .await
        }
        ReposCmd::Update {
            id,
            maintainers,
            clear_maintainers,
        } => cmd_update_repo(client, &id, &maintainers, clear_maintainers).await,
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
        build_updated_repo_announcement, next_replaceable_created_at, protection_rules_json,
        validate_write_response, CliError, ProtectionFlags, RepoChange, RepositoryFounders,
        KIND_GIT_REPO_ANNOUNCEMENT, MAINTAINERS_TAG,
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
    fn rewrite_timestamp_is_the_later_of_head_plus_one_and_now() {
        // A stale head (the common case: the announcement is days old) is
        // stamped now, inside the relay's ingest window.
        assert_eq!(
            next_replaceable_created_at(100, 1_700_000_000),
            Some(1_700_000_000)
        );
        // A head at or ahead of the clock is still advanced past, never
        // leapfrogged, so last-write-wins keeps this rewrite.
        assert_eq!(
            next_replaceable_created_at(1_700_000_000, 1_700_000_000),
            Some(1_700_000_001)
        );
        assert_eq!(
            next_replaceable_created_at(1_700_000_500, 1_700_000_000),
            Some(1_700_000_501)
        );
        assert_eq!(next_replaceable_created_at(u64::MAX, 1_700_000_000), None);
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

        let before = Timestamp::now().as_secs();
        let updated = build_updated_repo_announcement(
            &existing,
            RepoChange::SetProtection(Box::new(replacement)),
        )
        .expect("build update")
        .sign_with_keys(&Keys::generate())
        .expect("sign update");

        assert_eq!(updated.content, "repository content");
        assert!(
            updated.created_at.as_secs() >= before,
            "rewrite must be stamped now"
        );
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

        let before = Timestamp::now().as_secs();
        let updated =
            build_updated_repo_announcement(&existing, RepoChange::BindChannel(channel.clone()))
                .expect("build bind update")
                .sign_with_keys(&Keys::generate())
                .expect("sign bind update");

        assert_eq!(updated.content, "repository content");
        assert!(
            updated.created_at.as_secs() >= before,
            "rewrite must be stamped now"
        );
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
            &[],
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
        let event = build_create_announcement("demo", None, None, &[], None, &[], None, None, &[])
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
            build_create_announcement("demo", None, None, &[], None, &[], Some("nope"), None, &[])
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
        let event = build_create_announcement(
            "demo",
            None,
            None,
            &[],
            None,
            &[],
            None,
            Some(&coordinate),
            &[],
        )
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
            &[],
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
                build_create_announcement("demo", None, None, &[], None, &[], None, Some(bad), &[])
                    .expect_err("malformed coordinate must not build an announcement");
            assert!(
                matches!(error, crate::error::CliError::Usage(_)),
                "{bad:?} must be a usage error"
            );
        }
    }

    // ── finding 33: co-founders on the announcement ──────────────────

    /// `--maintainer` writes exactly one NIP-34 tag, deduped and lowercased,
    /// and `RepositoryFounders` reads the two founders back out of it.
    #[test]
    fn create_with_maintainers_emits_one_deduped_lowercase_tag() {
        let brian = "3d".repeat(32);
        let andy = Keys::generate();
        let event = build_create_announcement(
            "demo",
            None,
            None,
            &[],
            None,
            &[],
            None,
            None,
            &[brian.to_uppercase(), brian.clone(), format!("  {brian}  ")],
        )
        .expect("build create announcement")
        .sign_with_keys(&andy)
        .expect("sign create announcement");

        let tags: Vec<&[String]> = event
            .tags
            .iter()
            .map(|tag| tag.as_slice())
            .filter(|tag| tag.first().map(String::as_str) == Some(MAINTAINERS_TAG))
            .collect();
        assert_eq!(tags.len(), 1, "exactly one maintainers tag: {tags:?}");
        assert_eq!(tags[0], ["maintainers".to_string(), brian.clone()]);

        let founders = RepositoryFounders::from_announcement(&event);
        assert_eq!(
            founders.pubkeys(),
            &[andy.public_key().to_hex(), brian],
            "signer first, then the maintainer"
        );
    }

    /// A maintainer that is not 64-hex is a usage error at the writer, not a
    /// key that quietly founds nothing.
    #[test]
    fn a_malformed_maintainer_is_refused_at_the_writer() {
        for bad in ["nope", &"ab".repeat(31), &format!("{}z", "ab".repeat(31))] {
            let error = build_create_announcement(
                "demo",
                None,
                None,
                &[],
                None,
                &[],
                None,
                None,
                &[bad.to_string()],
            )
            .expect_err("a malformed maintainer must not build an announcement");
            assert!(matches!(error, CliError::Usage(_)), "{bad:?}: {error:?}");
        }
    }

    /// `repos update --maintainer` replaces the tag whole rather than merging,
    /// so the printed founder set is what the announcement now says.
    #[test]
    fn updating_maintainers_replaces_the_tag_whole() {
        let owner = Keys::generate();
        let old = "aa".repeat(32);
        let new = "bb".repeat(32);
        let existing =
            EventBuilder::new(nostr::Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
                .tags([
                    Tag::parse(["d", "demo"]).expect("d"),
                    Tag::parse(["maintainers", &old]).expect("maintainers"),
                ])
                .sign_with_keys(&owner)
                .expect("sign existing");

        let updated = build_updated_repo_announcement(
            &existing,
            RepoChange::SetMaintainers(vec![new.clone()]),
        )
        .expect("build update")
        .sign_with_keys(&owner)
        .expect("sign update");
        let founders = RepositoryFounders::from_announcement(&updated);
        assert_eq!(founders.pubkeys(), &[owner.public_key().to_hex(), new]);
        assert!(
            !founders.contains(&old),
            "the previous maintainer is gone, not merged"
        );
    }

    /// `--clear-maintainers` removes the tag, leaving the signer alone.
    #[test]
    fn clearing_maintainers_removes_the_tag() {
        let owner = Keys::generate();
        let existing =
            EventBuilder::new(nostr::Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
                .tags([
                    Tag::parse(["d", "demo"]).expect("d"),
                    Tag::parse(["maintainers", &"aa".repeat(32)]).expect("maintainers"),
                ])
                .sign_with_keys(&owner)
                .expect("sign existing");

        let updated =
            build_updated_repo_announcement(&existing, RepoChange::SetMaintainers(Vec::new()))
                .expect("build update")
                .sign_with_keys(&owner)
                .expect("sign update");
        assert!(
            !updated
                .tags
                .iter()
                .any(|tag| tag.as_slice().first().map(String::as_str) == Some(MAINTAINERS_TAG)),
            "no maintainers tag survives a clear"
        );
        assert_eq!(
            RepositoryFounders::from_announcement(&updated).len(),
            1,
            "the signer is the only founder the announcement names"
        );
    }

    /// Setting maintainers leaves every other tag — the binding, the project
    /// back-reference, the protection rules — exactly as it found them.
    #[test]
    fn setting_maintainers_preserves_every_other_tag() {
        let owner = Keys::generate();
        let channel = uuid::Uuid::new_v4().to_string();
        let existing =
            EventBuilder::new(nostr::Kind::Custom(KIND_GIT_REPO_ANNOUNCEMENT as u16), "")
                .tags([
                    Tag::parse(["d", "demo"]).expect("d"),
                    Tag::parse(["buzz-channel", &channel]).expect("channel"),
                    Tag::parse(["buzz-protect", "refs/heads/main", "require-verdict"])
                        .expect("protect"),
                ])
                .sign_with_keys(&owner)
                .expect("sign existing");

        let updated = build_updated_repo_announcement(
            &existing,
            RepoChange::SetMaintainers(vec!["cc".repeat(32)]),
        )
        .expect("build update")
        .sign_with_keys(&owner)
        .expect("sign update");
        let names: Vec<String> = updated
            .tags
            .iter()
            .filter_map(|tag| tag.as_slice().first().cloned())
            .collect();
        for expected in ["d", "buzz-channel", "buzz-protect", "maintainers"] {
            assert!(
                names.iter().any(|name| name == expected),
                "{expected} survives: {names:?}"
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
