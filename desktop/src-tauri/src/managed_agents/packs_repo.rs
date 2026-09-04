//! Creating a project's packs repository, from inside the app.
//!
//! # Why this exists
//!
//! *"What happens if another team tried setting it up?"* — no team reads a
//! runbook. Before this, giving a project its own versioned role packs meant
//! knowing to announce a kind:30617, knowing the relay's git URL shape,
//! knowing to seed it from somewhere, and knowing to publish a kind:30624
//! afterwards. Four pieces of folklore, none of them on any screen.
//!
//! [`project_packs_init`] is those four steps as one host command, so the app
//! path shells nothing: Project settings calls it, and it is the same three
//! wire facts `bee packs init` produces from the CLI.
//!
//! # What it does, in order
//!
//! 1. **Announce** a kind:30617 under the viewer's own key, `d` defaulting to
//!    `<project-slug>-packs` but caller-chosen (LANE-L30: one packs
//!    repository can serve every project, rather than one per project), the
//!    `name` tag defaulting to that same id, carrying the project coordinate
//!    as a back reference and the relay's clone URL.
//! 2. **Seed** a fresh repository in this host's packs cache from the packs
//!    this build ships, one commit, and **push** it to the relay over the
//!    existing `git-credential-nostr` helper.
//! 3. **Publish** a kind:30624 naming the repository — but only if the push
//!    landed. A pack source pointing at an empty repository is a promise the
//!    hire would then have to break, and the whole point of the refusal
//!    sentence is that we do not make that promise.
//!
//! Every step reports the wire fact it produced — the event ids, the commit,
//! whether the push went out — because a setup flow that says "done" without
//! them is exactly the completion report this project does not accept.

use std::path::Path;

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::commands::project_git_exec::{run_git, GitAuthConfig};
use crate::managed_agents::packs_cache;

/// Kind of a project's pack-source record — read from the registry in
/// `buzz-core` rather than restated, so the number cannot drift.
const KIND_PROJECT_PACK_SOURCE: u16 = buzz_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16;

/// Kind of a NIP-34 git repository announcement.
const KIND_REPO_ANNOUNCEMENT: u16 = 30617;

/// Kind of the relay-derived ref state a push produces (`buzz_core::kind`).
const KIND_REPO_REF_STATE: u16 = 30618;

/// Schema string in a 30624's content, as `buzz-core` declares it.
const PACK_SOURCE_SCHEMA: &str = buzz_core_pkg::project_pack_source::PROJECT_PACK_SOURCE_SCHEMA;

/// Suffix appended to a project's slug to name its packs repository.
pub const PACKS_REPO_SUFFIX: &str = "-packs";

/// Branch the seeded repository publishes.
const SEED_BRANCH: &str = "main";

/// Message on the one commit the seed writes.
const SEED_COMMIT_MESSAGE: &str = "seed role packs from the shipped defaults";

/// What creating a packs repository actually produced.
///
/// The first four keys are the cross-lane contract (`repoRef`,
/// `sourceEventId`, `seedCommitSha`, `pushRecordEventId`); the rest are what
/// this host can say for certain and a screen that only renders "created"
/// would hide. A caller that reports success without reading [`Self::pushed`]
/// is announcing a repository that may hold nothing.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPacksInit {
    /// The repository coordinate, `30617:<viewer-hex>:<id>`.
    pub repo_ref: String,
    /// Event id of the kind:30624 pack source, or `null` when it was withheld
    /// — which happens exactly when the push did not land.
    pub source_event_id: Option<String>,
    /// The seed commit, lowercase 40-hex.
    pub seed_commit_sha: String,
    /// Event id of the relay-signed kind:30618 ref state that records the
    /// push, read back from the relay after it landed.
    ///
    /// `null` when the push did not land, and also when it did but the relay
    /// had not yet published the record when this command looked. **Never
    /// fabricated**: this is the relay's own event, not ours, and an id we
    /// invented would send a reader to something that does not exist.
    pub push_record_event_id: Option<String>,
    /// The repository's `d` tag.
    pub repo_id: String,
    /// The relay git URL the repository is served at.
    pub clone_url: String,
    /// Event id of the kind:30617 announcement.
    pub announcement_event_id: String,
    /// The branch the seed commit is on.
    pub branch: String,
    /// The role directories seeded, in the order they were written.
    pub roles: Vec<String>,
    /// Whether the push reached the relay.
    pub pushed: bool,
    /// The push's own words when it did not. `null` when it did.
    pub push_error: Option<String>,
    /// The relay's refusal of a published event, when one was refused.
    pub publication_error: Option<String>,
}

/// Split `30621:<owner-hex>:<slug>` into its owner and slug.
pub fn parse_project_coordinate(coordinate: &str) -> Result<(String, String), String> {
    let mut parts = coordinate.splitn(3, ':');
    let kind = parts.next().unwrap_or_default();
    let owner = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();
    if kind != "30621" {
        return Err(format!(
            "a project coordinate must be 30621:<owner>:<slug>, not {coordinate:?}"
        ));
    }
    if !crate::managed_agents::is_lowercase_hex_pubkey(owner) {
        return Err("a project owner must be 64-character lowercase hex".to_string());
    }
    if slug.is_empty() || slug.len() > 64 {
        return Err(format!("{slug:?} is not a project slug"));
    }
    Ok((owner.to_string(), slug.to_string()))
}

/// The repository id a project's packs get by default: `<slug>-packs`.
///
/// Derived rather than asked for, because the one thing a person setting this
/// up should not have to invent is a name — and a derived id makes the
/// repository findable from the project alone.
pub fn default_packs_repo_id(project_slug: &str) -> Result<String, String> {
    let slug: String = project_slug
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        return Err("a project slug that is only punctuation cannot name a repository".to_string());
    }
    // The suffix must survive the length bound rather than be truncated away:
    // a repository called `<slug>` instead of `<slug>-packs` would collide
    // with the project's own code repository.
    let room = 64 - PACKS_REPO_SUFFIX.len();
    let head: String = slug.chars().take(room).collect();
    Ok(format!("{}{PACKS_REPO_SUFFIX}", head.trim_end_matches('-')))
}

/// The name a packs repository's kind:30617 carries: whatever the caller
/// typed (trimmed), or `repo_id` when they left it blank.
///
/// Mirrors the Packs settings screen's own default (LANE-L30: "a short name
/// field defaulting to the id") so a caller that omits `name` altogether — an
/// older client, a script — gets the same answer the form would have sent,
/// rather than a second, undocumented default drifting out of sync with it.
pub fn resolved_repo_name(name: Option<&str>, repo_id: &str) -> String {
    match name.map(str::trim).filter(|n| !n.is_empty()) {
        Some(name) => name.to_string(),
        None => repo_id.to_string(),
    }
}

/// Copy `from` into `to`, directories and files, following no symlinks.
///
/// Deliberately not a `cp -R`: a symlink in the shipped packs would otherwise
/// be seeded into a repository other people clone, pointing at a path on the
/// machine that happened to publish it.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| format!("create {}: {error}", to.display()))?;
    let entries =
        std::fs::read_dir(from).map_err(|error| format!("read {}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read {}: {error}", from.display()))?;
        // `DirEntry::file_type` does not follow the link; `metadata()` would,
        // and a symlink to a directory would then be copied as its contents.
        let file_type = entry
            .file_type()
            .map_err(|error| format!("stat {}: {error}", entry.path().display()))?;
        let target = to.join(entry.file_name());
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &target)
                .map_err(|error| format!("copy {}: {error}", entry.path().display()))?;
        }
    }
    Ok(())
}

/// Seed `checkout` from `shipped` and commit once. Returns `(commit, roles)`.
///
/// Idempotent in the only sense that matters here: a checkout that already
/// exists is cleared first, so a retry after a failed push seeds the same tree
/// rather than layering a second copy over the first.
pub fn seed_packs_checkout(
    checkout: &Path,
    shipped: &Path,
    auth: &GitAuthConfig,
) -> Result<(String, Vec<String>), String> {
    if checkout.exists() {
        std::fs::remove_dir_all(checkout)
            .map_err(|error| format!("clear {}: {error}", checkout.display()))?;
    }
    std::fs::create_dir_all(checkout)
        .map_err(|error| format!("create {}: {error}", checkout.display()))?;
    let roles_dir = checkout.join(packs_cache::DEFAULT_PACK_PATH);
    copy_tree(shipped, &roles_dir)?;

    let mut roles: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(&roles_dir)
        .map_err(|error| format!("read {}: {error}", roles_dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("read {}: {error}", roles_dir.display()))?;
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if packs_cache::role_persona_in_pack(&entry.path(), &name).is_some() {
            roles.push(name);
        }
    }
    roles.sort();
    if roles.is_empty() {
        return Err(
            "this build ships no role packs, so there is nothing to seed a repository with"
                .to_string(),
        );
    }

    run_git(
        &["init", "--quiet", "--initial-branch", SEED_BRANCH],
        Some(checkout),
        auth,
    )?;
    if !checkout.join(".git").is_dir() {
        return Err("git init produced no repository".to_string());
    }
    run_git(&["add", "--all"], Some(checkout), auth)?;
    run_git(
        &["commit", "--quiet", "-m", SEED_COMMIT_MESSAGE],
        Some(checkout),
        auth,
    )?;
    let commit = run_git(&["rev-parse", "HEAD"], Some(checkout), auth)?
        .trim()
        .to_string();
    Ok((commit, roles))
}

/// Build the kind:30617 announcement for a project's packs repository.
///
/// `name` is whatever the caller resolved (a viewer-typed name, or its own
/// default of `repo_id` — see [`project_packs_init`]); this function does not
/// invent one, so the announcement's `name` tag always says exactly what was
/// asked for.
fn build_announcement(
    keys: &Keys,
    repo_id: &str,
    project: &str,
    name: &str,
    clone_url: &str,
) -> Result<nostr::Event, String> {
    let name = name.to_string();
    let description =
        "Role packs for this project's agent seats. Seeded from Beekeeper's shipped defaults."
            .to_string();
    let tags = vec![
        Tag::parse(vec!["d".to_string(), repo_id.to_string()])
            .map_err(|error| format!("invalid d tag: {error}"))?,
        Tag::parse(vec!["name".to_string(), name])
            .map_err(|error| format!("invalid name tag: {error}"))?,
        Tag::parse(vec!["description".to_string(), description])
            .map_err(|error| format!("invalid description tag: {error}"))?,
        Tag::parse(vec!["clone".to_string(), clone_url.to_string()])
            .map_err(|error| format!("invalid clone tag: {error}"))?,
        // The back reference, so the packs repository is reachable from the
        // project rather than only from whoever remembers its name.
        Tag::parse(vec!["project".to_string(), project.to_string()])
            .map_err(|error| format!("invalid project tag: {error}"))?,
    ];
    EventBuilder::new(Kind::Custom(KIND_REPO_ANNOUNCEMENT), String::new())
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the repository announcement: {error}"))
}

/// Build the kind:30624 pack source for `project`, naming `repo`.
fn build_pack_source(keys: &Keys, project: &str, repo: &str) -> Result<nostr::Event, String> {
    let content = serde_json::json!({ "schema": PACK_SOURCE_SCHEMA }).to_string();
    let tags = vec![
        Tag::parse(vec!["d".to_string(), project.to_string()])
            .map_err(|error| format!("invalid d tag: {error}"))?,
        Tag::parse(vec!["repo".to_string(), repo.to_string()])
            .map_err(|error| format!("invalid repo tag: {error}"))?,
        Tag::parse(vec!["ref".to_string(), format!("refs/heads/{SEED_BRANCH}")])
            .map_err(|error| format!("invalid ref tag: {error}"))?,
        Tag::parse(vec![
            "path".to_string(),
            packs_cache::DEFAULT_PACK_PATH.to_string(),
        ])
        .map_err(|error| format!("invalid path tag: {error}"))?,
    ];
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE), content)
        .tags(tags)
        .sign_with_keys(keys)
        .map_err(|error| format!("sign the pack source: {error}"))
}

/// Give a project its own packs repository, seeded from the shipped defaults.
///
/// Announces, seeds, pushes and publishes — see the module docs for the order
/// and for why the 30624 is withheld when the push does not land. The viewer's
/// own key signs everything and pushes; the relay decides whether that key may
/// announce inside this project, and its refusal is returned verbatim rather
/// than re-worded into something friendlier and less true.
///
/// `repo_id` and `name` are both caller-chosen and both optional: an absent
/// or blank `repo_id` falls back to [`default_packs_repo_id`], and an absent
/// or blank `name` falls back to whatever `repo_id` resolved to. Passing the
/// same `repo_id` from more than one project's "Create packs repository" is
/// exactly how one packs repository ends up serving all of them.
#[tauri::command]
pub async fn project_packs_init(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    repo_id: Option<String>,
    // `#[serde(default)]` has no effect on a bare command parameter (Tauri
    // already treats a missing key as `None` for an `Option<T>` argument —
    // see `repo_id` above); documented here so a reader does not go looking
    // for an attribute that would be a no-op on a fn parameter.
    name: Option<String>,
) -> Result<ProjectPacksInit, String> {
    let project = project_ref;
    let (_owner, project_slug) = parse_project_coordinate(project.trim())?;
    let repo_id = match repo_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        Some(id) => id.to_string(),
        None => default_packs_repo_id(&project_slug)?,
    };
    let name = resolved_repo_name(name.as_deref(), &repo_id);
    let keys = state.signing_keys()?;
    let viewer = keys.public_key().to_hex();
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(&state));
    let clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &repo_id);
    let repo = format!("30617:{viewer}:{repo_id}");
    // Refuse before anything is signed if the coordinate we would announce is
    // not one this host would later stage from.
    packs_cache::parse_repo_coordinate(&repo)?;

    let shipped = packs_cache::shipped_packs_dir(&app)
        .ok_or_else(|| "this build ships no role packs to seed a repository with".to_string())?;
    let packs_root = packs_cache::packs_root(&app)?;
    let checkout = packs_cache::packs_checkout_dir(&packs_root, &viewer, &repo_id);

    let announcement = build_announcement(&keys, &repo_id, project.trim(), &name, &clone_url)?;
    let mut publication_error =
        crate::relay::submit_signed_event_with_keys(&announcement, &state, &keys, None)
            .await
            .err();

    let seed_keys = keys.clone();
    let seed_checkout = checkout.clone();
    let seed_clone_url = clone_url.clone();
    let seeded = tokio::task::spawn_blocking(
        move || -> Result<(String, Vec<String>, Option<String>), String> {
            let auth =
                crate::commands::project_git_exec::build_git_auth_config_for_keys(&seed_keys)?;
            let (commit, roles) = seed_packs_checkout(&seed_checkout, &shipped, &auth)?;
            let push_error = run_git(
                &[
                    "push",
                    "--quiet",
                    "--",
                    &seed_clone_url,
                    &format!("HEAD:refs/heads/{SEED_BRANCH}"),
                ],
                Some(&seed_checkout),
                &auth,
            )
            .err();
            Ok((commit, roles, push_error))
        },
    )
    .await
    .map_err(|error| format!("seeding the packs repository did not finish: {error}"))??;
    let (commit, roles, push_error) = seeded;

    // The 30624 is published only when the repository actually holds the
    // packs it names. A source pointing at an empty repository turns every
    // later hire into HIRE_PACK_UNAVAILABLE, which is a promise broken later
    // instead of a failure reported now.
    let source_event_id = if push_error.is_none() {
        let source = build_pack_source(&keys, project.trim(), &repo)?;
        if let Some(error) =
            crate::relay::submit_signed_event_with_keys(&source, &state, &keys, None)
                .await
                .err()
        {
            publication_error.get_or_insert(error);
            None
        } else {
            Some(source.id.to_hex())
        }
    } else {
        None
    };

    // The relay derives a kind:30618 ref state from the push. Read it back
    // rather than assert it: the id belongs to an event the relay signed, and
    // an id we made up would point a reader at nothing.
    let push_record_event_id = if push_error.is_none() {
        read_push_record_id(&state, &viewer, &repo_id).await
    } else {
        None
    };

    Ok(ProjectPacksInit {
        repo_ref: repo,
        source_event_id,
        seed_commit_sha: commit,
        push_record_event_id,
        repo_id,
        clone_url,
        announcement_event_id: announcement.id.to_hex(),
        branch: SEED_BRANCH.to_string(),
        roles,
        pushed: push_error.is_none(),
        push_error,
        publication_error,
    })
}

/// The relay-signed kind:30618 ref state for this repository, if it is there.
///
/// A failure to read is reported as `None`, not as a failed init: the push
/// landed either way, and the record is an observation about it rather than
/// part of it.
async fn read_push_record_id(state: &AppState, owner: &str, repo_id: &str) -> Option<String> {
    let filter = serde_json::json!({
        "kinds": [KIND_REPO_REF_STATE],
        "#d": [repo_id],
        "limit": 1,
    });
    match crate::relay::query_relay(state, &[filter]).await {
        Ok(events) => events.first().map(|event| event.id.to_hex()),
        Err(error) => {
            tracing::debug!(
                %owner,
                %repo_id,
                %error,
                "the packs repository pushed, but its ref-state record could not be read back"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "packs_repo_tests.rs"]
mod tests;
