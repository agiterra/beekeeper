//! Prepare a checkout for continuing somebody else's session (§5).
//!
//! Reconstruction is not a metaphor: to continue an absent participant's work
//! this host has to end up with **their** commit and **their** uncommitted
//! bytes on disk, or say plainly which of the two it could not get. That is
//! all this command does — fetch the wip ref the checkpoint named, prove the
//! sha is the one the checkpoint claimed, put it on a branch, and apply the
//! patch artifact if there is one.
//!
//! Three rules it does not bend.
//!
//! * **The sha is verified, never trusted.** A wip ref can move between the
//!   checkpoint and the fetch. If what arrived is not the sha the checkpoint
//!   named, nothing is checked out and the caller is told — reconstructing
//!   from a different commit and labelling it with the checkpoint's would be
//!   a lie in the one place the product promises evidence.
//! * **Never a half-applied tree.** The patch is dry-run with `git apply
//!   --check` first, and if the real apply still fails the worktree is reset
//!   hard back to the checked-out sha. A tree with three of five hunks in it
//!   looks exactly like a tree somebody edited, and no later reader could
//!   tell them apart.
//! * **Somebody else's uncommitted work is never in the way.** A dirty
//!   worktree is refused up front rather than stashed, moved, or checked out
//!   over.
//!
//! Every git invocation goes through [`run_git_status`] with an argument
//! vector — nothing here builds a shell string.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::commands::project_git_exec::{build_git_auth_config, run_git_status, GitAuthConfig};

/// What the desktop knows when a person presses "Continue this session's work".
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HandoverPrepareCheckoutRequest {
    /// The checkout to prepare. Must be an existing, clean git worktree.
    pub cwd: String,
    /// An explicit remote name, when the caller already knows it.
    ///
    /// Absent is the ordinary case, and it is **not** a synonym for `origin`:
    /// the remote is resolved from the checkout's own list against the
    /// repository the checkpoint names (AGENTS.md § Remotes — the names moved
    /// once and two guards that hard-coded one broke silently).
    pub repo_remote: Option<String>,
    /// The `30617:<owner-hex>:<id>` coordinate the checkpoint's wip ref lives
    /// in, used to recognize the right remote in this checkout.
    pub repo_ref: Option<String>,
    /// This community's relay origin, so the expected clone URL can be built.
    pub relay_origin: Option<String>,
    /// The checkpoint's wip ref, e.g. `refs/heads/wip/builder/9a1c2b3d`.
    pub wip_ref: String,
    /// The sha the checkpoint said that ref stood at.
    pub sha: String,
    /// The umbrella, used for the branch name `handover/<session8>`.
    pub session_ref: String,
    /// The patch artifact's text, when the checkpoint carried one.
    pub patch_text: Option<String>,
    /// The base the patch was cut against, so `--3way` has a base to use.
    pub base_sha: Option<String>,
}

/// What this host actually managed to put on disk, and what it did not.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoverPrepareCheckoutReport {
    /// The branch the work was checked out onto.
    pub branch: String,
    /// The sha now checked out — always the requested one, or this failed.
    pub checked_out_sha: String,
    /// Each artifact this host recovered, in the words a continuation carries.
    pub recovered: Vec<String>,
    /// Each artifact it could not, with the reason. Never silently empty.
    pub missing: Vec<String>,
}

/// Refuse a value that git would read as an option rather than a remote.
fn safe_remote(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('-')
        || trimmed.contains("..")
        || trimmed.contains(char::is_whitespace)
        || trimmed.contains('\0')
    {
        return Err(format!("the remote is not a usable git remote: {value:?}"));
    }
    Ok(trimmed.to_string())
}

/// Characters that would turn a ref argument into something else entirely.
///
/// `:` is the one that matters most: `git fetch <remote> <src>:<dst>` **writes
/// `<dst>` in this repository**, so a checkpoint naming
/// `+refs/heads/wip/x:refs/heads/main` would rewrite the person's own `main`
/// before any sha check could refuse it — the fetch has already happened by
/// then. `+` forces that write past a non-fast-forward. The rest
/// (`~ ^ ? * [ \` and control characters) are revision syntax or glob syntax
/// that `git check-ref-format` refuses anyway.
const REFUSED_REF_CHARACTERS: [char; 8] = [':', '+', '~', '^', '?', '*', '[', '\\'];

/// A wip ref, and nothing else: `refs/heads/<name>`, conservatively spelled.
///
/// Deliberately narrower than git's own rules. A checkpoint is written by
/// somebody else's machine, so this value is untrusted input that reaches a
/// subprocess; the only shape this command has any business fetching is the
/// branch ref the seat hook pushes.
fn safe_wip_ref(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let name = trimmed.strip_prefix("refs/heads/").unwrap_or("");
    if name.is_empty()
        || trimmed.contains("..")
        || trimmed.ends_with('/')
        || trimmed.contains("//")
        || trimmed.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || REFUSED_REF_CHARACTERS.contains(&character)
        })
        || !name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._/-".contains(character))
    {
        return Err(format!(
            "the wip ref is not a plain refs/heads/… branch ref: {value:?}"
        ));
    }
    Ok(trimmed.to_string())
}

fn safe_sha(value: &str, what: &str) -> Result<String, String> {
    let trimmed = value.trim().to_ascii_lowercase();
    if !(trimmed.len() == 40 || trimmed.len() == 64)
        || !trimmed
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(format!("{what} is not a commit sha: {value:?}"));
    }
    Ok(trimmed)
}

/// The canonical relay-hosted clone URL for a `30617:<owner-hex>:<id>` ref.
///
/// The one shape a Buzz relay serves its own repositories at —
/// `<relay-origin>/git/<owner>/<id>` — and the same one
/// `deriveRelayCloneUrl` builds on the TypeScript side.
fn expected_clone_url(repo_ref: &str, relay_origin: &str) -> Option<String> {
    let mut parts = repo_ref.splitn(3, ':');
    if parts.next()? != "30617" {
        return None;
    }
    let owner = parts.next()?.to_ascii_lowercase();
    let id = parts.next()?;
    if owner.len() != 64 || !owner.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let origin = relay_origin.trim_end_matches('/');
    Some(format!("{origin}/git/{owner}/{id}"))
}

/// Compare two clone URLs the way a person would: host and path, not spelling.
fn same_repository(left: &str, right: &str) -> bool {
    fn normalize(url: &str) -> String {
        url.trim()
            .trim_end_matches('/')
            .trim_end_matches(".git")
            .to_ascii_lowercase()
            .replace("https://", "")
            .replace("http://", "")
    }
    normalize(left) == normalize(right)
}

/// The remote in **this checkout** that points at the repository the
/// checkpoint names.
///
/// Never `origin` by assumption. This repository's own remotes were renamed
/// once and every tool that hard-coded a name broke silently that day, so the
/// name is read from the checkout and matched against the URL the coordinate
/// resolves to. Zero matches or several is a refusal that names what was
/// looked for and what was found — a guess here would fetch somebody else's
/// branch and check it out as if it were the checkpoint's.
fn resolve_remote(
    cwd: &Path,
    auth: &GitAuthConfig,
    repo_ref: Option<&str>,
    relay_origin: Option<&str>,
) -> Result<String, String> {
    let (Some(repo_ref), Some(relay_origin)) = (repo_ref, relay_origin) else {
        return Err(
            "no repository coordinate or relay origin was supplied, so this app cannot tell which              remote holds the checkpoint's branch; it will not guess a remote name"
                .to_string(),
        );
    };
    let expected = expected_clone_url(repo_ref, relay_origin)
        .ok_or_else(|| format!("{repo_ref:?} is not a 30617:<owner>:<id> repository coordinate"))?;
    let (listed, output, error) = git(&["remote", "-v"], cwd, auth)?;
    if !listed {
        return Err(format!("could not list this checkout's remotes: {error}"));
    }
    let mut matches: Vec<String> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for line in output.lines() {
        let mut fields = line.split_whitespace();
        let (Some(name), Some(url), Some(kind)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if kind != "(fetch)" {
            continue;
        }
        seen.push(format!("{name} -> {url}"));
        if same_repository(url, &expected) && !matches.iter().any(|found| found == name) {
            matches.push(name.to_string());
        }
    }
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!(
            "no remote in this checkout fetches from {expected}; it has {}. Nothing was fetched —              this app does not guess a remote name",
            if seen.is_empty() {
                "no remotes at all".to_string()
            } else {
                seen.join(", ")
            }
        )),
        _ => Err(format!(
            "{} remotes in this checkout fetch from {expected} ({}); which one holds the              checkpoint's branch is not this app's guess to make",
            matches.len(),
            matches.join(", ")
        )),
    }
}

/// `handover/<session8>` — the branch §4 step 5 names.
fn handover_branch(session_ref: &str) -> Result<String, String> {
    let slug: String = session_ref
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .take(8)
        .collect();
    if slug.len() < 8 {
        return Err(format!(
            "session reference {session_ref:?} has no eight-character slug to name a branch with"
        ));
    }
    Ok(format!("handover/{slug}"))
}

fn git(args: &[&str], cwd: &Path, auth: &GitAuthConfig) -> Result<(bool, String, String), String> {
    let outcome = run_git_status(args, Some(cwd), auth)?;
    Ok((
        outcome.status.success(),
        outcome.stdout.trim().to_string(),
        outcome.stderr.trim().to_string(),
    ))
}

/// The paths `git apply` complained about, so a reader is told which files.
fn failed_paths(stderr: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in stderr.lines() {
        let line = line.trim();
        for marker in [
            "error: patch failed: ",
            "error: cannot apply binary patch to '",
        ] {
            if let Some(rest) = line.strip_prefix(marker) {
                let path = rest
                    .split(':')
                    .next()
                    .unwrap_or(rest)
                    .trim_end_matches('\'')
                    .trim();
                if !path.is_empty() && !paths.iter().any(|seen| seen == path) {
                    paths.push(path.to_string());
                }
            }
        }
    }
    paths
}

/// Fetch, verify, check out, and apply — the whole of reconstruction on disk.
pub(crate) fn prepare_checkout(
    request: &HandoverPrepareCheckoutRequest,
    auth: &GitAuthConfig,
) -> Result<HandoverPrepareCheckoutReport, String> {
    let cwd = PathBuf::from(request.cwd.trim());
    if !cwd.is_dir() {
        return Err(format!("{} is not a directory", cwd.display()));
    }
    let wip_ref = safe_wip_ref(&request.wip_ref)?;
    let sha = safe_sha(&request.sha, "the checkpoint sha")?;
    let branch = handover_branch(&request.session_ref)?;

    let (is_repo, _, stderr) = git(&["rev-parse", "--git-dir"], &cwd, auth)?;
    if !is_repo {
        return Err(format!("{} is not a git checkout: {stderr}", cwd.display()));
    }
    // Resolved from the checkout, or named explicitly by the caller. Never
    // defaulted.
    let remote = match request.repo_remote.as_deref() {
        Some(explicit) => safe_remote(explicit)?,
        None => resolve_remote(
            &cwd,
            auth,
            request.repo_ref.as_deref(),
            request.relay_origin.as_deref(),
        )?,
    };
    // A person's own uncommitted work is never checked out over, stashed, or
    // moved aside to make room for somebody else's session.
    let (_, status, _) = git(&["status", "--porcelain"], &cwd, auth)?;
    if !status.is_empty() {
        return Err(format!(
            "{} has uncommitted changes; commit or move them before continuing somebody else's session",
            cwd.display()
        ));
    }

    // An explicit, destination-free refspec: whatever the ref spells, this
    // fetch writes nothing in this repository but `FETCH_HEAD`.
    let refspec = format!("{wip_ref}:");
    let (fetched, _, fetch_error) = git(&["fetch", &remote, &refspec], &cwd, auth)?;
    if !fetched {
        return Err(format!(
            "could not fetch {wip_ref} from {remote}: {fetch_error}"
        ));
    }
    let (resolved, fetch_head, resolve_error) = git(&["rev-parse", "FETCH_HEAD"], &cwd, auth)?;
    if !resolved {
        return Err(format!("could not read the fetched ref: {resolve_error}"));
    }
    if fetch_head.to_ascii_lowercase() != sha {
        return Err(format!(
            "{wip_ref} is at {fetch_head} on {remote}, not the {sha} the checkpoint named; nothing was checked out"
        ));
    }

    // `checkout -B` moves an existing branch, so a second reconstruction into
    // the same directory would silently discard the first one's commits. The
    // branch is only reused when its tip is already contained in the sha being
    // checked out; otherwise this refuses and names the branch and its tip, so
    // a person can look at what is there before anything moves.
    let (branch_exists, existing_tip, _) = git(
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ],
        &cwd,
        auth,
    )?;
    if branch_exists && !existing_tip.is_empty() && existing_tip != sha {
        let (contained, _, _) = git(
            &["merge-base", "--is-ancestor", &existing_tip, &sha],
            &cwd,
            auth,
        )?;
        if !contained {
            return Err(format!(
                "{branch} already exists at {existing_tip}, which is not contained in {sha}; \
                 an earlier reconstruction's commits are on it, so nothing was checked out"
            ));
        }
    }
    let (checked_out, _, checkout_error) = git(&["checkout", "-B", &branch, &sha], &cwd, auth)?;
    if !checked_out {
        return Err(format!(
            "could not check out {sha} on {branch}: {checkout_error}"
        ));
    }
    let mut recovered = vec![format!("wip-ref {wip_ref} at {sha}")];
    let mut missing: Vec<String> = Vec::new();

    if let Some(patch_text) = request
        .patch_text
        .as_deref()
        .map(str::trim_end)
        .filter(|text| !text.is_empty())
    {
        match apply_patch(patch_text, request.base_sha.as_deref(), &cwd, auth) {
            Ok(bytes) => recovered.push(format!("patch applied ({bytes} bytes)")),
            Err(reason) => {
                // The tree is back at the checked-out sha; say what did not
                // land rather than leaving a person to discover it in a diff.
                missing.push(reason);
            }
        }
    }

    Ok(HandoverPrepareCheckoutReport {
        branch,
        checked_out_sha: sha,
        recovered,
        missing,
    })
}

/// Apply the patch, or leave the tree exactly as the checkout left it.
fn apply_patch(
    patch_text: &str,
    base_sha: Option<&str>,
    cwd: &Path,
    auth: &GitAuthConfig,
) -> Result<usize, String> {
    let bytes = patch_text.len();
    if let Some(base) = base_sha {
        let base = safe_sha(base, "the patch base")?;
        let (has_base, _, _) = git(
            &["cat-file", "-e", &format!("{base}^{{commit}}")],
            cwd,
            auth,
        )?;
        if !has_base {
            return Err(format!(
                "the patch was cut against {base}, which this checkout does not have; it was not applied"
            ));
        }
    }
    let patch_path = write_patch(patch_text)?;
    let patch_arg = patch_path.to_string_lossy().to_string();
    let result = (|| -> Result<usize, String> {
        // Dry run first: a patch that cannot apply must never start applying.
        let (checks, _, check_error) =
            git(&["apply", "--check", "--binary", &patch_arg], cwd, auth)?;
        if !checks {
            return Err(patch_refusal(&check_error, bytes));
        }
        let (applied, _, apply_error) = git(
            &["apply", "--binary", "--3way", "--index", &patch_arg],
            cwd,
            auth,
        )?;
        if !applied {
            // `--3way` can leave conflict markers and index entries behind, so
            // the tree is put back before anybody sees it.
            let _ = git(&["reset", "--hard", "HEAD"], cwd, auth);
            let _ = git(&["clean", "-fd"], cwd, auth);
            return Err(patch_refusal(&apply_error, bytes));
        }
        Ok(bytes)
    })();
    let _ = std::fs::remove_file(&patch_path);
    result
}

fn patch_refusal(stderr: &str, bytes: usize) -> String {
    let paths = failed_paths(stderr);
    if paths.is_empty() {
        format!("the {bytes}-byte patch did not apply to this checkout; it was not applied")
    } else {
        format!(
            "the {bytes}-byte patch did not apply to {}; nothing from it was applied",
            paths.join(", ")
        )
    }
}

fn write_patch(patch_text: &str) -> Result<PathBuf, String> {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "beekeeper-handover-{}-{}.patch",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default()
    ));
    let mut text = patch_text.to_string();
    if !text.ends_with('\n') {
        text.push('\n');
    }
    std::fs::write(&path, text).map_err(|error| format!("could not stage the patch: {error}"))?;
    Ok(path)
}

/// Put an absent participant's committed work — and their patch — on disk.
#[tauri::command]
pub async fn handover_prepare_checkout(
    request: HandoverPrepareCheckoutRequest,
    state: tauri::State<'_, AppState>,
) -> Result<HandoverPrepareCheckoutReport, String> {
    let auth = build_git_auth_config(&state)?;
    tauri::async_runtime::spawn_blocking(move || prepare_checkout(&request, &auth))
        .await
        .map_err(|error| format!("handover checkout task failed: {error}"))?
}

#[cfg(test)]
#[path = "handover_tests.rs"]
mod tests;
