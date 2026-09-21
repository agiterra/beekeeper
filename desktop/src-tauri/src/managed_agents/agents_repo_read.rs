//! Reading a project's agents repository for the Files tab (spec § 4.12):
//! the listing of `main` and one file's text, always from the packs cache's
//! **fetched tip** (`refs/remotes/origin/<branch>`), never from its working
//! copy — the cache is a detached, force-synced, `git clean`ed checkout
//! whose local `main` never moves after the first clone (ledger 172).
//!
//! Every read says which tip it read and when that tip was fetched, so a
//! reader who declined a refresh is never shown a stale file as current.

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use super::packs_cache;
use crate::commands::project_git_exec::{
    build_git_auth_config, run_git, run_git_bytes, GitAuthConfig,
};
use crate::AppState;

/// The largest file `agents_repo_read` returns as text: the draft cap, so
/// what can be read can be drafted, and a file past it is disclosed rather
/// than silently truncated.
pub const MAX_READ_BYTES: u64 =
    buzz_core_pkg::agents_repo_draft::MAX_AGENTS_REPO_DRAFT_TEXT_BYTES as u64;

/// The project's agents repository as this computer holds it.
#[derive(Clone)]
pub(crate) struct AgentsRepoCheckout {
    /// `30617:<owner>:<id>`.
    pub repo: String,
    /// The branch the source follows (`main`).
    pub branch: String,
    /// The packs-cache checkout.
    pub checkout: PathBuf,
    /// The relay clone URL.
    pub clone_url: String,
    /// `refs/remotes/origin/<branch>` as last fetched.
    pub tip: String,
    /// When the tip was fetched, RFC 3339, or null if unknown.
    pub synced_at: Option<String>,
    /// The git auth for further commands.
    pub auth: GitAuthConfig,
}

/// One entry of the listing.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoEntry {
    /// Path relative to the root.
    pub path: String,
    /// Blob sha.
    pub blob: String,
    /// Size in bytes.
    pub size: u64,
    /// `plan`, `role`, `skill`, `manifest`, `readme`, `archived-role`,
    /// `archived-plan`, `gitkeep` or `other` (outside the layout; read-only).
    pub kind: String,
}

/// The listing of `main`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoListing {
    /// `30617:<owner>:<id>`.
    pub repo: String,
    /// The branch followed.
    pub branch: String,
    /// The tip listed.
    pub commit: String,
    /// When the tip was fetched, or null.
    pub synced_at: Option<String>,
    /// Every blob, in `git ls-tree` order.
    pub entries: Vec<AgentsRepoEntry>,
}

/// One file as `main` has it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoFile {
    /// The path.
    pub path: String,
    /// The text; null when the file is not on `main`, is not UTF-8 text, or
    /// is over [`MAX_READ_BYTES`] — `state` says which.
    pub text: Option<String>,
    /// `on-main`, `not-on-main`, `not-text`, `too-large`.
    pub state: String,
    /// Blob sha, when on `main`.
    pub blob: Option<String>,
    /// The tip read.
    pub commit: String,
    /// Size in bytes, when on `main`.
    pub size: Option<u64>,
    /// When the tip was fetched, or null.
    pub synced_at: Option<String>,
}

/// Resolve the project's agents repository on this computer, syncing the
/// packs cache when `refresh` asks for it or nothing has been fetched yet.
/// Refuses a source that is not an agents repository — a pack layout under
/// a sub-path, or a sha pin with no branch — by name, never falling back.
pub(crate) fn resolve_agents_repo(
    app: &AppHandle,
    state: &AppState,
    source: &packs_cache::ProjectPackSource,
    refresh: bool,
) -> Result<AgentsRepoCheckout, String> {
    if !buzz_core_pkg::project_pack_source::is_root_pack_path(&source.path) {
        return Err(format!(
            "this project's source is a pack-layout repository ({} at path {:?}), not an agents \
             repository; the Files tab edits the flat layout at a repository root",
            source.repo, source.path
        ));
    }
    let Some(git_ref) = source.git_ref.clone() else {
        return Err(format!(
            "this project's source pins a commit ({}) rather than following a branch; drafts \
             land on a branch, so re-point it with `bee packs set-source` first",
            source.sha.as_deref().unwrap_or("?")
        ));
    };
    let branch = git_ref
        .strip_prefix("refs/heads/")
        .unwrap_or(&git_ref)
        .to_owned();
    let root = packs_cache::packs_root(app)?;
    let auth = build_git_auth_config(state)?;
    let relay_http =
        crate::relay::relay_http_base_url(&crate::relay::relay_ws_url_with_override(state));
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    let checkout = packs_cache::packs_checkout_dir(&root, &owner, &id);
    let clone_url = packs_cache::packs_clone_url(&relay_http, &owner, &id);
    crate::commands::project_git_exec::validate_clone_url(&clone_url)?;
    let remote_ref = format!("refs/remotes/origin/{branch}");
    let have_tip = checkout.join(".git").is_dir()
        && run_git(
            &["rev-parse", "--verify", "--quiet", &remote_ref],
            Some(&checkout),
            &auth,
        )
        .is_ok();
    if refresh || !have_tip {
        packs_cache::sync_packs_checkout(&checkout, &clone_url, source, &auth)?;
    }
    let tip = run_git(
        &["rev-parse", "--verify", "--quiet", &remote_ref],
        Some(&checkout),
        &auth,
    )
    .map(|out| out.trim().to_owned())
    .map_err(|error| format!("the agents repository has no {remote_ref} after sync: {error}"))?;
    let synced_at = fetch_time(&checkout);
    Ok(AgentsRepoCheckout {
        repo: source.repo.clone(),
        branch,
        checkout,
        clone_url,
        tip,
        synced_at,
        auth,
    })
}

/// When the checkout last fetched: the mtime of `.git/FETCH_HEAD`, RFC 3339.
fn fetch_time(checkout: &Path) -> Option<String> {
    let modified = std::fs::metadata(checkout.join(".git").join("FETCH_HEAD"))
        .ok()?
        .modified()
        .ok()?;
    let stamp: chrono::DateTime<chrono::Utc> = modified.into();
    Some(stamp.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// Where a path sits in the layout, for the tree's grouping.
pub(crate) fn classify(path: &str) -> &'static str {
    use buzz_core_pkg::agents_repo_draft::{validate_draft_path, DraftPathClass};
    if path.ends_with("/.gitkeep") || path == ".gitkeep" {
        return "gitkeep";
    }
    match validate_draft_path(path) {
        Ok(DraftPathClass::RootFile) if path == "README.md" => "readme",
        Ok(DraftPathClass::RootFile) => "manifest",
        Ok(DraftPathClass::Role) => "role",
        Ok(DraftPathClass::ArchivedRole) => "archived-role",
        Ok(DraftPathClass::RoleSkill | DraftPathClass::SharedSkill) => "skill",
        Ok(DraftPathClass::Plan) => "plan",
        Ok(DraftPathClass::ArchivedPlan) => "archived-plan",
        Err(_) => "other",
    }
}

/// List every blob at the tip.
pub(crate) fn list_tip(repo: &AgentsRepoCheckout) -> Result<AgentsRepoListing, String> {
    let out = run_git_bytes(
        &["ls-tree", "-r", "-l", "-z", &repo.tip],
        Some(&repo.checkout),
        &repo.auth,
        &[],
    )?;
    let mut entries = Vec::new();
    for record in out
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let record = String::from_utf8_lossy(record);
        let Some((meta, name)) = record.split_once('\t') else {
            continue;
        };
        let fields: Vec<&str> = meta.split_whitespace().collect();
        let [_mode, kind, oid, size] = fields.as_slice() else {
            continue;
        };
        if *kind != "blob" {
            continue;
        }
        entries.push(AgentsRepoEntry {
            path: name.to_owned(),
            blob: (*oid).to_owned(),
            size: size.parse().unwrap_or(0),
            kind: classify(name).to_owned(),
        });
    }
    Ok(AgentsRepoListing {
        repo: repo.repo.clone(),
        branch: repo.branch.clone(),
        commit: repo.tip.clone(),
        synced_at: repo.synced_at.clone(),
        entries,
    })
}

/// The blob at `path` on the tip, or `None` when absent.
pub(crate) fn blob_at_tip(repo: &AgentsRepoCheckout, path: &str) -> Option<String> {
    run_git(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{}:{path}", repo.tip),
        ],
        Some(&repo.checkout),
        &repo.auth,
    )
    .ok()
    .map(|out| out.trim().to_owned())
    .filter(|sha| sha.len() == 40)
}

/// Read one file at the tip.
pub(crate) fn read_tip(repo: &AgentsRepoCheckout, path: &str) -> Result<AgentsRepoFile, String> {
    buzz_core_pkg::agents_repo_draft::validate_draft_path(path)?;
    let Some(blob) = blob_at_tip(repo, path) else {
        return Ok(AgentsRepoFile {
            path: path.to_owned(),
            text: None,
            state: "not-on-main".into(),
            blob: None,
            commit: repo.tip.clone(),
            size: None,
            synced_at: repo.synced_at.clone(),
        });
    };
    let size: u64 = run_git(&["cat-file", "-s", &blob], Some(&repo.checkout), &repo.auth)?
        .trim()
        .parse()
        .unwrap_or(u64::MAX);
    if size > MAX_READ_BYTES {
        return Ok(AgentsRepoFile {
            path: path.to_owned(),
            text: None,
            state: "too-large".into(),
            blob: Some(blob),
            commit: repo.tip.clone(),
            size: Some(size),
            synced_at: repo.synced_at.clone(),
        });
    }
    let bytes = run_git_bytes(
        &["cat-file", "blob", &blob],
        Some(&repo.checkout),
        &repo.auth,
        &[],
    )?;
    let (text, state) = match String::from_utf8(bytes) {
        Ok(text) => (Some(text), "on-main"),
        Err(_) => (None, "not-text"),
    };
    Ok(AgentsRepoFile {
        path: path.to_owned(),
        text,
        state: state.into(),
        blob: Some(blob),
        commit: repo.tip.clone(),
        size: Some(size),
        synced_at: repo.synced_at.clone(),
    })
}
