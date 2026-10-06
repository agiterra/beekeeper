//! `bee projects` commands — NIP-MP kind:30621 write path.
//!
//! All mutations follow a read-modify-write pattern:
//!   1. Fetch the caller's own live head via `kinds:[30621] + authors:[self] + #d:[slug]`.
//!   2. Mutate the tag set (strip `auth`, apply change).
//!   3. Re-validate the full envelope through Layer A before submitting.
//!   4. Set `created_at = max(client_now, head.created_at + 1)` so the
//!      replacement dominates the observed head and uses wall clock for
//!      ordinary stale heads. Unusually future heads may still hit the relay's
//!      timestamp-drift guard until time advances.
//!
//! Limitations recorded in this phase:
//!   - Relay hints are read-preserved but not authored (`--repo` carries
//!     a coordinate only; existing hinted tags survive RMW unchanged).
//!   - `delete` targets signer-self only (NIP-OA owner-delete path deferred).
//!   - Deletion durability against later arrival (watermark follow-up) is
//!     not in scope.

use beekeeper_core::kind::{
    KIND_GIT_REPO_ANNOUNCEMENT, KIND_PROJECT, KIND_PROJECT_MEMBERS, KIND_PROJECT_PUT_MEMBER,
    KIND_PROJECT_REMOVE_MEMBER, PROJECT_ROLE_COLLABORATOR, PROJECT_ROLE_OWNER,
};
use beekeeper_sdk::{
    build_delete_addressable, build_project_with_tags, ProjectMemberCoord, PROJECT_D_MAX_LEN,
};
use nostr::{Event, EventBuilder, Kind, Tag, Timestamp};

use crate::client::BuzzClient;
use crate::commands::parse_write_response;
use crate::commands::projects_cascade;
use crate::error::CliError;

// ── Buzz repo-ID grammar (bare --repo shorthand) ─────────────────────────────

/// Pattern for a Buzz-hosted repo identifier (bare `--repo` shorthand).
/// `[a-zA-Z0-9._-]{1,64}` — no colons, so guaranteed collision-free with
/// `30617:<owner>:<d>` full coordinates.
fn is_bare_repo_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

/// Expand a CLI `--repo` argument into a full `30617:<owner>:<d>` coordinate.
///
/// Bare form (`[a-zA-Z0-9._-]{1,64}`): owner defaults to the caller's pubkey.
/// Full form (`30617:<owner-hex>:<d>`): used verbatim.
fn expand_repo_coord(s: &str, caller_pubkey: &str) -> Result<ProjectMemberCoord, CliError> {
    if is_bare_repo_id(s) {
        // Bare form: expand to full coordinate with caller as owner.
        let full = format!("30617:{caller_pubkey}:{s}");
        ProjectMemberCoord::parse_full(&full)
            .map_err(|e| CliError::Usage(format!("invalid repo coordinate: {e}")))
    } else {
        // Full form: must be parseable as a complete coordinate.
        ProjectMemberCoord::parse_full(s)
            .map_err(|e| CliError::Usage(format!("invalid repo coordinate: {e}")))
    }
}

// ── Head-fetch helper ─────────────────────────────────────────────────────────

fn parse_events(json: &str) -> Result<Vec<Event>, CliError> {
    serde_json::from_str(json)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))
}

/// Fetch the caller's own live kind:30621 head for `slug`.
async fn fetch_own_project(client: &BuzzClient, slug: &str) -> Result<Option<Event>, CliError> {
    fetch_project(client, slug, None).await
}

/// Fetch a project head by slug and optional owner pubkey.
async fn fetch_project(
    client: &BuzzClient,
    slug: &str,
    owner: Option<&str>,
) -> Result<Option<Event>, CliError> {
    let pubkey = match owner {
        Some(pk) => {
            crate::validate::validate_hex64(pk)?;
            pk.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };
    let filter = serde_json::json!({
        "kinds": [KIND_PROJECT],
        "authors": [pubkey],
        "#d": [slug],
        "limit": 1,
    });
    let raw = client.query(&filter).await?;
    let mut events = parse_events(&raw)?;
    events.sort_by_key(|e| std::cmp::Reverse(e.created_at));
    Ok(events.into_iter().next())
}

// ── Tag helpers ───────────────────────────────────────────────────────────────

fn tag_name(tag: &Tag) -> Option<&str> {
    tag.as_slice().first().map(String::as_str)
}

fn tag_value(tag: &Tag) -> Option<&str> {
    tag.as_slice().get(1).map(String::as_str)
}

fn make_tag(parts: &[&str]) -> Result<Tag, CliError> {
    Tag::parse(parts.iter().copied())
        .map_err(|e| CliError::Other(format!("tag construction failed: {e}")))
}

// ── Submit helper ─────────────────────────────────────────────────────────────

async fn submit_project(client: &BuzzClient, builder: EventBuilder) -> Result<(), CliError> {
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "project changed concurrently; retry")?
    );
    Ok(())
}

// ── Build helpers ─────────────────────────────────────────────────────────────

/// Choose the later of client wall clock and the instant after the observed head.
///
/// The relay remains authoritative for timestamp drift: a sufficiently future
/// head can require a timestamp that the relay will temporarily reject.
fn next_timestamp(head: &Event, now: Timestamp) -> Result<Timestamp, CliError> {
    let after_head = head
        .created_at
        .as_secs()
        .checked_add(1)
        .ok_or_else(|| CliError::Other("project timestamp cannot be advanced".into()))?;
    Ok(Timestamp::from(after_head.max(now.as_secs())))
}

/// Strip `auth` from a tag list and pass the resulting envelope through
/// Layer A validation. Returns a validated `EventBuilder` at `next_ts`.
fn rebuild_project(
    content: &str,
    tags: Vec<Tag>,
    next_ts: Timestamp,
) -> Result<EventBuilder, CliError> {
    // Strip auth tags.
    let clean_tags: Vec<Tag> = tags
        .into_iter()
        .filter(|t| tag_name(t) != Some("auth"))
        .collect();

    build_project_with_tags(content, clean_tags)
        .map_err(|e| CliError::Other(format!("envelope validation failed: {e}")))
        .map(|b| b.custom_created_at(next_ts))
}

// ── Command implementations ───────────────────────────────────────────────────

/// `bee projects create`
#[allow(clippy::too_many_arguments)]
pub async fn cmd_create(
    client: &BuzzClient,
    slug: &str,
    repos: &[String],
    name: Option<&str>,
    description: Option<&str>,
    channel: Option<&str>,
    visibility: Option<&str>,
    access: &str,
    invited: &[String],
) -> Result<(), CliError> {
    // ── Local validation (all checks before any .await) ───────────────────
    validate_project_slug(slug)?;

    let caller_pubkey = client.keys().public_key().to_hex();

    // No repositories named: create the project's own two (spec § 4.11).
    if repos.is_empty() {
        return create_with_repositories(
            client,
            slug,
            name,
            description,
            channel,
            visibility,
            access,
            invited,
        )
        .await;
    }

    // Expand and validate repo coordinates.
    let members: Vec<ProjectMemberCoord> = repos
        .iter()
        .map(|r| expand_repo_coord(r, &caller_pubkey))
        .collect::<Result<Vec<_>, _>>()?;

    // Dedupe: preserve first occurrence, reject duplicates with Usage.
    let mut seen = std::collections::HashSet::new();
    for m in &members {
        if !seen.insert(m.coord.clone()) {
            return Err(CliError::Usage(format!(
                "duplicate --repo coordinate in this invocation: {:?}",
                m.coord
            )));
        }
    }

    // Validate optional metadata (early, before any network call).
    if let Some(ch) = channel {
        crate::validate::validate_uuid(ch)?;
    }
    if let Some(vis) = visibility {
        validate_visibility(vis)?;
    }
    if let Some(n) = name {
        if n.len() > 256 {
            return Err(CliError::Usage(format!(
                "project name must not exceed 256 bytes (got {})",
                n.len()
            )));
        }
    }

    // Parse invited members (`<pubkey>[:role]`); the creator is an implicit
    // owner and must not be invited.
    let invites = parse_member_args(invited, &caller_pubkey)?;

    // ── Network: collision preflight ──────────────────────────────────────
    if fetch_own_project(client, slug).await?.is_some() {
        return Err(CliError::Conflict(format!(
            "project {slug:?} already exists; use 'bee projects update' to modify it"
        )));
    }

    // ── Build the full tag set and validate through Layer A ──────────────
    let mut tags: Vec<Tag> = vec![make_tag(&["d", slug])?];
    if let Some(n) = name {
        tags.push(make_tag(&["name", n])?);
    }
    if let Some(d) = description {
        tags.push(make_tag(&["description", d])?);
    }
    for m in &members {
        let parts = m.to_tag_parts();
        let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
        tags.push(
            Tag::parse(parts_ref.iter().copied())
                .map_err(|e| CliError::Other(format!("member tag construction failed: {e}")))?,
        );
    }
    if let Some(ch) = channel {
        tags.push(make_tag(&["buzz-channel", ch])?);
    }
    if let Some(vis) = visibility {
        tags.push(make_tag(&["buzz-visibility", vis])?);
    }
    tags.push(make_tag(&["buzz-access", access])?);
    for (pubkey, role) in &invites {
        tags.push(make_tag(&["p", pubkey, "", role])?);
    }

    let builder = build_project_with_tags("", tags).map_err(crate::validate::sdk_err)?;
    submit_project(client, builder).await
}

/// The two repository ids a project creates with it (spec § 4.11).
pub(crate) fn project_repository_ids(slug: &str) -> Result<(String, String), CliError> {
    crate::validate::validate_repo_id(slug).map_err(|error| {
        CliError::Usage(format!(
            "without --repo the project slug must also be a repository id: {error}"
        ))
    })?;
    Ok((
        slug.to_string(),
        super::packs::default_repo_id(slug, super::packs::AGENTS_REPO_SUFFIX),
    ))
}

/// The refusal when one of `ids` is already announced by a key other than
/// `caller` — repository ids are one namespace per community — or `None`.
pub(crate) fn taken_repository_refusal(
    announcements: &[Event],
    caller: &str,
    ids: &[&str],
) -> Option<String> {
    announcements.iter().find_map(|event| {
        let author = event.pubkey.to_hex();
        if author == caller {
            return None;
        }
        let id = event
            .tags
            .iter()
            .find(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
            .and_then(|tag| tag.as_slice().get(1))?;
        ids.contains(&id.as_str()).then(|| {
            format!(
                "repository id {id:?} is already taken in this community by {}…; repository ids \
                 are one namespace per community, so choose another project slug — nothing was \
                 changed",
                &author[..8]
            )
        })
    })
}

/// `bee projects create <slug>` without `--repo`: the project and its two
/// repositories, in the order the desktop uses (spec § 4.11) — preflight both
/// ids community-wide; publish the 30621; announce `<slug>`, empty; announce
/// `<slug>-beekeeper-agents`; seed it from this build's shipped role
/// templates by reference and push `main`; publish the 30624 (`ref:
/// refs/heads/main`, `path: .`); republish the 30621 naming both. A seed or
/// push failure withdraws the agents announcement it followed; the project
/// and the code repository stand, and the printed JSON says what landed.
#[allow(clippy::too_many_arguments)]
async fn create_with_repositories(
    client: &BuzzClient,
    slug: &str,
    name: Option<&str>,
    description: Option<&str>,
    channel: Option<&str>,
    visibility: Option<&str>,
    access: &str,
    invited: &[String],
) -> Result<(), CliError> {
    use beekeeper_core::project_pack_source::{build_project_pack_source, PackPin, PACK_PATH_ROOT};
    use serde_json::json;

    let (code_id, agents_id) = project_repository_ids(slug)?;
    let caller = client.keys().public_key().to_hex();
    let coordinate = format!("30621:{caller}:{slug}");
    if let Some(ch) = channel {
        crate::validate::validate_uuid(ch)?;
    }
    if let Some(vis) = visibility {
        validate_visibility(vis)?;
    }
    let invites = parse_member_args(invited, &caller)?;
    // The seed's inputs are resolved before anything is published.
    let templates = crate::commands::pack::resolve_templates_dir(None).ok_or_else(|| {
        CliError::Usage(format!(
            "{}; nothing was created",
            crate::commands::pack::no_templates_message()
        ))
    })?;
    let catalog = beekeeper_persona::template::TemplateCatalog::load(&templates, "cli")
        .map_err(|e| CliError::Usage(format!("template catalog: {e}")))?;

    // ── Preflight ─────────────────────────────────────────────────────────
    if fetch_own_project(client, slug).await?.is_some() {
        return Err(CliError::Conflict(format!(
            "project {slug:?} already exists; use 'bee projects update' to modify it"
        )));
    }
    let announced = parse_events(
        &client
            .query(&json!({
                "kinds": [KIND_GIT_REPO_ANNOUNCEMENT],
                "#d": [code_id, agents_id],
                "limit": 16,
            }))
            .await?,
    )?;
    if let Some(refusal) = taken_repository_refusal(&announced, &caller, &[&code_id, &agents_id]) {
        return Err(CliError::Conflict(refusal));
    }
    let existing_ids: Vec<String> = announced
        .iter()
        .filter_map(|event| {
            event
                .tags
                .iter()
                .find(|tag| tag.as_slice().first().map(String::as_str) == Some("d"))
                .and_then(|tag| tag.as_slice().get(1).cloned())
        })
        .collect();

    // ── The project head, without members yet ─────────────────────────────
    let mut tags: Vec<Tag> = vec![make_tag(&["d", slug])?];
    if let Some(n) = name {
        tags.push(make_tag(&["name", n])?);
    }
    if let Some(d) = description {
        tags.push(make_tag(&["description", d])?);
    }
    if let Some(ch) = channel {
        tags.push(make_tag(&["buzz-channel", ch])?);
    }
    if let Some(vis) = visibility {
        tags.push(make_tag(&["buzz-visibility", vis])?);
    }
    tags.push(make_tag(&["buzz-access", access])?);
    for (pubkey, role) in &invites {
        tags.push(make_tag(&["p", pubkey, "", role])?);
    }
    let head = client
        .sign_event(build_project_with_tags("", tags.clone()).map_err(crate::validate::sdk_err)?)?;
    client.submit_event(head.clone()).await?;

    // ── The two announcements ─────────────────────────────────────────────
    let relay = client.relay_url().trim_end_matches('/').to_string();
    let code_url = format!("{relay}/git/{caller}/{code_id}");
    let agents_url = format!("{relay}/git/{caller}/{agents_id}");
    let mut announced_now: Vec<Option<String>> = Vec::new();
    for (id, repo_name, desc, url) in [
        (
            &code_id,
            name.unwrap_or(slug).to_string(),
            "This project's code.",
            &code_url,
        ),
        (
            &agents_id,
            format!("{} agents", name.unwrap_or(slug)),
            "This project's agent team and plans: roles/ and plans/, each with an archive/ for \
             what is retired. Beekeeper's role source (kind:30624, path `.`).",
            &agents_url,
        ),
    ] {
        if existing_ids.iter().any(|existing| existing == id) {
            announced_now.push(None);
            continue;
        }
        let builder = crate::commands::repos::build_create_announcement(
            id,
            Some(&repo_name),
            Some(desc),
            &[url.to_string()],
            None,
            &[],
            None,
            Some(&coordinate),
            &[],
        )?;
        let event = client.sign_event(builder)?;
        client.submit_event(event.clone()).await?;
        announced_now.push(Some(event.id.to_hex()));
    }
    let code_announcement = announced_now[0].clone();
    let agents_announcement = announced_now[1].clone();

    // ── Seed and push the agents repository ───────────────────────────────
    let seed =
        std::env::temp_dir().join(format!("bee-agents-seed-{}", uuid::Uuid::new_v4().simple()));
    beekeeper_persona::seed::write_agents_repo_seed(&seed, &catalog, slug)
        .map_err(|e| CliError::Other(format!("seed: {e}")))?;
    let seeded = super::packs::seed_packs_repository(&seed, PACK_PATH_ROOT, &agents_url);
    std::fs::remove_dir_all(&seed).ok();
    let seeded = match seeded {
        Ok(seeded) => seeded,
        Err(error) => {
            let (withdrawn, withdrawal_error) = if agents_announcement.is_some() {
                super::packs::withdraw_repo_announcement(client, &caller, &agents_id).await
            } else {
                (None, None)
            };
            eprintln!(
                "{}",
                json!({
                    "step": "seed",
                    "project": coordinate,
                    "code_repo": format!("30617:{caller}:{code_id}"),
                    "agents_repo": format!("30617:{caller}:{agents_id}"),
                    "seed_error": error.to_string(),
                    "agents_announcement_withdrawn_event_id": withdrawn,
                    "agents_announcement_withdrawal_error": withdrawal_error,
                    "note": "the project and its code repository exist; the agents repository was not seeded. Fix the push and finish with `bee packs init --project <coordinate>`.",
                })
            );
            return Err(error);
        }
    };

    // ── The source, then the head naming both repositories ────────────────
    let draft = build_project_pack_source(
        &coordinate,
        &format!("30617:{caller}:{agents_id}"),
        &PackPin::Ref(seeded.pushed_ref.clone()),
        Some(PACK_PATH_ROOT),
        Some("seeded by bee projects create from this build's shipped role templates"),
    )
    .map_err(CliError::Usage)?;
    let source_tags: Vec<Tag> = draft
        .tags
        .iter()
        .map(|tag| {
            Tag::parse(tag.clone())
                .map_err(|error| CliError::Other(format!("invalid tag: {error}")))
        })
        .collect::<Result<_, _>>()?;
    let source = client.sign_event(
        EventBuilder::new(
            Kind::Custom(beekeeper_core::kind::KIND_PROJECT_PACK_SOURCE as u16),
            draft.content,
        )
        .tags(source_tags),
    )?;
    client.submit_event(source.clone()).await?;

    for id in [&code_id, &agents_id] {
        let member = ProjectMemberCoord::parse_full(&format!("30617:{caller}:{id}"))
            .map_err(|e| CliError::Other(format!("member coordinate: {e}")))?;
        let parts = member.to_tag_parts();
        let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
        tags.push(
            Tag::parse(parts_ref.iter().copied())
                .map_err(|e| CliError::Other(format!("member tag construction failed: {e}")))?,
        );
    }
    let republished = client.sign_event(rebuild_project(
        "",
        tags,
        next_timestamp(&head, Timestamp::now())?,
    )?)?;
    client.submit_event(republished.clone()).await?;

    println!(
        "{}",
        json!({
            "project": coordinate,
            "event_id": republished.id.to_hex(),
            "code_repo": format!("30617:{caller}:{code_id}"),
            "code_repo_existed": code_announcement.is_none(),
            "agents_repo": format!("30617:{caller}:{agents_id}"),
            "agents_repo_existed": agents_announcement.is_none(),
            "commit": seeded.commit,
            "pushed_ref": seeded.pushed_ref,
            "pack_source_event_id": source.id.to_hex(),
            "path": PACK_PATH_ROOT,
        })
    );
    Ok(())
}

/// `bee projects get`
pub async fn cmd_get(client: &BuzzClient, slug: &str, owner: Option<&str>) -> Result<(), CliError> {
    validate_project_slug(slug)?;
    let resp = match fetch_project(client, slug, owner).await? {
        Some(event) => serde_json::json!({
            "event_id": event.id.to_hex(),
            "pubkey": event.pubkey.to_hex(),
            "created_at": event.created_at.as_secs(),
            "kind": event.kind.as_u16(),
            "tags": event.tags.iter().map(|t| t.as_slice().to_vec()).collect::<Vec<_>>(),
            "content": event.content,
        }),
        None => {
            let owner_desc = owner.unwrap_or("current identity");
            return Err(CliError::NotFound(format!(
                "project {slug:?} not found for {owner_desc}"
            )));
        }
    };
    println!("{resp}");
    Ok(())
}

/// `bee projects list`
pub async fn cmd_list(
    client: &BuzzClient,
    owner: Option<&str>,
    limit: Option<u32>,
) -> Result<(), CliError> {
    let pubkey = match owner {
        Some(pk) => {
            crate::validate::validate_hex64(pk)?;
            pk.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };
    let mut filter = serde_json::json!({
        "kinds": [KIND_PROJECT],
        "authors": [pubkey],
    });
    if let Some(n) = limit {
        filter["limit"] = serde_json::json!(n);
    }
    let resp = client.query(&filter).await?;
    println!("{resp}");
    Ok(())
}

/// `bee projects add-repo`
pub async fn cmd_add_repo(
    client: &BuzzClient,
    slug: &str,
    repos: &[String],
) -> Result<(), CliError> {
    validate_project_slug(slug)?;
    let caller_pubkey = client.keys().public_key().to_hex();

    // ── Local validation before any .await ────────────────────────────────
    let new_members: Vec<ProjectMemberCoord> = repos
        .iter()
        .map(|r| expand_repo_coord(r, &caller_pubkey))
        .collect::<Result<Vec<_>, _>>()?;

    // Dedupe within this invocation: first occurrence wins, duplicate → Usage.
    let mut seen = std::collections::HashSet::new();
    for m in &new_members {
        if !seen.insert(m.coord.clone()) {
            return Err(CliError::Usage(format!(
                "duplicate --repo coordinate in this invocation: {:?}",
                m.coord
            )));
        }
    }

    // ── Network: fetch head ───────────────────────────────────────────────
    let head = fetch_own_project(client, slug)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("project {slug:?} not found")))?;
    let next_ts = next_timestamp(&head, Timestamp::now())?;

    // Build the new tag set: keep existing tags (including hinted members),
    // append new members only if not already present (by coordinate).
    let mut tags: Vec<Tag> = head.tags.iter().cloned().collect();
    let existing_coords: std::collections::HashSet<String> = head
        .tags
        .iter()
        .filter(|t| tag_name(t) == Some("a"))
        .filter_map(|t| tag_value(t).map(String::from))
        .collect();
    let mut added = 0usize;
    for m in &new_members {
        if !existing_coords.contains(m.coord.as_str()) {
            let parts = m.to_tag_parts();
            let parts_ref: Vec<&str> = parts.iter().map(String::as_str).collect();
            tags.push(
                Tag::parse(parts_ref.iter().copied())
                    .map_err(|e| CliError::Other(format!("member tag construction failed: {e}")))?,
            );
            added += 1;
        }
    }

    // All requested coordinates were already present — no change to publish.
    if added == 0 {
        return Err(CliError::Conflict(format!(
            "all requested repositories are already members of project {slug:?}"
        )));
    }

    let builder = rebuild_project(&head.content, tags, next_ts)?;
    submit_project(client, builder).await
}

/// `bee projects remove-repo`
pub async fn cmd_remove_repo(
    client: &BuzzClient,
    slug: &str,
    repos: &[String],
) -> Result<(), CliError> {
    validate_project_slug(slug)?;
    let caller_pubkey = client.keys().public_key().to_hex();

    // ── Local validation before any .await ────────────────────────────────
    let to_remove: Vec<ProjectMemberCoord> = repos
        .iter()
        .map(|r| expand_repo_coord(r, &caller_pubkey))
        .collect::<Result<Vec<_>, _>>()?;

    // ── Network: fetch head ───────────────────────────────────────────────
    let head = fetch_own_project(client, slug)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("project {slug:?} not found")))?;
    let next_ts = next_timestamp(&head, Timestamp::now())?;

    // Verify all requested repos exist in the project.
    let existing_coords: std::collections::HashSet<String> = head
        .tags
        .iter()
        .filter(|t| tag_name(t) == Some("a"))
        .filter_map(|t| tag_value(t).map(String::from))
        .collect();
    for m in &to_remove {
        if !existing_coords.contains(m.coord.as_str()) {
            return Err(CliError::NotFound(format!(
                "project {slug:?} does not contain member {:?}",
                m.coord
            )));
        }
    }

    let remove_coords: std::collections::HashSet<&str> =
        to_remove.iter().map(|m| m.coord.as_str()).collect();

    // Keep all tags except auth and the removed members.
    let tags: Vec<Tag> = head
        .tags
        .iter()
        .filter(|t| {
            if tag_name(t) == Some("auth") {
                return false;
            }
            if tag_name(t) == Some("a") {
                if let Some(coord) = tag_value(t) {
                    return !remove_coords.contains(coord);
                }
            }
            true
        })
        .cloned()
        .collect();

    // Single rebuild validates the full envelope and strips any remaining auth.
    let builder = rebuild_project(&head.content, tags, next_ts)?;
    submit_project(client, builder).await
}

/// `bee projects update`
///
/// Requires at least one setter or clearer; a no-op call is a usage error.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_update(
    client: &BuzzClient,
    slug: &str,
    name: Option<&str>,
    clear_name: bool,
    description: Option<&str>,
    clear_description: bool,
    channel: Option<&str>,
    clear_channel: bool,
    visibility: Option<&str>,
    clear_visibility: bool,
    access: Option<&str>,
) -> Result<(), CliError> {
    // Guard: at least one mutation required. The clap `ArgGroup` with
    // `required(true).multiple(true)` enforces this at parse time; this
    // runtime check is a defense-in-depth safety net for callers that invoke
    // `cmd_update` directly (e.g. tests and future programmatic callers).
    let has_mutation = name.is_some()
        || clear_name
        || description.is_some()
        || clear_description
        || channel.is_some()
        || clear_channel
        || visibility.is_some()
        || clear_visibility
        || access.is_some();
    if !has_mutation {
        return Err(CliError::Usage(
            "bee projects update requires at least one of: \
             --name, --clear-name, --description, --clear-description, \
             --channel, --clear-channel, --visibility, --clear-visibility, \
             --access"
                .into(),
        ));
    }

    validate_project_slug(slug)?;
    if let Some(ch) = channel {
        crate::validate::validate_uuid(ch)?;
    }
    if let Some(vis) = visibility {
        validate_visibility(vis)?;
    }

    let head = fetch_own_project(client, slug)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("project {slug:?} not found")))?;
    let next_ts = next_timestamp(&head, Timestamp::now())?;

    // Build the new tag set. For each singleton metadata field:
    //   - setter present: replace value (strip old, append new)
    //   - clear flag set: drop the tag
    //   - neither: keep existing
    // Non-singleton / non-metadata tags (d, a, p, unknown) are preserved
    // as-is. `buzz-access` has no clear variant: an absent tag means public,
    // so flipping must always be an explicit setter.
    let singleton_fields = [
        "name",
        "description",
        "buzz-channel",
        "buzz-visibility",
        "buzz-access",
    ];
    let mut tags: Vec<Tag> = head
        .tags
        .iter()
        .filter(|t| {
            if tag_name(t) == Some("auth") {
                return false;
            }
            // Drop singletons we're replacing or clearing.
            if let Some(field) = tag_name(t) {
                if singleton_fields.contains(&field) {
                    let clear = match field {
                        "name" => clear_name || name.is_some(),
                        "description" => clear_description || description.is_some(),
                        "buzz-channel" => clear_channel || channel.is_some(),
                        "buzz-visibility" => clear_visibility || visibility.is_some(),
                        "buzz-access" => access.is_some(),
                        _ => false,
                    };
                    return !clear;
                }
            }
            true
        })
        .cloned()
        .collect();

    // Append new singleton values.
    if let Some(n) = name {
        tags.push(make_tag(&["name", n])?);
    }
    if let Some(d) = description {
        tags.push(make_tag(&["description", d])?);
    }
    if let Some(ch) = channel {
        tags.push(make_tag(&["buzz-channel", ch])?);
    }
    if let Some(vis) = visibility {
        tags.push(make_tag(&["buzz-visibility", vis])?);
    }
    if let Some(acc) = access {
        tags.push(make_tag(&["buzz-access", acc])?);
    }

    let builder = build_project_with_tags(&head.content, tags)
        .map_err(|e| CliError::Other(format!("envelope validation failed: {e}")))?
        .custom_created_at(next_ts);
    submit_project(client, builder).await
}

/// Publish the kind:30621 tombstone for `slug` against an observed `head`.
///
/// Head-based and verified:
///   1. Build tombstone at `max(client_now, head.created_at + 1)`.
///   2. Submit.
///   3. Re-query the coordinate; if a newer head survived → `Conflict`.
///
/// Shared by the default delete and the `--cascade` path so the two can never
/// drift; the cascade runs this **last**.
async fn publish_project_tombstone(
    client: &BuzzClient,
    slug: &str,
    head: &Event,
) -> Result<(), CliError> {
    let next_ts = next_timestamp(head, Timestamp::now())?;

    let pubkey_hex = client.keys().public_key().to_hex();
    let tombstone = build_delete_addressable(KIND_PROJECT, &pubkey_hex, slug)
        .map_err(|e| CliError::Other(format!("failed to build delete event: {e}")))?
        .custom_created_at(next_ts);

    let event = client.sign_event(tombstone)?;
    let raw = client.submit_event(event).await?;
    parse_write_response(&raw, "delete event was dominated; a newer head exists")?;

    // Post-submit verification: re-query to confirm the head is gone.
    if let Some(survivor) = fetch_own_project(client, slug).await? {
        // A newer head survived the tombstone.
        return Err(CliError::Conflict(format!(
            "project {slug:?} still exists (head at {}); a concurrent write raced the delete",
            survivor.created_at.as_secs()
        )));
    }

    Ok(())
}

/// `bee projects delete`
///
/// **Default (no `--cascade`): unchanged.** Per `docs/nips/NIP-MP.md`, deleting
/// a project deletes the kind:30621 event only — member repositories, channels,
/// workflows, and messages are untouched. There is no cascade in either
/// direction unless the caller explicitly asks for one.
///
/// With `--cascade` the CLI additionally orchestrates, client-side and in this
/// order, the deletion of the project's channels (kind:9008) and the caller's
/// own workflow definitions in them (kind:5 `a`-tag), and only then publishes
/// the project tombstone — see [`crate::commands::projects_cascade`].
///
/// `--dry-run` prints the enumerated plan and publishes nothing. Without
/// `--dry-run`, a cascade **always** requires explicit `--yes` confirmation:
/// the plan is printed and the command exits with a usage error until the
/// caller re-runs with `--yes`. That holds even when the plan enumerated no
/// children — an empty plan is indistinguishable from a failed enumeration,
/// and a destructive command must not skip its own gate on the strength of a
/// guess.
pub async fn cmd_delete(
    client: &BuzzClient,
    slug: &str,
    cascade: bool,
    dry_run: bool,
    yes: bool,
) -> Result<(), CliError> {
    validate_project_slug(slug)?;

    let head = fetch_own_project(client, slug)
        .await?
        .ok_or_else(|| CliError::NotFound(format!("project {slug:?} not found")))?;

    if !cascade {
        // Default path — byte-identical to the pre-cascade behavior.
        publish_project_tombstone(client, slug, &head).await?;
        println!("{}", serde_json::json!({ "deleted": slug, "status": "ok" }));
        return Ok(());
    }

    let owner_hex = client.keys().public_key().to_hex();
    let coordinate = projects_cascade::project_coordinate(&owner_hex, slug);
    // The head's `a` tags mix repository and agent coordinates; the cascade
    // filters to kind:30617 itself. Its `["channel", <uuid>]` tags are the
    // project's forward refs to member channels — one of the two independent
    // bindings a channel can have, so the cascade needs both.
    let member_coords: Vec<String> = head
        .tags
        .iter()
        .filter(|t| tag_name(t) == Some("a"))
        .filter_map(|t| tag_value(t).map(String::from))
        .collect();
    let head_channel_ids: Vec<String> = head
        .tags
        .iter()
        .filter(|t| tag_name(t) == Some("channel"))
        .filter_map(|t| tag_value(t).map(String::from))
        .filter(|id| !id.is_empty())
        .collect();

    let plan = projects_cascade::enumerate_cascade(
        client,
        slug,
        &coordinate,
        &member_coords,
        &head_channel_ids,
    )
    .await?;

    if dry_run {
        println!("{}", projects_cascade::plan_json(&plan, true));
        return Ok(());
    }

    // The confirmation gate is unconditional for `--cascade`. An empty plan is
    // NOT self-evidently "nothing to do" — it is equally the signature of an
    // enumeration that came back short (a relay page missed, a binding this
    // client does not know about), and letting that publish a tombstone
    // silently is exactly the failure mode this gate exists to stop. So print
    // the plan and refuse until `--yes`, saying plainly which case it is.
    if let Some(message) = projects_cascade::cascade_confirmation_required(&plan, yes) {
        // Print the same plan, then refuse. Nothing has been published.
        println!("{}", projects_cascade::plan_json(&plan, false));
        return Err(CliError::Usage(message));
    }

    projects_cascade::execute_cascade(client, &plan, || {
        publish_project_tombstone(client, slug, &head)
    })
    .await
}

// ── Membership ops (kinds 9010/9011 + kind 39010 roster reads) ────────────────

/// Parse repeated `--member <pubkey>[:role]` create arguments.
///
/// Role defaults to `collaborator`; the pubkey must be 64 lowercase hex and
/// must not be the caller (the creator is the project's implicit owner and
/// never appears in `p` tags).
fn parse_member_args(
    members: &[String],
    caller_pubkey: &str,
) -> Result<Vec<(String, &'static str)>, CliError> {
    let mut parsed = Vec::with_capacity(members.len());
    let mut seen = std::collections::HashSet::new();
    for member in members {
        let (pubkey, role) = match member.split_once(':') {
            Some((pubkey, role)) => (pubkey, parse_project_role(role)?),
            None => (member.as_str(), PROJECT_ROLE_COLLABORATOR),
        };
        validate_member_pubkey(pubkey)?;
        if pubkey == caller_pubkey {
            return Err(CliError::Usage(
                "the project creator is an implicit owner and must not be listed in --member"
                    .into(),
            ));
        }
        if !seen.insert(pubkey.to_string()) {
            return Err(CliError::Usage(format!(
                "duplicate --member pubkey in this invocation: {pubkey:?}"
            )));
        }
        parsed.push((pubkey.to_string(), role));
    }
    Ok(parsed)
}

/// Validate a project role token against the pinned vocabulary.
fn parse_project_role(role: &str) -> Result<&'static str, CliError> {
    beekeeper_core::kind::PROJECT_ROLES
        .iter()
        .find(|candidate| **candidate == role)
        .copied()
        .ok_or_else(|| {
            CliError::Usage(format!(
                "member role must be one of {:?} (got {role:?})",
                beekeeper_core::kind::PROJECT_ROLES
            ))
        })
}

/// Validate a member pubkey: exactly 64 lowercase hex characters (the relay
/// gate compares byte-exact, so uppercase would silently never match).
pub(crate) fn validate_member_pubkey(pubkey: &str) -> Result<(), CliError> {
    if pubkey.len() != 64
        || !pubkey
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(CliError::Usage(format!(
            "member pubkey must be a 64-character lowercase hex string: {pubkey}"
        )));
    }
    Ok(())
}

/// Build the canonical project coordinate `30621:<owner>:<slug>` a membership
/// op or roster read is scoped to. Owner defaults to the caller.
fn membership_coordinate(
    client: &BuzzClient,
    slug: &str,
    owner: Option<&str>,
) -> Result<String, CliError> {
    validate_project_slug(slug)?;
    let owner_hex = match owner {
        Some(owner) => {
            validate_member_pubkey(owner)?;
            owner.to_string()
        }
        None => client.keys().public_key().to_hex(),
    };
    Ok(format!("{KIND_PROJECT}:{owner_hex}:{slug}"))
}

/// Publish a kind 9010 put-member op: add a member or change their role.
///
/// Shared by `add-member` and `set-role` — the relay treats a re-put of an
/// existing member as a role change.
pub async fn cmd_put_member(
    client: &BuzzClient,
    slug: &str,
    pubkey: &str,
    role: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_member_pubkey(pubkey)?;
    let role = parse_project_role(role)?;
    let coordinate = membership_coordinate(client, slug, owner)?;

    let tags = vec![
        make_tag(&["a", &coordinate])?,
        make_tag(&["p", pubkey, "", role])?,
    ];
    let builder = EventBuilder::new(Kind::Custom(KIND_PROJECT_PUT_MEMBER as u16), "").tags(tags);
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "membership op was already applied")?
    );
    Ok(())
}

/// Publish a kind 9011 remove-member op.
pub async fn cmd_remove_member(
    client: &BuzzClient,
    slug: &str,
    pubkey: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    validate_member_pubkey(pubkey)?;
    let coordinate = membership_coordinate(client, slug, owner)?;

    let tags = vec![make_tag(&["a", &coordinate])?, make_tag(&["p", pubkey])?];
    let builder = EventBuilder::new(Kind::Custom(KIND_PROJECT_REMOVE_MEMBER as u16), "").tags(tags);
    let event = client.sign_event(builder)?;
    let raw = client.submit_event(event).await?;
    println!(
        "{}",
        parse_write_response(&raw, "membership op was already applied")?
    );
    Ok(())
}

/// Extract `(pubkey, role)` pairs from `["p", <hex>, <hint>, <role>]` tags in
/// a raw event JSON value. A missing or unknown role falls back to
/// `collaborator` — the legacy meaning of a role-less invite.
fn roster_from_event_json(event: &serde_json::Value) -> Vec<(String, String)> {
    let Some(tags) = event.get("tags").and_then(serde_json::Value::as_array) else {
        return Vec::new();
    };
    tags.iter()
        .filter_map(|tag| {
            let parts = tag.as_array()?;
            if parts.first()?.as_str()? != "p" {
                return None;
            }
            let pubkey = parts.get(1)?.as_str()?.to_string();
            let role = parts
                .get(3)
                .and_then(serde_json::Value::as_str)
                .filter(|role| beekeeper_core::kind::is_valid_project_role(role))
                .unwrap_or(PROJECT_ROLE_COLLABORATOR)
                .to_string();
            Some((pubkey, role))
        })
        .collect()
}

/// Prepend the project creator to a roster read off the wire.
///
/// The relay's kind:39010 projection emits one `p` tag per *invited* member
/// and none for the creator: a membership op targeting them is refused
/// outright (`ProjectMemberOpRefusal::TargetsCreator`), so they can never
/// hold a roster row. They are nonetheless an Owner — `ProjectGate::role_of`
/// answers `Owner` for them before it looks at the member list, and that is
/// the role the relay authorizes pushes, reads and deletes against.
///
/// Printing the raw `p` tags therefore omitted the one member who can do the
/// most, and `bee projects members` disagreed with both the desktop roster
/// (which calls `rosterWithOwner` for exactly this reason) and the relay.
///
/// The creator is pinned first, and any stray roster row bearing their
/// pubkey is dropped rather than printed twice — a projection cannot
/// legitimately contain one, and if a hand-rolled event does, the implicit
/// Owner is the truth.
pub(crate) fn roster_with_creator(
    creator_hex: &str,
    roster: Vec<(String, String)>,
) -> Vec<(String, String)> {
    let creator = creator_hex.to_ascii_lowercase();
    let mut out = vec![(creator.clone(), PROJECT_ROLE_OWNER.to_string())];
    out.extend(
        roster
            .into_iter()
            .filter(|(pubkey, _)| pubkey.to_ascii_lowercase() != creator),
    );
    out
}

/// The creator component of a `30621:<owner-hex>:<slug>` coordinate.
fn coordinate_creator(coordinate: &str) -> &str {
    coordinate.split(':').nth(1).unwrap_or("")
}

/// Every pubkey the project at `coordinate` grants **Owner** — the roster half
/// of a repository's founder set (finding 33).
///
/// The creator is an Owner implicitly (they hold no roster row: a membership
/// op naming them is refused outright), and is always first. Then the accepted
/// roster: the relay-signed kind:39010 projection when one exists, and
/// otherwise the project head's own `p` tags, which is what the relay's ACL
/// was built from before any membership op was accepted — the same fallback
/// [`cmd_members`] uses, so this cannot disagree with what a person is shown.
///
/// An `Err` here means the roster could not be **read**, which every caller
/// must disclose rather than treat as "there are no other owners".
pub(crate) async fn project_owner_pubkeys(
    client: &BuzzClient,
    coordinate: &str,
) -> Result<Vec<String>, CliError> {
    let creator = coordinate_creator(coordinate).to_ascii_lowercase();
    let slug = coordinate.splitn(3, ':').nth(2).unwrap_or_default();
    let raw = client
        .query(&serde_json::json!({
            "kinds": [KIND_PROJECT_MEMBERS],
            "#d": [coordinate],
            "limit": 1,
        }))
        .await?;
    let mut projections: Vec<serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
    projections.sort_by_key(|event| {
        std::cmp::Reverse(event.get("created_at").and_then(serde_json::Value::as_i64))
    });
    let roster = match projections.first() {
        Some(projection) => roster_from_event_json(projection),
        None => match fetch_project(client, slug, Some(&creator)).await? {
            Some(head) => roster_from_event_json(&serde_json::json!({
                "tags": head.tags.iter().map(|t| t.as_slice().to_vec()).collect::<Vec<_>>(),
            })),
            None => Vec::new(),
        },
    };
    Ok(roster_with_creator(&creator, roster)
        .into_iter()
        .filter(|(_, role)| role == PROJECT_ROLE_OWNER)
        .map(|(pubkey, _)| pubkey)
        .collect())
}

/// `bee projects members` — print the authoritative roster as
/// `[{pubkey, role}]`.
///
/// Reads the latest relay-signed kind:39010 projection for the coordinate;
/// when none exists (the roster is still head-sourced) falls back to the
/// head's own `p` tags, where a role-less invite is a legacy collaborator.
pub async fn cmd_members(
    client: &BuzzClient,
    slug: &str,
    owner: Option<&str>,
) -> Result<(), CliError> {
    let coordinate = membership_coordinate(client, slug, owner)?;
    let output: Vec<serde_json::Value> = project_roster(client, &coordinate)
        .await?
        .iter()
        .map(|(pubkey, role)| serde_json::json!({ "pubkey": pubkey, "role": role }))
        .collect();
    println!("{}", serde_json::Value::Array(output));
    Ok(())
}

/// The roster of the project at `coordinate` as `(pubkey, role)` pairs, the
/// creator first as its implicit Owner — exactly what `bee projects members`
/// prints.
///
/// Reads the latest relay-signed kind:39010 projection; when none exists (the
/// roster is still head-sourced) falls back to the head's own `p` tags, where
/// a role-less invite is a legacy collaborator. `NotFound` when neither a
/// projection nor a head exists: an unreadable project is never reported as
/// a roster of one.
pub(crate) async fn project_roster(
    client: &BuzzClient,
    coordinate: &str,
) -> Result<Vec<(String, String)>, CliError> {
    let creator = coordinate_creator(coordinate).to_ascii_lowercase();
    let slug = coordinate.splitn(3, ':').nth(2).unwrap_or_default();
    let filter = serde_json::json!({
        "kinds": [KIND_PROJECT_MEMBERS],
        "#d": [coordinate],
        "limit": 1,
    });
    let raw = client.query(&filter).await?;
    let mut projections: Vec<serde_json::Value> = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
    projections.sort_by_key(|event| {
        std::cmp::Reverse(event.get("created_at").and_then(serde_json::Value::as_i64))
    });

    let roster = match projections.first() {
        Some(projection) => roster_from_event_json(projection),
        None => {
            // Head-sourced roster: no membership op has been accepted yet.
            let head = fetch_project(client, slug, Some(&creator))
                .await?
                .ok_or_else(|| CliError::NotFound(format!("project {slug:?} not found")))?;
            let head_json = serde_json::json!({
                "tags": head.tags.iter().map(|t| t.as_slice().to_vec()).collect::<Vec<_>>(),
            });
            roster_from_event_json(&head_json)
        }
    };
    Ok(roster_with_creator(&creator, roster))
}

// ── Validation helpers ────────────────────────────────────────────────────────

/// Validate a project slug: non-empty, ≤1024 bytes, verbatim.
/// Does NOT impose the Buzz repo-ID grammar — project slugs are more permissive.
pub(crate) fn validate_project_slug(slug: &str) -> Result<(), CliError> {
    if slug.is_empty() {
        return Err(CliError::Usage("project slug must not be empty".into()));
    }
    if slug.len() > PROJECT_D_MAX_LEN {
        return Err(CliError::Usage(format!(
            "project slug must not exceed {PROJECT_D_MAX_LEN} bytes (got {})",
            slug.len()
        )));
    }
    Ok(())
}

/// Validate a `buzz-visibility` value at the writer level.
fn validate_visibility(vis: &str) -> Result<(), CliError> {
    if vis != "listed" && vis != "unlisted" {
        return Err(CliError::Usage(format!(
            "visibility must be 'listed' or 'unlisted' (got {vis:?})"
        )));
    }
    Ok(())
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn dispatch(
    cmd: crate::ProjectsCmd,
    client: &BuzzClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::ProjectsCmd;
    match cmd {
        ProjectsCmd::Create {
            slug,
            repo,
            name,
            description,
            channel,
            visibility,
            access,
            member,
        } => {
            cmd_create(
                client,
                &slug,
                &repo,
                name.as_deref(),
                description.as_deref(),
                channel.as_deref(),
                visibility.map(|v| v.as_str()),
                access.as_str(),
                &member,
            )
            .await
        }
        ProjectsCmd::Get { slug, owner } => cmd_get(client, &slug, owner.as_deref()).await,
        ProjectsCmd::List { owner, limit } => cmd_list(client, owner.as_deref(), limit).await,
        ProjectsCmd::AddRepo { slug, repo } => cmd_add_repo(client, &slug, &repo).await,
        ProjectsCmd::RemoveRepo { slug, repo } => cmd_remove_repo(client, &slug, &repo).await,
        ProjectsCmd::Update {
            slug,
            name,
            clear_name,
            description,
            clear_description,
            channel,
            clear_channel,
            visibility,
            clear_visibility,
            access,
        } => {
            cmd_update(
                client,
                &slug,
                name.as_deref(),
                clear_name,
                description.as_deref(),
                clear_description,
                channel.as_deref(),
                clear_channel,
                visibility.map(|v| v.as_str()),
                clear_visibility,
                access.map(|a| a.as_str()),
            )
            .await
        }
        ProjectsCmd::Delete {
            slug,
            cascade,
            dry_run,
            yes,
        } => cmd_delete(client, &slug, cascade, dry_run, yes).await,
        ProjectsCmd::AddMember {
            slug,
            pubkey,
            role,
            owner,
        } => cmd_put_member(client, &slug, &pubkey, role.as_str(), owner.as_deref()).await,
        ProjectsCmd::RemoveMember {
            slug,
            pubkey,
            owner,
        } => cmd_remove_member(client, &slug, &pubkey, owner.as_deref()).await,
        ProjectsCmd::SetRole {
            slug,
            pubkey,
            role,
            owner,
        } => cmd_put_member(client, &slug, &pubkey, role.as_str(), owner.as_deref()).await,
        ProjectsCmd::Members { slug, owner } => cmd_members(client, &slug, owner.as_deref()).await,
        ProjectsCmd::Agents {
            slug,
            owner,
            project,
        } => {
            crate::commands::project_agents::cmd_agents(
                client,
                slug.as_deref(),
                owner.as_deref(),
                project.as_deref(),
                format,
            )
            .await
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use beekeeper_sdk::{validate_project_envelope, PROJECT_MEMBER_CAP};
    use nostr::Tag;

    use super::*;

    // ── Coordinate expansion ──────────────────────────────────────────────────

    const OWNER_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OWNER_B_HEX: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn expand_repo_coord_bare_expands_with_caller_pubkey() {
        let coord = expand_repo_coord("my-repo", OWNER_HEX).unwrap();
        assert_eq!(coord.coord, format!("30617:{OWNER_HEX}:my-repo"));
    }

    #[test]
    fn expand_repo_coord_full_passes_through() {
        let full = format!("30617:{OWNER_HEX}:some-repo");
        let coord = expand_repo_coord(&full, OWNER_B_HEX).unwrap();
        // Owner from the full coord, not the caller.
        assert_eq!(coord.coord, full);
    }

    #[test]
    fn expand_repo_coord_full_cross_owner() {
        let full = format!("30617:{OWNER_B_HEX}:infra");
        let coord = expand_repo_coord(&full, OWNER_HEX).unwrap();
        assert_eq!(coord.coord, full);
    }

    #[test]
    fn expand_repo_coord_rejects_uppercase_owner() {
        let upper = "30617:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA:buzz";
        assert!(expand_repo_coord(upper, OWNER_HEX).is_err());
    }

    #[test]
    fn expand_repo_coord_rejects_coordinate_shaped_bare_value() {
        // A value with a colon is never a bare id.
        let not_bare = "30617:something";
        // parse_full will fail because it's not a valid full coordinate either.
        assert!(expand_repo_coord(not_bare, OWNER_HEX).is_err());
    }

    // ── validate_project_slug ─────────────────────────────────────────────────

    #[test]
    fn validate_project_slug_accepts_normal() {
        assert!(validate_project_slug("my-project").is_ok());
        assert!(validate_project_slug("platform:v2").is_ok()); // colons allowed — more permissive than repo-id
    }

    #[test]
    fn validate_project_slug_rejects_empty() {
        assert!(validate_project_slug("").is_err());
    }

    #[test]
    fn validate_project_slug_rejects_over_1024() {
        let long = "a".repeat(1025);
        assert!(validate_project_slug(&long).is_err());
    }

    /// Spec § 4.11: without `--repo` the slug names the code repository and
    /// `<slug>-beekeeper-agents` the agents one, so it must be a repository
    /// id; a colon-bearing project slug is a project, not a repository.
    #[test]
    fn a_project_without_repos_creates_two_ids_from_a_repository_shaped_slug() {
        assert_eq!(
            project_repository_ids("tank-loop").unwrap(),
            (
                "tank-loop".to_string(),
                "tank-loop-beekeeper-agents".to_string()
            )
        );
        assert!(project_repository_ids("a:b").is_err());
        assert!(project_repository_ids(&"x".repeat(65)).is_err());
    }

    /// An id another key already announced refuses the create by name; the
    /// caller's own announcement is reused, not a collision.
    #[test]
    fn a_repository_id_taken_by_another_key_is_refused_by_name() {
        use nostr::{EventBuilder, Keys, Kind, Tag};
        let caller = Keys::generate();
        let other = Keys::generate();
        let announce = |keys: &Keys, id: &str| {
            EventBuilder::new(Kind::Custom(30617), "")
                .tags(vec![
                    Tag::parse(vec!["d".to_string(), id.to_string()]).unwrap()
                ])
                .sign_with_keys(keys)
                .unwrap()
        };
        let caller_hex = caller.public_key().to_hex();
        let ids = ["demo", "demo-beekeeper-agents"];
        assert_eq!(
            taken_repository_refusal(&[announce(&caller, "demo")], &caller_hex, &ids),
            None
        );
        let refusal = taken_repository_refusal(
            &[announce(&other, "demo-beekeeper-agents")],
            &caller_hex,
            &ids,
        )
        .expect("refused");
        assert!(refusal.contains("demo-beekeeper-agents"), "{refusal}");
        assert!(
            refusal.contains(&other.public_key().to_hex()[..8]),
            "{refusal}"
        );
        assert_eq!(
            taken_repository_refusal(&[announce(&other, "unrelated")], &caller_hex, &ids),
            None
        );
    }

    #[test]
    fn validate_project_slug_accepts_1024() {
        let at_limit = "a".repeat(1024);
        assert!(validate_project_slug(&at_limit).is_ok());
    }

    // ── validate_visibility ───────────────────────────────────────────────────

    #[test]
    fn validate_visibility_accepts_listed_and_unlisted() {
        assert!(validate_visibility("listed").is_ok());
        assert!(validate_visibility("unlisted").is_ok());
    }

    #[test]
    fn validate_visibility_rejects_unknown_token() {
        assert!(validate_visibility("chartreuse").is_err());
        assert!(validate_visibility("").is_err());
    }

    // ── is_bare_repo_id ───────────────────────────────────────────────────────

    #[test]
    fn bare_repo_id_accepts_valid() {
        assert!(is_bare_repo_id("buzz"));
        assert!(is_bare_repo_id("my-repo_1.0"));
    }

    #[test]
    fn bare_repo_id_rejects_colon() {
        assert!(!is_bare_repo_id("30617:something"));
        assert!(!is_bare_repo_id("has:colon"));
    }

    #[test]
    fn bare_repo_id_rejects_empty() {
        assert!(!is_bare_repo_id(""));
    }

    #[test]
    fn bare_repo_id_rejects_over_64() {
        let long = "a".repeat(65);
        assert!(!is_bare_repo_id(&long));
    }

    // ── tag helpers ───────────────────────────────────────────────────────────

    fn make_test_tag(parts: &[&str]) -> Tag {
        Tag::parse(parts.iter().copied()).unwrap()
    }

    // ── rebuild_project: hinted / unknown tag preservation ───────────────────

    #[test]
    fn rebuild_project_preserves_hinted_member_tags() {
        // A member 'a' tag with a relay hint must survive RMW untouched.
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let hint = "wss://relay.example.com";
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            Tag::parse(["a", &coord, hint]).unwrap(),
        ];
        let ts = Timestamp::from(1_700_000_001u64);
        let b = rebuild_project("", tags, ts).unwrap();
        let ev = b.sign_with_keys(&nostr::Keys::generate()).expect("sign");
        let a_tag = ev
            .tags
            .iter()
            .find(|t| tag_name(t) == Some("a"))
            .expect("a tag present");
        assert_eq!(
            a_tag.as_slice(),
            &["a".to_string(), coord, hint.to_string()],
            "relay hint must survive rebuild"
        );
    }

    #[test]
    fn rebuild_project_preserves_unknown_tags() {
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            make_test_tag(&["future-metadata", "value"]),
        ];
        let ts = Timestamp::from(1_700_000_001u64);
        let b = rebuild_project("", tags, ts).unwrap();
        let ev = b.sign_with_keys(&nostr::Keys::generate()).expect("sign");
        assert!(ev
            .tags
            .iter()
            .any(|t| tag_name(t) == Some("future-metadata")));
    }

    #[test]
    fn rebuild_project_strips_auth_tag() {
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            make_test_tag(&["auth", &"a".repeat(64), "kind=30617", &"b".repeat(128)]),
        ];
        let ts = Timestamp::from(1_700_000_001u64);
        let b = rebuild_project("", tags, ts).unwrap();
        let ev = b.sign_with_keys(&nostr::Keys::generate()).expect("sign");
        assert!(
            !ev.tags.iter().any(|t| tag_name(t) == Some("auth")),
            "auth tag must be stripped"
        );
    }

    #[test]
    fn rebuild_project_rejects_over_cap_foreign_head() {
        // A foreign head with 65 members must fail Layer A on republish.
        let mut tags = vec![make_test_tag(&["d", "wide"])];
        for i in 0..=64u32 {
            let coord = format!("30617:{OWNER_HEX}:repo-{i:02}");
            tags.push(make_test_tag(&["a", &coord]));
        }
        assert_eq!(
            tags.iter().filter(|t| tag_name(t) == Some("a")).count(),
            65,
            "65 a-tags"
        );
        let ts = Timestamp::from(1_700_000_001u64);
        // rebuild_project strips auth, but 65 a-tags still exceeds cap.
        assert!(
            rebuild_project("", tags, ts).is_err(),
            "over-cap foreign head must fail rebuild"
        );
    }

    #[test]
    fn rebuild_project_at_exact_cap_succeeds() {
        let mut tags = vec![make_test_tag(&["d", "wide"])];
        for i in 0..PROJECT_MEMBER_CAP {
            let coord = format!("30617:{OWNER_HEX}:repo-{i:02}");
            tags.push(make_test_tag(&["a", &coord]));
        }
        let ts = Timestamp::from(1_700_000_001u64);
        assert!(rebuild_project("", tags, ts).is_ok());
    }

    // ── clear-flag semantics ──────────────────────────────────────────────────

    /// Build a minimal head Event for testing update semantics without the relay.
    fn make_head_tags(extra: &[Tag]) -> Vec<Tag> {
        let mut tags = vec![make_test_tag(&["d", "platform"])];
        tags.extend_from_slice(extra);
        tags
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_update_tags(
        head_tags: Vec<Tag>,
        name: Option<&str>,
        clear_name: bool,
        description: Option<&str>,
        clear_description: bool,
        channel: Option<&str>,
        clear_channel: bool,
        visibility: Option<&str>,
        clear_visibility: bool,
    ) -> Vec<Tag> {
        // Replicate the tag-mutation logic from cmd_update (sans relay I/O).
        let singleton_fields = ["name", "description", "buzz-channel", "buzz-visibility"];
        let mut tags: Vec<Tag> = head_tags
            .iter()
            .filter(|t| {
                if tag_name(t) == Some("auth") {
                    return false;
                }
                if let Some(field) = tag_name(t) {
                    if singleton_fields.contains(&field) {
                        let clear = match field {
                            "name" => clear_name || name.is_some(),
                            "description" => clear_description || description.is_some(),
                            "buzz-channel" => clear_channel || channel.is_some(),
                            "buzz-visibility" => clear_visibility || visibility.is_some(),
                            _ => false,
                        };
                        return !clear;
                    }
                }
                true
            })
            .cloned()
            .collect();
        if let Some(n) = name {
            tags.push(make_test_tag(&["name", n]));
        }
        if let Some(d) = description {
            tags.push(make_test_tag(&["description", d]));
        }
        if let Some(ch) = channel {
            tags.push(make_test_tag(&["buzz-channel", ch]));
        }
        if let Some(vis) = visibility {
            tags.push(make_test_tag(&["buzz-visibility", vis]));
        }
        tags
    }

    #[test]
    fn update_omission_preserves_existing_field() {
        let head = make_head_tags(&[make_test_tag(&["name", "Old Name"])]);
        let result = apply_update_tags(head, None, false, None, false, None, false, None, false);
        assert!(result.iter().any(|t| tag_value(t) == Some("Old Name")));
    }

    #[test]
    fn update_setter_replaces_existing_field() {
        let head = make_head_tags(&[make_test_tag(&["name", "Old Name"])]);
        let result = apply_update_tags(
            head,
            Some("New Name"),
            false,
            None,
            false,
            None,
            false,
            None,
            false,
        );
        assert!(result.iter().any(|t| tag_value(t) == Some("New Name")));
        assert!(!result.iter().any(|t| tag_value(t) == Some("Old Name")));
    }

    #[test]
    fn update_clear_drops_existing_field() {
        let head = make_head_tags(&[make_test_tag(&["name", "Old Name"])]);
        let result = apply_update_tags(head, None, true, None, false, None, false, None, false);
        assert!(!result.iter().any(|t| tag_name(t) == Some("name")));
    }

    #[test]
    fn update_clear_visibility_drops_tag() {
        let head = make_head_tags(&[make_test_tag(&["buzz-visibility", "unlisted"])]);
        let result = apply_update_tags(head, None, false, None, false, None, false, None, true);
        assert!(!result
            .iter()
            .any(|t| tag_name(t) == Some("buzz-visibility")));
    }

    #[test]
    fn update_exactly_one_singleton_after_replace() {
        // Start with a buzz-channel; replace with a new one; must have exactly one.
        let uuid1 = "3580ca9b-47b4-4af9-b22a-1068778f26c6";
        let uuid2 = "00000000-0000-0000-0000-000000000000";
        let head = make_head_tags(&[make_test_tag(&["buzz-channel", uuid1])]);
        let result = apply_update_tags(
            head,
            None,
            false,
            None,
            false,
            Some(uuid2),
            false,
            None,
            false,
        );
        let channels: Vec<_> = result
            .iter()
            .filter(|t| tag_name(t) == Some("buzz-channel"))
            .collect();
        assert_eq!(channels.len(), 1);
        assert_eq!(tag_value(channels[0]), Some(uuid2));
    }

    // ── duplicate-member rejection on republish ───────────────────────────────

    #[test]
    fn duplicate_member_in_foreign_head_fails_rebuild() {
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            make_test_tag(&["a", &coord]),
            make_test_tag(&["a", &coord]), // duplicate
        ];
        let ts = Timestamp::from(1_700_000_001u64);
        assert!(rebuild_project("", tags, ts).is_err());
    }

    // ── validate_project_envelope integration ────────────────────────────────

    #[test]
    fn validate_project_envelope_accepts_hinted_member() {
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            Tag::parse(["a", &coord, "wss://relay.example.com"]).unwrap(),
        ];
        assert!(validate_project_envelope(&tags, "").is_ok());
    }

    #[test]
    fn validate_project_envelope_rejects_four_element_member() {
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            Tag::parse(["a", &coord, "wss://relay.example.com", "extra"]).unwrap(),
        ];
        assert!(validate_project_envelope(&tags, "").is_err());
    }

    // ── next_timestamp ordering ───────────────────────────────────────────────

    fn project_head_at(created_at: u64) -> Event {
        let keys = nostr::Keys::generate();
        let tags = vec![
            make_test_tag(&["d", "platform"]),
            make_test_tag(&["a", &format!("30617:{OWNER_HEX}:buzz")]),
        ];
        rebuild_project("", tags, Timestamp::from(created_at))
            .expect("valid head envelope")
            .sign_with_keys(&keys)
            .expect("sign")
    }

    #[test]
    fn next_timestamp_uses_later_of_wall_clock_and_after_head() {
        let cases = [
            ("stale head", 100, 1_000, 1_000),
            ("head equal to now", 1_000, 1_000, 1_001),
            ("future head", 1_500, 1_000, 1_501),
            ("last timestamp inside future boundary", 1_899, 1_000, 1_900),
            (
                "future boundary cannot be dominated inside the window",
                1_900,
                1_000,
                1_901,
            ),
        ];

        for (name, head_ts, now, expected) in cases {
            let head = project_head_at(head_ts);
            let next = next_timestamp(&head, Timestamp::from(now)).expect("no overflow");

            assert_eq!(next.as_secs(), expected, "case: {name}");
        }
    }

    #[test]
    fn next_timestamp_rejects_overflowing_head() {
        let head = project_head_at(u64::MAX);

        let err = next_timestamp(&head, Timestamp::from(1_000u64))
            .expect_err("maximum timestamp cannot be advanced");

        assert!(
            matches!(err, CliError::Other(ref message) if message == "project timestamp cannot be advanced"),
            "unexpected error: {err}"
        );
    }

    // ── empty update guard ────────────────────────────────────────────────────

    /// `cmd_update` with no setters or clearers must return `CliError::Usage`
    /// before making any network call.  The guard is synchronous (before the
    /// first `.await`) so we can drive it with a dummy client whose address
    /// would reject any real connection attempt.
    #[tokio::test]
    async fn empty_update_returns_usage_error_before_any_network_call() {
        let keys = nostr::Keys::generate();
        // Port 9 is the discard protocol — any real connect will be refused
        // immediately, but the guard fires before the first await so this
        // never reaches the network.
        let client = crate::client::BuzzClient::new("http://127.0.0.1:9".into(), keys, None, None)
            .expect("client construction");

        let err = cmd_update(
            &client, "my-slug", None, false, // name / clear_name
            None, false, // description / clear_description
            None, false, // channel / clear_channel
            None, false, // visibility / clear_visibility
            None,  // access
        )
        .await
        .expect_err("empty update must fail");

        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage, got {err:?}"
        );
    }

    // ── no-network malformed-input tests ─────────────────────────────────────
    //
    // All three cases use port 9 (discard protocol): any real connection is
    // refused immediately, but local validation fires before the first .await
    // so the network is never touched.

    fn discard_client() -> crate::client::BuzzClient {
        let keys = nostr::Keys::generate();
        crate::client::BuzzClient::new("http://127.0.0.1:9".into(), keys, None, None)
            .expect("client construction")
    }

    /// Invalid visibility token must return Usage before touching the relay.
    #[tokio::test]
    async fn create_invalid_visibility_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_create(
            &client,
            "my-slug",
            &["buzz".to_string()],
            None,
            None,
            None,
            Some("chartreuse"),
            "private",
            &[],
        )
        .await
        .expect_err("invalid visibility must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for invalid visibility, got {err:?}"
        );
    }

    /// A name longer than 256 bytes must return Usage before touching the relay.
    #[tokio::test]
    async fn create_overlong_name_returns_usage_before_any_network_call() {
        let client = discard_client();
        let long_name = "a".repeat(257);
        let err = cmd_create(
            &client,
            "my-slug",
            &["buzz".to_string()],
            Some(&long_name),
            None,
            None,
            None,
            "private",
            &[],
        )
        .await
        .expect_err("overlong name must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for overlong name, got {err:?}"
        );
    }

    /// A malformed --repo coordinate must return Usage before touching the relay.
    #[tokio::test]
    async fn create_malformed_repo_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_create(
            &client,
            "my-slug",
            &["nope:bad".to_string()],
            None,
            None,
            None,
            None,
            "private",
            &[],
        )
        .await
        .expect_err("malformed repo must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for malformed repo, got {err:?}"
        );
    }

    /// A malformed --repo coordinate on add-repo must return Usage before touching the relay.
    #[tokio::test]
    async fn add_repo_malformed_coord_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_add_repo(&client, "my-slug", &["nope:bad".to_string()])
            .await
            .expect_err("malformed repo must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for malformed repo on add-repo, got {err:?}"
        );
    }

    /// A malformed --repo coordinate on remove-repo must return Usage before touching the relay.
    #[tokio::test]
    async fn remove_repo_malformed_coord_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_remove_repo(&client, "my-slug", &["nope:bad".to_string()])
            .await
            .expect_err("malformed repo must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for malformed repo on remove-repo, got {err:?}"
        );
    }

    // ── duplicate --repo within one invocation ────────────────────────────────

    /// Supplying the same coordinate twice in one create call must return Usage
    /// (names the duplicate) before any network call.
    #[tokio::test]
    async fn create_duplicate_repo_returns_usage_before_any_network_call() {
        let client = discard_client();
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let err = cmd_create(
            &client,
            "my-slug",
            &[coord.clone(), coord.clone()],
            None,
            None,
            None,
            None,
            "private",
            &[],
        )
        .await
        .expect_err("duplicate repo must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for duplicate repo, got {err:?}"
        );
        // Error message must name the duplicate coordinate.
        assert!(
            format!("{err}").contains("buzz"),
            "Usage message must name the duplicate coordinate, got {err:?}"
        );
    }

    /// Supplying the same coordinate twice in one add-repo call must return Usage
    /// (names the duplicate) before any network call.
    #[tokio::test]
    async fn add_repo_duplicate_coord_returns_usage_before_any_network_call() {
        let client = discard_client();
        let coord = format!("30617:{OWNER_HEX}:buzz");
        let err = cmd_add_repo(&client, "my-slug", &[coord.clone(), coord.clone()])
            .await
            .expect_err("duplicate repo must fail");
        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage for duplicate repo on add-repo, got {err:?}"
        );
    }

    // ── parse_member_args (`--member <pubkey>[:role]`) ────────────────────────

    #[test]
    fn parse_member_args_defaults_role_to_collaborator() {
        let member = "c".repeat(64);
        let parsed = parse_member_args(std::slice::from_ref(&member), OWNER_HEX).unwrap();
        assert_eq!(parsed, vec![(member, PROJECT_ROLE_COLLABORATOR)]);
    }

    #[test]
    fn parse_member_args_accepts_explicit_roles() {
        for role in ["owner", "collaborator", "viewer"] {
            let member = format!("{}:{role}", "c".repeat(64));
            let parsed = parse_member_args(&[member], OWNER_HEX).unwrap();
            assert_eq!(parsed[0].1, role);
        }
    }

    #[test]
    fn parse_member_args_rejects_unknown_role() {
        let member = format!("{}:admin", "c".repeat(64));
        let err = parse_member_args(&[member], OWNER_HEX).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn parse_member_args_rejects_uppercase_and_short_pubkeys() {
        for bad in ["C".repeat(64), "c".repeat(63), "not-hex".to_string()] {
            let err = parse_member_args(&[bad], OWNER_HEX).unwrap_err();
            assert!(matches!(err, CliError::Usage(_)));
        }
    }

    #[test]
    fn parse_member_args_rejects_the_caller() {
        let err = parse_member_args(&[OWNER_HEX.to_string()], OWNER_HEX).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[test]
    fn parse_member_args_rejects_duplicates_across_role_spellings() {
        let plain = "c".repeat(64);
        let roled = format!("{plain}:viewer");
        let err = parse_member_args(&[plain, roled], OWNER_HEX).unwrap_err();
        assert!(matches!(err, CliError::Usage(_)));
    }

    // ── roster_from_event_json ────────────────────────────────────────────────

    #[test]
    fn roster_from_event_json_reads_role_element_four() {
        let event = serde_json::json!({
            "tags": [
                ["d", "coord"],
                ["p", "a".repeat(64), "", "owner"],
                ["p", "b".repeat(64), "", "viewer"],
            ]
        });
        assert_eq!(
            roster_from_event_json(&event),
            vec![
                ("a".repeat(64), "owner".to_string()),
                ("b".repeat(64), "viewer".to_string()),
            ]
        );
    }

    #[test]
    fn roster_from_event_json_defaults_missing_or_unknown_role_to_collaborator() {
        let event = serde_json::json!({
            "tags": [
                ["p", "a".repeat(64)],
                ["p", "b".repeat(64), "wss://relay"],
                ["p", "c".repeat(64), "", "mystery-role"],
            ]
        });
        let roster = roster_from_event_json(&event);
        assert_eq!(roster.len(), 3);
        assert!(roster.iter().all(|(_, role)| role == "collaborator"));
    }

    // ── roster_with_creator ───────────────────────────────────────────────────

    /// The bug this fixes: the relay's 39010 projection never names the
    /// creator, so the raw `p` tags omit the project's most privileged
    /// member entirely.
    #[test]
    fn roster_lists_the_creator_as_owner_even_though_no_p_tag_names_them() {
        let alice = "b".repeat(64);
        let roster =
            roster_with_creator(OWNER_HEX, vec![(alice.clone(), "collaborator".to_string())]);
        assert_eq!(
            roster,
            vec![
                (OWNER_HEX.to_string(), "owner".to_string()),
                (alice, "collaborator".to_string()),
            ],
            "the creator must be pinned first as Owner"
        );
    }

    /// A project whose roster is empty still has an Owner.
    #[test]
    fn a_projectless_roster_is_still_the_creator() {
        assert_eq!(
            roster_with_creator(OWNER_HEX, Vec::new()),
            vec![(OWNER_HEX.to_string(), "owner".to_string())]
        );
    }

    /// A hand-rolled 39010 naming the creator must not print them twice, and
    /// must not be able to demote them: the implicit Owner wins.
    #[test]
    fn a_stray_creator_row_is_dropped_rather_than_honoured() {
        let roster = roster_with_creator(
            OWNER_HEX,
            vec![
                (OWNER_HEX.to_ascii_uppercase(), "viewer".to_string()),
                ("c".repeat(64), "owner".to_string()),
            ],
        );
        assert_eq!(
            roster,
            vec![
                (OWNER_HEX.to_string(), "owner".to_string()),
                ("c".repeat(64), "owner".to_string()),
            ]
        );
    }

    /// The coordinate is the only place the creator's key appears in this
    /// command, so parsing it is load-bearing.
    #[test]
    fn coordinate_creator_reads_the_middle_component() {
        assert_eq!(
            coordinate_creator(&format!("30621:{OWNER_HEX}:platform")),
            OWNER_HEX
        );
        // A slug may contain colons — `splitn(3, ':')` semantics elsewhere —
        // so only the second component is ever taken.
        assert_eq!(
            coordinate_creator(&format!("30621:{OWNER_HEX}:a:b")),
            OWNER_HEX
        );
        assert_eq!(coordinate_creator("malformed"), "");
    }

    // ── membership_coordinate ─────────────────────────────────────────────────

    #[tokio::test]
    async fn membership_coordinate_defaults_owner_to_caller() {
        let client = discard_client();
        let caller = client.keys().public_key().to_hex();
        let coord = membership_coordinate(&client, "platform", None).unwrap();
        assert_eq!(coord, format!("30621:{caller}:platform"));
    }

    #[tokio::test]
    async fn membership_coordinate_uses_explicit_owner_and_rejects_uppercase() {
        let client = discard_client();
        let coord = membership_coordinate(&client, "platform", Some(OWNER_HEX)).unwrap();
        assert_eq!(coord, format!("30621:{OWNER_HEX}:platform"));

        let upper = OWNER_HEX.to_uppercase();
        assert!(membership_coordinate(&client, "platform", Some(&upper)).is_err());
    }

    // ── membership op input validation (no network) ───────────────────────────

    #[tokio::test]
    async fn put_member_invalid_role_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_put_member(&client, "my-slug", &"c".repeat(64), "admin", None)
            .await
            .expect_err("unknown role must fail");
        assert!(matches!(err, CliError::Usage(_)));
    }

    #[tokio::test]
    async fn remove_member_invalid_pubkey_returns_usage_before_any_network_call() {
        let client = discard_client();
        let err = cmd_remove_member(&client, "my-slug", "not-a-pubkey", None)
            .await
            .expect_err("malformed pubkey must fail");
        assert!(matches!(err, CliError::Usage(_)));
    }

    // ── create collision guard ────────────────────────────────────────────────

    // The create-collision Conflict path is pinned by the live transcript
    // (step: duplicate create → Conflict, exit=5). No relay mock is available
    // for a unit test; the no-network tests above cover all pre-await paths.

    // ── add-repo no-op guard ──────────────────────────────────────────────────

    // The add-repo no-op Conflict path is pinned by the live transcript
    // (step 7: buzz already present → exit=5). No relay mock is available
    // for a unit test; the async no-network tests above cover all pre-await paths.
}

// ── `projects delete --cascade` against a mock relay ──────────────────────────
//
// A tiny axum relay (same shape as `client.rs`'s retry-policy harness) that
// answers `/query` from a canned per-kind fixture and records every `/events`
// submission. That is enough to pin the two properties that matter: `--dry-run`
// publishes nothing, and a confirmed cascade publishes the kind:30621 tombstone
// last.
#[cfg(test)]
mod cascade_relay_tests {
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::routing::post;
    use axum::{Json, Router};
    use nostr::Keys;
    use serde_json::{json, Value};
    use tokio::net::TcpListener;

    use super::*;

    const CHANNEL_STREAM: &str = "11111111-1111-4111-8111-111111111111";
    const CHANNEL_TRANSPORT: &str = "22222222-2222-4222-8222-222222222222";
    const WORKFLOW: &str = "33333333-3333-4333-8333-333333333333";
    /// A channel bound to the project **only** by the head's forward
    /// `["channel", …]` ref — the relay never wrote a `project` back-reference
    /// onto its kind:39000 metadata.
    const CHANNEL_FORWARD_ONLY: &str = "44444444-4444-4444-8444-444444444444";

    /// Every event the mock relay accepted, in submission order.
    type Submitted = Arc<Mutex<Vec<Value>>>;

    fn head_event(keys: &Keys, slug: &str, extra_tags: Vec<Tag>) -> Event {
        let mut tags = vec![
            Tag::parse(["d", slug]).expect("d tag"),
            Tag::parse(["a", &format!("30617:{}:buzz", keys.public_key().to_hex())])
                .expect("a tag"),
        ];
        tags.extend(extra_tags);
        build_project_with_tags("", tags)
            .expect("valid project envelope")
            .custom_created_at(Timestamp::from(1_700_000_000u64))
            .sign_with_keys(keys)
            .expect("sign head")
    }

    fn channel_metadata(channel_id: &str, name: &str, ty: &str, coordinate: Option<&str>) -> Value {
        let mut tags = vec![
            json!(["d", channel_id]),
            json!(["name", name]),
            json!(["t", ty]),
        ];
        if let Some(coordinate) = coordinate {
            tags.push(json!(["project", coordinate]));
        }
        json!({
            "id": "0".repeat(64),
            "pubkey": "0".repeat(64),
            "created_at": 1_700_000_000u64,
            "kind": 39000,
            "content": "",
            "tags": tags,
        })
    }

    /// kind:39001 group-admins projection granting `owner` on `channel_id`.
    fn channel_admins(channel_id: &str, owner_hex: &str) -> Value {
        json!({
            "id": "2".repeat(64),
            "pubkey": "0".repeat(64),
            "created_at": 1_700_000_000u64,
            "kind": 39001,
            "content": "",
            "tags": [["d", channel_id], ["p", owner_hex, "owner"]],
        })
    }

    fn workflow_def(workflow_id: &str, channel_id: &str, author: &str) -> Value {
        json!({
            "id": "1".repeat(64),
            "pubkey": author,
            "created_at": 1_700_000_000u64,
            "kind": 30620,
            "content": "name: demo",
            "tags": [["d", workflow_id], ["h", channel_id]],
        })
    }

    /// The relay-side world one test runs against.
    struct Fixture {
        /// Extra tags on the kind:30621 head (forward `channel` refs, agent
        /// `a` members, …).
        head_tags: Vec<Tag>,
        /// kind:39000 channel metadata the relay serves.
        channels: Vec<Value>,
        /// kind:30620 workflow definitions the relay serves.
        workflows: Vec<Value>,
    }

    /// The default world: two back-referenced channels (one transport) and one
    /// caller-authored workflow.
    fn default_fixture(keys: &Keys, coordinate: &str) -> Fixture {
        Fixture {
            head_tags: Vec::new(),
            channels: vec![
                channel_metadata(CHANNEL_STREAM, "general", "stream", Some(coordinate)),
                channel_metadata(
                    CHANNEL_TRANSPORT,
                    "sessions",
                    projects_cascade::TRANSPORT_CHANNEL_TYPE,
                    Some(coordinate),
                ),
            ],
            workflows: vec![workflow_def(
                WORKFLOW,
                CHANNEL_STREAM,
                &keys.public_key().to_hex(),
            )],
        }
    }

    /// Spawn a mock relay. `/query` dispatches on the first requested kind;
    /// `/events` records the submission. Once a kind:5 tombstone naming the
    /// project coordinate arrives, kind:30621 queries return empty so the
    /// real post-submit verification in `publish_project_tombstone` passes.
    async fn mock_relay(keys: &Keys, slug: &str, fixture: Fixture) -> (String, Submitted) {
        let head =
            serde_json::to_value(head_event(keys, slug, fixture.head_tags)).expect("head json");
        let coordinate = projects_cascade::project_coordinate(&keys.public_key().to_hex(), slug);
        let owner_hex = keys.public_key().to_hex();
        // Every served channel grants the caller `owner`, so the ownership
        // pre-flight stays quiet unless a test says otherwise.
        let admins: Vec<Value> = fixture
            .channels
            .iter()
            .filter_map(|c| {
                c.get("tags")
                    .and_then(Value::as_array)
                    .and_then(|tags| tags.first())
                    .and_then(|t| t.get(1))
                    .and_then(Value::as_str)
            })
            .map(|id| channel_admins(id, &owner_hex))
            .collect();
        let channels = fixture.channels;
        let workflows = fixture.workflows;
        let submitted: Submitted = Arc::new(Mutex::new(Vec::new()));

        #[derive(Clone)]
        struct S {
            head: Value,
            coordinate: String,
            channels: Vec<Value>,
            admins: Vec<Value>,
            workflows: Vec<Value>,
            submitted: Submitted,
        }

        let state = S {
            head,
            coordinate,
            channels,
            admins,
            workflows,
            submitted: submitted.clone(),
        };

        let app = Router::new()
            .route(
                "/query",
                post(
                    |State(s): State<S>, Json(filters): Json<Vec<Value>>| async move {
                        let kind = filters
                            .first()
                            .and_then(|f| f.get("kinds"))
                            .and_then(Value::as_array)
                            .and_then(|k| k.first())
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let tombstoned = s.submitted.lock().is_ok_and(|events| {
                            events.iter().any(|e| {
                                e.get("kind").and_then(Value::as_u64) == Some(5)
                                    && e.get("tags").and_then(Value::as_array).is_some_and(|tags| {
                                        tags.iter().any(|t| {
                                            t.get(0).and_then(Value::as_str) == Some("a")
                                                && t.get(1).and_then(Value::as_str)
                                                    == Some(s.coordinate.as_str())
                                        })
                                    })
                            })
                        });
                        let body = match kind {
                            30621 if tombstoned => vec![],
                            30621 => vec![s.head.clone()],
                            39000 => s.channels.clone(),
                            39001 => s.admins.clone(),
                            30620 => s.workflows.clone(),
                            _ => vec![],
                        };
                        Json(Value::Array(body))
                    },
                ),
            )
            .route(
                "/events",
                post(|State(s): State<S>, Json(event): Json<Value>| async move {
                    let id = event
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if let Ok(mut events) = s.submitted.lock() {
                        events.push(event);
                    }
                    Json(json!({ "event_id": id, "accepted": true, "message": "ok" }))
                }),
            )
            .with_state(state);

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr: SocketAddr = listener.local_addr().expect("addr");
        tokio::spawn(async move { axum::serve(listener, app).await.expect("serve") });
        (format!("http://{addr}"), submitted)
    }

    async fn client_against(keys: &Keys, slug: &str) -> (crate::client::BuzzClient, Submitted) {
        let coordinate = projects_cascade::project_coordinate(&keys.public_key().to_hex(), slug);
        client_against_fixture(keys, slug, default_fixture(keys, &coordinate)).await
    }

    async fn client_against_fixture(
        keys: &Keys,
        slug: &str,
        fixture: Fixture,
    ) -> (crate::client::BuzzClient, Submitted) {
        let (url, submitted) = mock_relay(keys, slug, fixture).await;
        let client = crate::client::BuzzClient::new(url, keys.clone(), None, None)
            .expect("client construction");
        (client, submitted)
    }

    fn submitted_kinds(submitted: &Submitted) -> Vec<u64> {
        submitted
            .lock()
            .map(|events| {
                events
                    .iter()
                    .filter_map(|e| e.get("kind").and_then(Value::as_u64))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `--dry-run` enumerates and prints, and publishes absolutely nothing.
    #[tokio::test]
    async fn cascade_dry_run_publishes_nothing() {
        let keys = Keys::generate();
        let (client, submitted) = client_against(&keys, "platform").await;

        cmd_delete(&client, "platform", true, true, false)
            .await
            .expect("dry run must succeed");

        assert!(
            submitted_kinds(&submitted).is_empty(),
            "--dry-run must publish nothing, got {:?}",
            submitted_kinds(&submitted)
        );
    }

    /// `--cascade` without `--yes` refuses and publishes nothing.
    #[tokio::test]
    async fn cascade_without_yes_refuses_and_publishes_nothing() {
        let keys = Keys::generate();
        let (client, submitted) = client_against(&keys, "platform").await;

        let err = cmd_delete(&client, "platform", true, false, false)
            .await
            .expect_err("unconfirmed cascade must fail");

        assert!(
            matches!(err, CliError::Usage(_)),
            "expected CliError::Usage, got {err:?}"
        );
        assert!(
            submitted_kinds(&submitted).is_empty(),
            "an unconfirmed cascade must publish nothing"
        );
    }

    /// A confirmed cascade deletes both channels (transport included) and the
    /// workflow, and publishes the kind:30621 tombstone LAST.
    #[tokio::test]
    async fn confirmed_cascade_publishes_the_project_tombstone_last() {
        let keys = Keys::generate();
        let (client, submitted) = client_against(&keys, "platform").await;

        cmd_delete(&client, "platform", true, false, true)
            .await
            .expect("confirmed cascade must succeed");

        // 9008 × 2 channels (stream + transport), kind:5 × 1 workflow, then
        // kind:5 for the project.
        assert_eq!(submitted_kinds(&submitted), vec![9008, 9008, 5, 5]);

        let events = submitted.lock().expect("submitted lock").clone();
        let last = events.last().expect("at least one submission");
        let coordinate =
            projects_cascade::project_coordinate(&keys.public_key().to_hex(), "platform");
        let a_tag = last
            .get("tags")
            .and_then(Value::as_array)
            .and_then(|tags| tags.first().cloned())
            .expect("tombstone a tag");
        assert_eq!(
            a_tag,
            json!(["a", coordinate]),
            "the final submission must be the kind:30621 project tombstone"
        );
        // The transport channel was one of the deleted channels.
        let deleted_channels: Vec<String> = events
            .iter()
            .filter(|e| e.get("kind").and_then(Value::as_u64) == Some(9008))
            .filter_map(|e| {
                e.get("tags")
                    .and_then(Value::as_array)
                    .and_then(|tags| tags.first())
                    .and_then(|t| t.get(1))
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .collect();
        assert!(
            deleted_channels.iter().any(|id| id == CHANNEL_TRANSPORT),
            "the transport channel must be deleted by the cascade, got {deleted_channels:?}"
        );
    }

    /// A cascade whose enumeration found no children must STILL stop at the
    /// confirmation gate. Regression: `has_no_children()` used to short-circuit
    /// the `!yes` branch, so a failed enumeration published the tombstone and
    /// printed `"status":"ok"` with an empty `deleted` list.
    #[tokio::test]
    async fn empty_cascade_plan_does_not_bypass_confirmation() {
        let keys = Keys::generate();
        let (client, submitted) = client_against_fixture(
            &keys,
            "platform",
            Fixture {
                head_tags: Vec::new(),
                channels: Vec::new(),
                workflows: Vec::new(),
            },
        )
        .await;

        let err = cmd_delete(&client, "platform", true, false, false)
            .await
            .expect_err("a childless cascade must still require --yes");

        match err {
            CliError::Usage(message) => assert!(
                message.contains("enumerated NO channels"),
                "the refusal must name the empty enumeration, got {message:?}"
            ),
            other => panic!("expected CliError::Usage, got {other:?}"),
        }
        assert!(
            submitted_kinds(&submitted).is_empty(),
            "an unconfirmed cascade must publish nothing — not even the tombstone"
        );
    }

    /// A channel bound to the project only by the head's forward
    /// `["channel", …]` ref is enumerated and deleted. Regression: the cascade
    /// matched the relay's back-reference alone, so forward-ref-only channels
    /// were invisible and the project was tombstoned over live children.
    #[tokio::test]
    async fn cascade_enumerates_head_forward_ref_channels() {
        let keys = Keys::generate();
        let coordinate =
            projects_cascade::project_coordinate(&keys.public_key().to_hex(), "platform");
        let (client, submitted) =
            client_against_fixture(
                &keys,
                "platform",
                Fixture {
                    head_tags: vec![
                        Tag::parse(["channel", CHANNEL_FORWARD_ONLY]).expect("channel tag")
                    ],
                    channels: vec![
                        channel_metadata(CHANNEL_STREAM, "general", "stream", Some(&coordinate)),
                        // No `project` back-reference — bound by the head only.
                        channel_metadata(CHANNEL_FORWARD_ONLY, "design", "forum", None),
                    ],
                    workflows: Vec::new(),
                },
            )
            .await;

        cmd_delete(&client, "platform", true, false, true)
            .await
            .expect("confirmed cascade must succeed");

        let events = submitted.lock().expect("submitted lock").clone();
        let deleted_channels: Vec<String> = events
            .iter()
            .filter(|e| e.get("kind").and_then(Value::as_u64) == Some(9008))
            .filter_map(|e| {
                e.get("tags")
                    .and_then(Value::as_array)
                    .and_then(|tags| tags.first())
                    .and_then(|t| t.get(1))
                    .and_then(Value::as_str)
                    .map(String::from)
            })
            .collect();
        assert_eq!(
            deleted_channels,
            vec![CHANNEL_STREAM.to_string(), CHANNEL_FORWARD_ONLY.to_string()],
            "both bindings must enumerate; the forward-ref-only channel must not be skipped"
        );
    }

    /// The default (no `--cascade`) path publishes exactly one event: the
    /// project tombstone. No channel or workflow is touched.
    #[tokio::test]
    async fn default_delete_publishes_only_the_project_tombstone() {
        let keys = Keys::generate();
        let (client, submitted) = client_against(&keys, "platform").await;

        cmd_delete(&client, "platform", false, false, false)
            .await
            .expect("default delete must succeed");

        assert_eq!(
            submitted_kinds(&submitted),
            vec![5],
            "the default delete must remain a single kind:5 tombstone"
        );
    }
}
