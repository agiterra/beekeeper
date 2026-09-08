//! How the pack revisions other machines *reported* relate to the one this
//! machine's packs checkout is actually on.
//!
//! The Packs tab already knows two things: the commit this computer resolved
//! the project's kind:30624 to ([`crate::managed_agents::role_packs_view`]),
//! and the `packRef.sha` each execution published on its kind:44223. Until
//! this module existed it could only compare them for equality — same or
//! different — which is the least useful true thing to say. "Different" hides
//! four different situations that call for four different responses: an
//! execution running an *earlier* revision (it keeps it until its next launch
//! or resume), an execution running a *later* one (this machine is the stale
//! party and has not refreshed), a revision from an unrelated history, and a
//! revision this machine has never seen at all.
//!
//! So this module asks `git` — the only authority on ancestry — and answers
//! with the relation and the distance.
//!
//! # What it refuses to do
//!
//! - **It never fetches.** Every command here is local: `rev-parse`,
//!   `cat-file -e`, `merge-base --is-ancestor`, `rev-list --count`. Nothing
//!   clones, fetches, checks out, or writes. A comparison that quietly
//!   refreshed the checkout would change the very fact the renderer is asking
//!   about, and would make a read of the Packs tab a mutation of this host.
//! - **It never guesses.** With no source, no checkout, or a `git` that
//!   refused, `current_sha` is `null`, `reason` says which in the host's own
//!   words, and every relation is
//!   [`PackRevisionKind::UnknownHere`] — never `current`, which is the one
//!   answer a reader would act on.
//! - **It never lets the wire name a revision `git` has not vetted.** Every
//!   sha is 40 lowercase hex before it reaches a `git` argument, and a single
//!   malformed one refuses the whole call rather than being dropped: a
//!   silently skipped row would read as "no reported revision" instead of
//!   "this reported revision is not a revision".

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;

use crate::app_state::AppState;
use crate::commands::project_git_exec::{run_git, GitAuthConfig};
use crate::managed_agents::packs_cache;

/// The reason [`ProjectPackRevisionComparison::current_sha`] is `null` because
/// the project names no packs source at all.
const NO_SOURCE: &str = "this project names no packs repository, so this computer has no \
                         resolution of its own to compare reported revisions against.";

/// The reason [`ProjectPackRevisionComparison::current_sha`] is `null` because
/// this computer has never resolved the project's packs repository.
const NO_CHECKOUT: &str = "this computer has not resolved this project's packs repository yet, \
                           so there is no checkout here to compare reported revisions against.";

/// How one reported revision stands to the commit this machine's packs
/// checkout is on.
///
/// Serialised in kebab-case, the wire spelling the renderer switches on
/// (`docs/ROLE_ADOPTION_EVIDENCE.md` § 3a).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackRevisionKind {
    /// The reported revision *is* the commit this checkout is on.
    Current,
    /// An ancestor of this checkout's commit: the execution is running an
    /// older revision of the same history.
    Earlier,
    /// A descendant of this checkout's commit: the execution is running a
    /// newer revision, and this machine has not refreshed.
    Later,
    /// Both commits are in this checkout's object store and neither is an
    /// ancestor of the other — a different history, not a different age.
    Unrelated,
    /// This checkout's object store does not hold the reported commit, so
    /// nothing can be said about it here.
    UnknownHere,
}

/// One reported revision and its relation to this machine's checkout.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackRevisionRelation {
    /// The reported commit, 40 lowercase hex.
    pub sha: String,
    /// How it stands to this machine's checkout.
    pub relation: PackRevisionKind,
    /// Commits in `sha..HEAD` when [`PackRevisionKind::Earlier`] — how far
    /// behind this machine the reported revision is. `null` otherwise, and
    /// `null` too when `git` could not count them.
    pub behind: Option<u64>,
    /// Commits in `HEAD..sha` when [`PackRevisionKind::Later`] — how far ahead
    /// of this machine the reported revision is. `null` otherwise, and `null`
    /// too when `git` could not count them.
    pub ahead: Option<u64>,
}

/// This machine's answer about a set of reported pack revisions, at one
/// moment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPackRevisionComparison {
    /// The `30617:<owner>:<id>` coordinate of the project's packs repository,
    /// or `null` when the project names none.
    pub repo: Option<String>,
    /// The commit this machine's packs checkout is on, or `null` with a
    /// [`Self::reason`] when there is no answer to give.
    pub current_sha: Option<String>,
    /// Unix milliseconds at which `git` was asked. Reported so the renderer
    /// can say *when* this machine's account was true rather than implying it
    /// is true now.
    pub compared_at: u64,
    /// Why [`Self::current_sha`] is `null`, in the host's or `git`'s own
    /// words. `null` when there is a commit.
    pub reason: Option<String>,
    /// One row per distinct reported revision, in the order the caller first
    /// named them.
    pub relations: Vec<PackRevisionRelation>,
}

/// A reported revision, validated before it can become a `git` argument.
///
/// The same rule [`packs_cache`] applies to a pinned 30624 commit — 40
/// lowercase hex — restated here because that one is private to the staging
/// path and this one guards a different door.
fn validate_reported_sha(value: &str) -> Result<String, String> {
    let sha = value.trim();
    if sha.len() == 40
        && sha
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Ok(sha.to_string());
    }
    Err(format!(
        "a reported pack revision must be 40-character lowercase hex, not {value:?}"
    ))
}

/// Validate every sha and drop repeats, keeping the caller's first order.
///
/// Two executions on the same revision are one question for `git`, but the
/// renderer's own list is allowed to repeat; deduplicating here keeps the
/// answer one row per revision without making the caller sort.
fn distinct_valid_shas(shas: &[String]) -> Result<Vec<String>, String> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut ordered: Vec<String> = Vec::with_capacity(shas.len());
    for sha in shas {
        let sha = validate_reported_sha(sha)?;
        if seen.insert(sha.clone()) {
            ordered.push(sha);
        }
    }
    Ok(ordered)
}

/// The answer when this machine has no commit to compare against: every
/// revision `unknown-here`, and `reason` says why.
fn nothing_to_compare(
    repo: Option<&str>,
    shas: Vec<String>,
    now_ms: u64,
    reason: String,
) -> ProjectPackRevisionComparison {
    ProjectPackRevisionComparison {
        repo: repo.map(str::to_owned),
        current_sha: None,
        compared_at: now_ms,
        reason: Some(reason),
        relations: shas
            .into_iter()
            .map(|sha| PackRevisionRelation {
                sha,
                relation: PackRevisionKind::UnknownHere,
                behind: None,
                ahead: None,
            })
            .collect(),
    }
}

/// `true` when `checkout`'s object store holds `sha` as a commit.
fn holds_commit(checkout: &Path, sha: &str, auth: &GitAuthConfig) -> bool {
    run_git(
        &["cat-file", "-e", &format!("{sha}^{{commit}}")],
        Some(checkout),
        auth,
    )
    .is_ok()
}

/// `true` when `ancestor` is an ancestor of `descendant` in `checkout`.
///
/// `git merge-base --is-ancestor` answers by exit status, so a `false` here
/// covers both "no" and "git could not say"; both callers have already
/// established that the two commits are present, and the fallthrough is
/// [`PackRevisionKind::Unrelated`], which claims nothing about age.
fn is_ancestor(checkout: &Path, ancestor: &str, descendant: &str, auth: &GitAuthConfig) -> bool {
    run_git(
        &["merge-base", "--is-ancestor", ancestor, descendant],
        Some(checkout),
        auth,
    )
    .is_ok()
}

/// Commits in `from..to`, or `None` when `git` did not answer with a number.
fn count_commits(checkout: &Path, from: &str, to: &str, auth: &GitAuthConfig) -> Option<u64> {
    run_git(
        &["rev-list", "--count", &format!("{from}..{to}")],
        Some(checkout),
        auth,
    )
    .ok()
    .and_then(|output| output.trim().parse::<u64>().ok())
}

/// Compare `shas` against the commit `checkout` is on.
///
/// The whole answer, computed from a directory and a clock — no `AppHandle`,
/// no store, no relay — so the two-host proof in this module's tests can build
/// two checkouts of one repository and ask each of them.
///
/// `checkout` is this machine's packs checkout for the project's repository;
/// `None`, or a directory with no `.git`, is not an error — it is an answer
/// with `current_sha: None` and a `reason`. So is a `git` that refused to read
/// `HEAD`: the refusal is carried verbatim rather than turned into a failed
/// call, because the renderer has rows to draw either way and "this machine
/// cannot say" is the honest label for them.
///
/// # Errors
/// A sentence naming the offending value when any sha is not 40 lowercase
/// hex. That is a caller mistake, not a state of this machine, and it refuses
/// the whole call.
pub(crate) fn compare_pack_revisions(
    checkout: Option<&Path>,
    repo: Option<&str>,
    shas: &[String],
    auth: &GitAuthConfig,
    now_ms: u64,
) -> Result<ProjectPackRevisionComparison, String> {
    let shas = distinct_valid_shas(shas)?;

    let Some(checkout) = checkout else {
        return Ok(nothing_to_compare(
            repo,
            shas,
            now_ms,
            NO_SOURCE.to_string(),
        ));
    };
    if !checkout.join(".git").is_dir() {
        return Ok(nothing_to_compare(
            repo,
            shas,
            now_ms,
            NO_CHECKOUT.to_string(),
        ));
    }

    let head = match run_git(
        &["rev-parse", "--verify", "HEAD^{commit}"],
        Some(checkout),
        auth,
    ) {
        Ok(output) => output.trim().to_string(),
        Err(error) => return Ok(nothing_to_compare(repo, shas, now_ms, error)),
    };
    if head.is_empty() {
        return Ok(nothing_to_compare(
            repo,
            shas,
            now_ms,
            NO_CHECKOUT.to_string(),
        ));
    }

    let relations = shas
        .into_iter()
        .map(|sha| {
            if sha == head {
                return PackRevisionRelation {
                    sha,
                    relation: PackRevisionKind::Current,
                    behind: None,
                    ahead: None,
                };
            }
            if !holds_commit(checkout, &sha, auth) {
                return PackRevisionRelation {
                    sha,
                    relation: PackRevisionKind::UnknownHere,
                    behind: None,
                    ahead: None,
                };
            }
            if is_ancestor(checkout, &sha, &head, auth) {
                let behind = count_commits(checkout, &sha, &head, auth);
                return PackRevisionRelation {
                    sha,
                    relation: PackRevisionKind::Earlier,
                    behind,
                    ahead: None,
                };
            }
            if is_ancestor(checkout, &head, &sha, auth) {
                let ahead = count_commits(checkout, &head, &sha, auth);
                return PackRevisionRelation {
                    sha,
                    relation: PackRevisionKind::Later,
                    behind: None,
                    ahead,
                };
            }
            PackRevisionRelation {
                sha,
                relation: PackRevisionKind::Unrelated,
                behind: None,
                ahead: None,
            }
        })
        .collect();

    Ok(ProjectPackRevisionComparison {
        repo: repo.map(str::to_owned),
        current_sha: Some(head),
        compared_at: now_ms,
        reason: None,
        relations,
    })
}

/// Unix milliseconds now, or `0` on a clock before the epoch.
///
/// `0` rather than a refusal: a comparison is still worth having on a machine
/// whose clock is wrong, and a renderer that sees the epoch will render an
/// obviously wrong age rather than a plausible one.
fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default()
}

/// This host's checkout directory for `source`'s packs repository.
///
/// Derived exactly the way [`crate::managed_agents::role_packs_view`] derives
/// it before syncing, so the commit read here is the commit that view landed.
fn checkout_dir_for_source(
    app: &AppHandle,
    source: &packs_cache::ProjectPackSource,
) -> Result<PathBuf, String> {
    let root = packs_cache::packs_root(app)?;
    let (owner, id) = packs_cache::parse_repo_coordinate(&source.repo)?;
    Ok(packs_cache::packs_checkout_dir(&root, &owner, &id))
}

/// Everything the command does off the async runtime: take the managed-agents
/// store lock, find this host's checkout for the project's packs repository,
/// and ask `git`.
///
/// The lock is the same one `list_project_role_packs` takes, and for the same
/// reason: without it this could read `HEAD` in the middle of a sync's
/// `checkout --detach` and report a commit no view ever showed. It **syncs
/// nothing** — the checkout is whatever the last list left, which is exactly
/// the fact the renderer is asking about.
///
/// # Errors
/// A sentence when git is not on PATH, when the project's repository
/// coordinate is malformed, when the packs cache directory could not be
/// created, when the store lock was poisoned, or when a reported sha is not
/// 40 lowercase hex.
pub(crate) fn compare_project_pack_revisions_blocking(
    app: &AppHandle,
    source: Option<packs_cache::ProjectPackSource>,
    shas: Vec<String>,
) -> Result<ProjectPackRevisionComparison, String> {
    use tauri::Manager;
    let state = app.state::<AppState>();
    // Local reads only, so no identity key is needed: a machine that has not
    // signed in can still say which commit its checkout is on.
    let auth = crate::commands::project_git_exec::build_local_git_auth_config()?;
    let _store_guard = state
        .managed_agents_store_lock
        .lock()
        .map_err(|error| error.to_string())?;
    let checkout = match &source {
        Some(source) => Some(checkout_dir_for_source(app, source)?),
        None => None,
    };
    compare_pack_revisions(
        checkout.as_deref(),
        source.as_ref().map(|source| source.repo.as_str()),
        &shas,
        &auth,
        now_unix_ms(),
    )
}

#[cfg(test)]
#[path = "pack_revisions_tests.rs"]
mod tests;
