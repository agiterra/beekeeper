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
//!
//! # Identity and rollback (LANE-L31, Finding 66)
//!
//! The seed commit is authored as this host's own identity — its `display_name`
//! read off the kind:0 it already has (falling back to `Beekeeper <pubkey8>`),
//! and `<pubkey8>@beekeeper.local` for the email — never left to git's own
//! hostname auto-detection. Production runs every git invocation with global
//! and system config cleared (`project_git_exec::configure_git_auth`), so
//! without an identity of its own git either refuses outright ("Author
//! identity unknown … unable to auto-detect email address") on a host whose
//! hostname has no dot, or silently authors the commit as `user@hostname`
//! elsewhere — neither of which is this host's identity.
//!
//! If the seed or the push fails *after* the announcement has landed, the
//! announcement is withdrawn (a kind:5 tombstone, [`withdraw_announcement`])
//! rather than left as a stray, packless repository under the viewer's key.
//! A tombstone failure is itself reported, never silently dropped — the
//! coordinate is still in the result for a founder to delete by hand.

use std::path::Path;

use nostr::{EventBuilder, Keys, Kind, Tag};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::app_state::AppState;
use crate::commands::project_git_exec::{run_git, GitAuthConfig};
use crate::managed_agents::packs_cache;

/// A viewer's own kind:0, or absence of one, resolved to the name and email
/// a git commit made on their behalf is authored as.
///
/// Reads `display_name`, falling back to `name` (the same fallback
/// `nostr_convert::profile_info_from_event` uses for every other profile
/// read in this app), from whatever kind:0 content is passed in; an absent
/// or empty name falls back to `Beekeeper <pubkey8>`. The email is always
/// `<pubkey8>@beekeeper.local` — not a real mailbox, an address stable to
/// the *key* that authored the commit rather than to a display name that can
/// change. Pure and synchronous so it is testable without a relay; see
/// [`resolve_app_commit_identity`] for the async wrapper that reads the
/// profile.
pub(crate) fn app_commit_identity_from_profile(
    pubkey_hex: &str,
    profile_content: Option<&str>,
) -> (String, String) {
    let short = pubkey_short(pubkey_hex);
    // One spelling of this address, in `beekeeper_core_pkg::seat_commit_identity`:
    // the app's own commits and every seat worktree the host configures must
    // resolve a key to the same author (ledger 239).
    let email = beekeeper_core_pkg::seat_commit_identity::beekeeper_local_email(pubkey_hex);
    let name = profile_content
        .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok())
        .and_then(|value| {
            value
                .get("display_name")
                .and_then(serde_json::Value::as_str)
                .or_else(|| value.get("name").and_then(serde_json::Value::as_str))
                .map(str::to_string)
        })
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("Beekeeper {short}"));
    (name, email)
}

/// The first 8 characters of a hex pubkey — `""` for anything shorter, which
/// never happens for a real 64-hex key but keeps this total rather than
/// panicking on a malformed one.
fn pubkey_short(pubkey_hex: &str) -> String {
    pubkey_hex.chars().take(8).collect()
}

/// [`app_commit_identity_from_profile`], reading this host's own kind:0 off
/// the relay. A read failure or no profile falls back exactly as an empty
/// profile would — a git commit must never block on the profile query, and
/// never has less to say than "the key's own name" when the query fails.
pub(crate) async fn resolve_app_commit_identity(
    state: &AppState,
    pubkey_hex: &str,
) -> (String, String) {
    let events = crate::relay::query_relay(
        state,
        &[serde_json::json!({ "kinds": [0], "authors": [pubkey_hex], "limit": 1 })],
    )
    .await
    .unwrap_or_default();
    app_commit_identity_from_profile(
        pubkey_hex,
        events.first().map(|event| event.content.as_str()),
    )
}

/// Kind of a project's pack-source record — read from the registry in
/// `buzz-core` rather than restated, so the number cannot drift.
const KIND_PROJECT_PACK_SOURCE: u16 = beekeeper_core_pkg::kind::KIND_PROJECT_PACK_SOURCE as u16;

/// Kind of a NIP-34 git repository announcement.
pub(crate) const KIND_REPO_ANNOUNCEMENT: u16 = 30617;

/// Kind of the relay-derived ref state a push produces (`beekeeper_core::kind`).
pub(crate) const KIND_REPO_REF_STATE: u16 = 30618;

/// Schema string in a 30624's content, as `buzz-core` declares it.
const PACK_SOURCE_SCHEMA: &str =
    beekeeper_core_pkg::project_pack_source::PROJECT_PACK_SOURCE_SCHEMA;

/// Suffix appended to a project's slug to name its packs repository.
pub const PACKS_REPO_SUFFIX: &str = "-packs";

/// Branch the seeded repository publishes.
pub(crate) const SEED_BRANCH: &str = "main";

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
    /// — which happens whenever the push did not land, including a seed that
    /// never reached the push step.
    pub source_event_id: Option<String>,
    /// The seed commit, lowercase 40-hex, or `null` when seeding itself
    /// failed (see [`Self::seed_error`]) — there is no commit to name.
    pub seed_commit_sha: Option<String>,
    /// The seed step's own words when it failed before there was anything to
    /// push. `null` when seeding succeeded (including when the *push*
    /// afterwards failed — see [`Self::push_error`] for that half).
    pub seed_error: Option<String>,
    /// The display name the seed commit was (or would have been) authored
    /// as — this host's own kind:0 `display_name`/`name`, or `Beekeeper
    /// <pubkey8>` when neither is set. **Finding 66**: never git's own
    /// hostname-derived guess.
    pub commit_identity_name: String,
    /// The email the seed commit was (or would have been) authored as:
    /// `<pubkey8>@beekeeper.local`, stable to the signing key.
    pub commit_identity_email: String,
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
    /// The role directories seeded, in the order they were written. Empty
    /// when seeding failed before any role was written.
    pub roles: Vec<String>,
    /// Whether the push reached the relay. `false` whenever seeding itself
    /// failed too — there was nothing to push.
    pub pushed: bool,
    /// The push's own words when it did not land (seeding having
    /// succeeded). `null` when it did, or when seeding never got that far
    /// (see [`Self::seed_error`] instead).
    pub push_error: Option<String>,
    /// The relay's refusal of a published event, when one was refused.
    pub publication_error: Option<String>,
    /// Event id of the kind:5 tombstone withdrawing the kind:30617
    /// announcement, published when the seed or push failed *after* the
    /// announcement had already landed — see the module docs' rollback
    /// step. `null` when nothing needed withdrawing.
    pub announcement_withdrawn_event_id: Option<String>,
    /// The withdrawal's own words when publishing the tombstone itself
    /// failed. [`Self::repo_ref`] is the coordinate a founder can delete by
    /// hand in that case (`bee repos delete`).
    pub announcement_withdrawal_error: Option<String>,
}

/// What the seed step (git init/copy/commit, then push) produced.
enum SeedOutcome {
    /// Seeding wrote a commit; the push may still have failed.
    Seeded {
        commit: String,
        roles: Vec<String>,
        push_error: Option<String>,
    },
    /// Seeding itself never produced a commit to push.
    SeedFailed { seed_error: String },
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
    default_repo_id(project_slug, PACKS_REPO_SUFFIX)
}

/// `<slug><suffix>`, sanitized to `[a-z0-9._-]` and bounded to 64 bytes
/// with the suffix kept whole: a truncated suffix would collide with the
/// project's own code repository.
pub fn default_repo_id(project_slug: &str, suffix: &str) -> Result<String, String> {
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
    let room = 64usize.saturating_sub(suffix.len());
    let head: String = slug.chars().take(room).collect();
    Ok(format!("{}{suffix}", head.trim_end_matches('-')))
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
pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
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
    build_repo_announcement(
        keys,
        repo_id,
        project,
        name,
        "Role packs for this project's agent seats. Seeded from Beekeeper's shipped defaults.",
        clone_url,
    )
}

/// Build a kind:30617 announcement for a repository this host creates
/// inside `project`: `d`, `name`, `description`, `clone`, and the project
/// back reference.
pub(crate) fn build_repo_announcement(
    keys: &Keys,
    repo_id: &str,
    project: &str,
    name: &str,
    description: &str,
    clone_url: &str,
) -> Result<nostr::Event, String> {
    let name = name.to_string();
    let description = description.to_string();
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
    let shipped = packs_cache::shipped_packs_dir(&app)
        .ok_or_else(|| "this build ships no role packs to seed a repository with".to_string())?;
    let packs_root = packs_cache::packs_root(&app)?;
    project_packs_init_with_paths(&state, project_ref, repo_id, name, shipped, packs_root).await
}

/// [`project_packs_init`]'s body, taking the shipped-packs directory and the
/// packs cache root directly instead of an [`AppHandle`] to resolve them.
///
/// The split exists for tests: `shipped_packs_dir`/`packs_root` need a real
/// Tauri app, which this crate's tests do not construct, while everything
/// below — the announce/seed/push/publish sequence, the identity resolution,
/// and the rollback this lane adds — needs only [`AppState`] and a relay,
/// both of which `build_app_state()` plus a stub HTTP server already give
/// [`crate::managed_agents::persona_events`]'s own tests.
async fn project_packs_init_with_paths(
    state: &AppState,
    project_ref: String,
    repo_id: Option<String>,
    name: Option<String>,
    shipped: std::path::PathBuf,
    packs_root: std::path::PathBuf,
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
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let clone_url = packs_cache::packs_clone_url(&relay_http, &viewer, &repo_id);
    let repo = format!("30617:{viewer}:{repo_id}");
    // Refuse before anything is signed if the coordinate we would announce is
    // not one this host would later stage from.
    packs_cache::parse_repo_coordinate(&repo)?;

    let checkout = packs_cache::packs_checkout_dir(&packs_root, &viewer, &repo_id);

    // Finding 66: the seed commit is authored as this host's own identity —
    // read from the kind:0 it already has, never from git config (production
    // clears every layer of that before any git invocation runs). Resolved
    // before the blocking seed step because reading the relay needs the
    // async runtime the blocking thread does not have.
    let (commit_identity_name, commit_identity_email) =
        resolve_app_commit_identity(state, &viewer).await;

    let announcement = build_announcement(&keys, &repo_id, project.trim(), &name, &clone_url)?;
    let mut publication_error =
        crate::relay::submit_signed_event_with_keys(&announcement, state, &keys, None)
            .await
            .err();

    // Nothing landed on the relay to roll back, seed, or push if the
    // announcement itself was refused — there is no promise yet.
    if publication_error.is_some() {
        return Ok(ProjectPacksInit {
            repo_ref: repo,
            source_event_id: None,
            seed_commit_sha: None,
            seed_error: None,
            commit_identity_name,
            commit_identity_email,
            push_record_event_id: None,
            repo_id,
            clone_url,
            announcement_event_id: announcement.id.to_hex(),
            branch: SEED_BRANCH.to_string(),
            roles: Vec::new(),
            pushed: false,
            push_error: None,
            publication_error,
            announcement_withdrawn_event_id: None,
            announcement_withdrawal_error: None,
        });
    }

    let seed_keys = keys.clone();
    let seed_checkout = checkout.clone();
    let seed_clone_url = clone_url.clone();
    let identity = (commit_identity_name.clone(), commit_identity_email.clone());
    let seeded: SeedOutcome =
        tokio::task::spawn_blocking(move || -> Result<SeedOutcome, String> {
            let mut auth =
                crate::commands::project_git_exec::build_git_auth_config_for_keys(&seed_keys)?;
            auth.set_commit_identity(identity.0, identity.1);
            match seed_packs_checkout(&seed_checkout, &shipped, &auth) {
                Err(seed_error) => Ok(SeedOutcome::SeedFailed { seed_error }),
                Ok((commit, roles)) => {
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
                    Ok(SeedOutcome::Seeded {
                        commit,
                        roles,
                        push_error,
                    })
                }
            }
        })
        .await
        .map_err(|error| format!("seeding the packs repository did not finish: {error}"))??;

    let (seed_commit_sha, roles, seed_error, push_error) = match seeded {
        SeedOutcome::SeedFailed { seed_error } => (None, Vec::new(), Some(seed_error), None),
        SeedOutcome::Seeded {
            commit,
            roles,
            push_error,
        } => (Some(commit), roles, None, push_error),
    };
    let seed_or_push_failed = seed_error.is_some() || push_error.is_some();

    // Rollback: the announcement promised a repository with packs in it.
    // When the seed or the push failed after that promise landed, withdraw
    // it rather than leave a stray repository with no packs under the
    // viewer's key — see the module docs' rollback step.
    let (announcement_withdrawn_event_id, announcement_withdrawal_error) = if seed_or_push_failed {
        withdraw_announcement(state, &keys, &repo_id).await
    } else {
        (None, None)
    };

    // The 30624 is published only when the repository actually holds the
    // packs it names. A source pointing at an empty repository turns every
    // later hire into HIRE_PACK_UNAVAILABLE, which is a promise broken later
    // instead of a failure reported now.
    let source_event_id = if !seed_or_push_failed {
        let source = build_pack_source(&keys, project.trim(), &repo)?;
        if let Some(error) =
            crate::relay::submit_signed_event_with_keys(&source, state, &keys, None)
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
    let push_record_event_id = if !seed_or_push_failed {
        read_push_record_id(state, &viewer, &repo_id, SEED_BRANCH).await
    } else {
        None
    };

    Ok(ProjectPacksInit {
        repo_ref: repo,
        source_event_id,
        seed_commit_sha,
        seed_error,
        commit_identity_name,
        commit_identity_email,
        push_record_event_id,
        repo_id,
        clone_url,
        announcement_event_id: announcement.id.to_hex(),
        branch: SEED_BRANCH.to_string(),
        roles,
        pushed: !seed_or_push_failed,
        push_error,
        publication_error,
        announcement_withdrawn_event_id,
        announcement_withdrawal_error,
    })
}

/// Withdraw this host's own `30617:<viewer>:<repo_id>` announcement: a kind:5
/// carrying `["a", "30617:<viewer>:<repo_id>"]`, the same shape `bee repos
/// delete` publishes (`crates/beekeeper-cli/src/commands/repos.rs::cmd_delete_repo`).
///
/// Called only after the announcement is known to have landed and the seed
/// or push that was supposed to fill it then failed — see
/// [`project_packs_init`]. Returns `(withdrawn_event_id, error)`, exactly one
/// `Some`: a tombstone failure is reported, not retried, and the caller
/// already has the coordinate (`ProjectPacksInit::repo_ref`) to hand a
/// founder for a manual `bee repos delete`.
pub(crate) async fn withdraw_announcement(
    state: &AppState,
    keys: &Keys,
    repo_id: &str,
) -> (Option<String>, Option<String>) {
    let owner = keys.public_key().to_hex();
    let builder = match beekeeper_sdk_pkg::build_delete_addressable(
        u32::from(KIND_REPO_ANNOUNCEMENT),
        &owner,
        repo_id,
    ) {
        Ok(builder) => builder,
        Err(error) => {
            return (
                None,
                Some(format!("could not build the withdrawal: {error}")),
            )
        }
    };
    let tombstone = match builder.sign_with_keys(keys) {
        Ok(event) => event,
        Err(error) => {
            return (
                None,
                Some(format!("could not sign the withdrawal: {error}")),
            )
        }
    };
    match crate::relay::submit_signed_event_with_keys(&tombstone, state, keys, None).await {
        Ok(_) => (Some(tombstone.id.to_hex()), None),
        Err(error) => (None, Some(error)),
    }
}

/// The relay-signed kind:30618 ref state that proves `branch` was pushed to
/// this repository, if it is there.
///
/// The relay writes a ref state at the repository's *creation* too — `HEAD`
/// only, no branch — so a record with the right `d` proves nothing about
/// commits (ledger 176: the code seed of RPG Test was skipped as "already
/// had commits" over exactly that record, leaving an unborn clone). Only a
/// record carrying `["refs/heads/<branch>", <sha>]` counts.
pub(crate) async fn read_push_record_id(
    state: &AppState,
    owner: &str,
    repo_id: &str,
    branch: &str,
) -> Option<String> {
    let filter = serde_json::json!({
        "kinds": [KIND_REPO_REF_STATE],
        "#d": [repo_id],
        "limit": 10,
    });
    let wanted = format!("refs/heads/{branch}");
    match crate::relay::query_relay(state, &[filter]).await {
        Ok(events) => events
            .iter()
            .find(|event| {
                event.tags.iter().any(|tag| {
                    let parts = tag.as_slice();
                    parts.first().map(String::as_str) == Some(wanted.as_str())
                        && parts.get(1).is_some_and(|sha| !sha.is_empty())
                })
            })
            .map(|event| event.id.to_hex()),
        Err(error) => {
            tracing::debug!(
                %owner,
                %repo_id,
                %branch,
                %error,
                "the repository pushed, but its ref-state record could not be read back"
            );
            None
        }
    }
}

#[cfg(test)]
#[path = "packs_repo_tests.rs"]
mod tests;
