//! The caller's own clone of a project's agents repository, brought up to a
//! commit `bee agents-repo commit` just landed.
//!
//! `commit` builds and pushes from a throwaway clone
//! ([`super::agents_repo_git::commit_drafts`]). A seat then validates and
//! adopts the plan with `bee sessions work validate|adopt --agents-repo <dir>
//! --commit <sha>`, which read `git show <sha>:<path>` from **its own** clone
//! — the one the host cut beside its worktree. Without this module that clone
//! had never seen the commit the seat had just made, and the seat validated a
//! stale tree.
//!
//! Which clone is the caller's own is decided by provenance, not by a search.
//! The host cuts the clone at `<worktree>-agents`
//! ([`buzz_core::model_registry_source::seat_agents_clone_path`]) and sets a
//! remote to the relay's clone URL for the project's agents repository
//! (`seat_agents_clone::cut_seat_agents_clone`). A clone is bound only when
//! one of its configured remotes **is** the canonical clone URL `bee` just
//! pushed to — same relay, same owner, same repository — compared after the
//! `ws(s)`→`http(s)` spelling the relay supports. A remote at another relay
//! with the same owner and id is a different repository with its own state,
//! and a host packs-cache path is not a relay at all: such a clone is never
//! fetched into and never followed, and is reported as not bound with its
//! remotes' names and credential-free URLs.
//!
//! The remote's name is read from the clone, never assumed.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Where the caller's own clone is, as far as provenance can say.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnClone {
    /// A clone with a configured remote at the canonical clone URL.
    Bound {
        /// The clone.
        path: PathBuf,
        /// The name of the remote that is the canonical clone URL.
        remote: String,
    },
    /// The host's clone location holds a repository none of whose remotes is
    /// the canonical clone URL; it is left alone.
    Unbound {
        /// The clone.
        path: PathBuf,
        /// Each remote as `name url`, the URL stripped of credentials.
        remotes: Vec<String>,
    },
    /// Neither the working directory's repository nor the host's clone
    /// location is a clone of this repository.
    Absent,
}

/// What a landed commit did to the caller's own clone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CloneRefresh {
    /// The clone.
    pub path: String,
    /// The remote the commit was fetched from.
    pub remote: String,
    /// The commit this run landed.
    pub commit: String,
    /// Whether the clone now holds the commit and `<remote>/main` contains
    /// it: `validate --commit` and `adopt` resolve it.
    pub fetched: bool,
    /// `fast-forwarded`, `current`, or `left` (the working tree was not
    /// touched; `reason` says why).
    pub working_tree: &'static str,
    /// Why the fetch failed or the working tree was left.
    pub reason: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = crate::commands::sessions::worktree::git_command(dir)
        .args(args)
        .output()
        .map_err(|error| format!("git {} could not run: {error}", args.join(" ")))?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    } else {
        Err(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

/// A clone URL in the one spelling two names of the same relay repository
/// share: `ws(s)` read as `http(s)`, scheme and host lowercased, the scheme's
/// default port dropped, no trailing `/` or `.git`. Credentials are not part
/// of the identity. `None` for anything that is not a `scheme://host/path`
/// URL — a local path is never a relay.
pub(crate) fn canonical_clone_url(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches('/');
    let url = url.strip_suffix(".git").unwrap_or(url);
    let (scheme, rest) = url.split_once("://")?;
    let scheme = match scheme.to_ascii_lowercase().as_str() {
        "ws" => "http".to_owned(),
        "wss" => "https".to_owned(),
        other => other.to_owned(),
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.to_ascii_lowercase();
    let host = match (scheme.as_str(), host.rsplit_once(':')) {
        ("https", Some((name, "443"))) | ("http", Some((name, "80"))) => name.to_owned(),
        _ => host,
    };
    if scheme.is_empty() || (host.is_empty() && scheme != "file") {
        return None;
    }
    Some(format!("{scheme}://{host}/{path}"))
}

/// A URL safe to print: any `user:password@` removed, and the query dropped.
pub(crate) fn redact_url(url: &str) -> String {
    let url = url.split(['?', '#']).next().unwrap_or_default();
    match url.split_once("://") {
        Some((scheme, rest)) => {
            let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
            let host = authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host);
            if path.is_empty() {
                format!("{scheme}://{host}")
            } else {
                format!("{scheme}://{host}/{path}")
            }
        }
        None => url.to_owned(),
    }
}

/// `text` with the `user:password@` part of every URL in it removed, for
/// messages (a git error names the URL it failed on) that are printed.
pub(crate) fn redact_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (head, tail) = rest.split_at(at + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c.is_whitespace() || c == '\'' || c == '"')
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        out.push_str(
            authority
                .rsplit_once('@')
                .map_or(authority, |(_, host)| host),
        );
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// The clone's configured remotes as `(name, url)`, from its own config.
fn remotes(dir: &Path) -> Vec<(String, String)> {
    let Ok(listing) = git(dir, &["config", "--get-regexp", r"^remote\..*\.url$"]) else {
        return Vec::new();
    };
    listing
        .lines()
        .filter_map(|line| {
            let (key, url) = line.split_once(' ')?;
            let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
            Some((name.to_owned(), url.to_owned()))
        })
        .collect()
}

/// The remote of `dir` that is `canonical`, preferring `main`'s upstream when
/// more than one is.
fn matching_remote(dir: &Path, canonical: &str) -> Option<String> {
    let mut matches: Vec<String> = remotes(dir)
        .into_iter()
        .filter(|(_, url)| canonical_clone_url(url).as_deref() == Some(canonical))
        .map(|(name, _)| name)
        .collect();
    let upstream = git(dir, &["config", "--get", "branch.main.remote"]).ok();
    matches.sort();
    match upstream {
        Some(upstream) if matches.contains(&upstream) => Some(upstream),
        _ => matches.into_iter().next(),
    }
}

/// The caller's own clone of the repository at `clone_url`: the repository
/// the caller stands in when one of its remotes is that URL, else the clone
/// the host cuts beside it. Nothing else is looked at.
pub fn find_own_clone(start: &Path, clone_url: &str) -> OwnClone {
    let Some(canonical) = canonical_clone_url(clone_url) else {
        return OwnClone::Absent;
    };
    let top = git(start, &["rev-parse", "--show-toplevel"])
        .map_or_else(|_| start.to_path_buf(), PathBuf::from);
    if top.join(".git").exists() {
        if let Some(remote) = matching_remote(&top, &canonical) {
            return OwnClone::Bound { path: top, remote };
        }
    }
    let Some(beside) = buzz_core::model_registry_source::seat_agents_clone_path(&top) else {
        return OwnClone::Absent;
    };
    if !beside.join(".git").exists() {
        return OwnClone::Absent;
    }
    match matching_remote(&beside, &canonical) {
        Some(remote) => OwnClone::Bound {
            path: beside,
            remote,
        },
        None => OwnClone::Unbound {
            remotes: remotes(&beside)
                .into_iter()
                .map(|(name, url)| format!("{name} {}", redact_url(&url)))
                .collect(),
            path: beside,
        },
    }
}

/// The first path the fast-forward to `commit` would write over that git
/// does not track at `HEAD`: an untracked or ignored file or directory at a
/// path the commit changes, or a non-directory where the commit needs a
/// directory. `git merge` refuses the untracked case itself but overwrites an
/// ignored file silently, so both are checked before anything moves.
fn untracked_in_the_way(clone: &Path, commit: &str) -> Result<Option<String>, String> {
    let changed = git(
        clone,
        &["diff", "--name-only", "--no-renames", "HEAD", commit],
    )?;
    for path in changed.lines().filter(|line| !line.is_empty()) {
        let segments: Vec<&str> = path.split('/').collect();
        for depth in 1..=segments.len() {
            let prefix = segments[..depth].join("/");
            let Ok(meta) = std::fs::symlink_metadata(clone.join(&prefix)) else {
                break;
            };
            let is_leaf = depth == segments.len();
            if !is_leaf && meta.is_dir() {
                continue;
            }
            let tracked = git(clone, &["ls-files", "--error-unmatch", "--", &prefix]).is_ok();
            if !tracked {
                return Ok(Some(prefix));
            }
            break;
        }
    }
    Ok(None)
}

/// Bring `clone` up to `commit`, which this run landed on the relay's `main`,
/// through its own `remote` (the one [`find_own_clone`] matched).
///
/// Fetches `main` into `<remote>/main`, then verifies the clone holds
/// `commit` and `<remote>/main` contains it. The working tree moves only by a
/// fast-forward of a checked-out `main` that tracks `remote` (or nothing), with
/// no uncommitted change and no untracked or ignored file where the commit
/// writes; anything else is left exactly as it was and the answer names it.
pub fn refresh_own_clone(clone: &Path, remote: &str, commit: &str) -> CloneRefresh {
    let answer = |fetched, working_tree, reason: Option<String>| CloneRefresh {
        path: clone.display().to_string(),
        remote: remote.to_owned(),
        commit: commit.to_owned(),
        fetched,
        working_tree,
        reason,
    };
    let tracking = format!("refs/remotes/{remote}/main");
    if let Err(error) = git(
        clone,
        &[
            "fetch",
            "--quiet",
            remote,
            &format!("+refs/heads/main:{tracking}"),
        ],
    ) {
        return answer(
            false,
            "left",
            Some(format!(
                "the commit is on main, but this clone could not fetch it: {}",
                redact_credentials(&error)
            )),
        );
    }
    let holds = git(clone, &["cat-file", "-e", &format!("{commit}^{{commit}}")]).is_ok();
    if !(holds && git(clone, &["merge-base", "--is-ancestor", commit, &tracking]).is_ok()) {
        return answer(
            false,
            "left",
            Some(format!(
                "after the fetch, {remote}/main does not contain {commit}"
            )),
        );
    }
    let head = git(clone, &["rev-parse", "HEAD"]).ok();
    let upstream = git(clone, &["config", "--get", "branch.main.remote"]).ok();
    let reason = match git(clone, &["symbolic-ref", "--quiet", "--short", "HEAD"]) {
        Ok(branch) if branch != "main" => format!("HEAD is on {branch}, not main"),
        Err(_) => format!(
            "HEAD is detached at {}",
            head.as_deref().unwrap_or("an unreadable commit")
        ),
        Ok(_) if upstream.as_deref().is_some_and(|name| name != remote) => format!(
            "main tracks {}, not {remote}",
            upstream.as_deref().unwrap_or_default()
        ),
        Ok(_) if head.as_deref() == Some(commit) => return answer(true, "current", None),
        Ok(_) => match git(clone, &["status", "--porcelain", "--untracked-files=no"]) {
            Err(error) => format!("the working tree's state could not be read: {error}"),
            Ok(status) if !status.is_empty() => "the working tree has uncommitted changes".into(),
            Ok(_) => match untracked_in_the_way(clone, commit) {
                Err(error) => format!("the incoming paths could not be checked: {error}"),
                Ok(Some(path)) => {
                    format!("{path} is untracked or ignored here and the commit writes it")
                }
                Ok(None) => match git(clone, &["merge", "--ff-only", "--quiet", commit]) {
                    Err(error) => format!("main cannot fast-forward to the commit: {error}"),
                    Ok(_) if git(clone, &["rev-parse", "HEAD"]).ok().as_deref() == Some(commit) => {
                        return answer(true, "fast-forwarded", None)
                    }
                    Ok(_) => "main did not land on the commit after the fast-forward".into(),
                },
            },
        },
    };
    answer(
        true,
        "left",
        Some(format!("{reason}; the working tree was not touched")),
    )
}

/// The `agents_clone` field `bee agents-repo commit` prints for a landed
/// commit: what happened to the caller's own clone, or why nothing did.
pub fn clone_refresh_report(
    start: Option<&Path>,
    clone_url: &str,
    commit: &str,
) -> serde_json::Value {
    let found = start.map_or(OwnClone::Absent, |start| find_own_clone(start, clone_url));
    let repository = redact_url(clone_url);
    match found {
        OwnClone::Bound { path, remote } => {
            serde_json::json!(refresh_own_clone(&path, &remote, commit))
        }
        OwnClone::Unbound { path, remotes } => serde_json::json!({
            "path": path.display().to_string(),
            "commit": commit,
            "fetched": false,
            "working_tree": "left",
            "remotes": remotes,
            "reason": format!(
                "none of this clone's remotes is this project's agents repository at \
                 {repository}; it was not fetched into"
            ),
        }),
        OwnClone::Absent => serde_json::json!({
            "path": null,
            "commit": commit,
            "fetched": false,
            "reason": format!(
                "no clone of this project's agents repository at {repository} in this working \
                 directory or beside it"
            ),
        }),
    }
}

#[cfg(test)]
#[path = "agents_repo_clone_tests.rs"]
mod tests;
