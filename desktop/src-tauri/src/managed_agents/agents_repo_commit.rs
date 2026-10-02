//! The Commit button (spec § 4.12): land a set of open draft heads on the
//! agents repository's `main`, or refuse naming every reason.
//!
//! The same sequence `bee agents-repo commit` runs, in this host's git
//! terms: a throwaway object database fed from the packs cache, the tip
//! this run read, a base check per head, the tree built with plumbing,
//! materialized and validated by `buzz_persona::agents_repo::validate_root`
//! before anything leaves this computer, one commit as the viewer with a
//! `Co-authored-by:` per draft author, a push under `--force-with-lease`
//! on the tip, and an `ls-remote` verify whose failure is reported
//! **unknown**, never "failed". The renderer publishes the `commit.record`;
//! this host never signs a relay event on the viewer's behalf here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use buzz_persona_pkg::agents_repo::{validate_root, ActionsCheck};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use super::agents_repo_read::{blob_at_tip, AgentsRepoCheckout};
use super::packs_cache;
use crate::commands::project_git_exec::{run_git, run_git_bytes, run_git_status};

/// One draft head to apply, as the renderer's fold reports it.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoDraftChange {
    /// The draft op's event id.
    pub id: String,
    /// Its author (64 hex).
    pub author: String,
    /// `file.put`, `file.move`, `file.delete` or `asset.put`.
    pub op: String,
    /// The path the op names.
    pub path: String,
    /// A move's destination.
    #[serde(default)]
    pub to: Option<String>,
    /// A put's text.
    #[serde(default)]
    pub text: Option<String>,
    /// An `asset.put`'s media blob id; the bytes are fetched before the
    /// commit runs and handed in beside the request.
    ///
    /// The row the renderer sends also carries `mime` and `size`; neither is
    /// taken here, because the sha256 check on the fetched bytes already
    /// settles what they would. Unknown fields are ignored, so the renderer
    /// sends the row as the fold reports it.
    #[serde(default)]
    pub sha256: Option<String>,
    /// The blob the author started from, or null for a new file.
    #[serde(default)]
    pub base: Option<String>,
    /// The author's one-line reason.
    #[serde(default)]
    pub message: Option<String>,
}

/// The bytes of every `asset.put` blob this commit lands, by sha256.
///
/// Fetched from the relay's media store and sha-verified by the caller
/// *before* the blocking commit runs, so the tree write stays synchronous and
/// `commit_in` stays testable without a network.
pub(crate) type AssetBytes = HashMap<String, Vec<u8>>;

/// A draft author's display name, for the `Co-authored-by:` trailer.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoAuthor {
    /// 64 hex.
    pub pubkey: String,
    /// As the renderer knows them; null falls back to `Beekeeper <pubkey8>`.
    #[serde(default)]
    pub name: Option<String>,
}

/// What the renderer asks for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoCommitRequest {
    /// The project coordinate.
    pub project_ref: String,
    /// The tip the dialog showed; a moved `main` refuses before building.
    #[serde(default)]
    pub expected_tip: Option<String>,
    /// The commit subject.
    pub message: String,
    /// The heads to apply.
    pub drafts: Vec<AgentsRepoDraftChange>,
    /// Names for the trailers.
    #[serde(default)]
    pub authors: Vec<AgentsRepoAuthor>,
}

/// One path that refused, with a stable code.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoCommitRefusal {
    /// The path, or null for a whole-run refusal.
    pub path: Option<String>,
    /// `main-moved`, `no-main`, `stale-base`, `invalid-tree`,
    /// `lease-rejected`, `push-refused`.
    pub code: String,
    /// The sentence.
    pub message: String,
}

/// One path the commit touched.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoCommittedPath {
    /// The path.
    pub path: String,
    /// `A`, `M`, `D` or `R`.
    pub status: String,
}

/// How the run ended. Every field is a wire fact; the renderer prints them
/// and composes no happier sentence.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentsRepoCommitResult {
    /// `yes`, `no` or `unknown`.
    pub pushed: String,
    /// `main` before, when it was read.
    pub tip_before: Option<String>,
    /// The new commit, when one was built.
    pub commit: Option<String>,
    /// Its tree.
    pub tree: Option<String>,
    /// What changed, when the commit was built.
    pub paths: Vec<AgentsRepoCommittedPath>,
    /// How `actions.yml` was checked, when the tree validated.
    pub actions: Option<String>,
    /// Every refusal, when `pushed` is `no`.
    pub refusals: Vec<AgentsRepoCommitRefusal>,
    /// Why the verify failed, when `pushed` is `unknown`.
    pub unknown_reason: Option<String>,
    /// The committer identity used.
    pub committer_name: String,
    /// Its e-mail.
    pub committer_email: String,
    /// The draft ids the caller asked to land, echoed for the record.
    pub draft_ids: Vec<String>,
}

fn refused(
    tip_before: Option<String>,
    refusals: Vec<AgentsRepoCommitRefusal>,
    committer: &(String, String),
    draft_ids: Vec<String>,
) -> AgentsRepoCommitResult {
    AgentsRepoCommitResult {
        pushed: "no".into(),
        tip_before,
        commit: None,
        tree: None,
        paths: Vec::new(),
        actions: None,
        refusals,
        unknown_reason: None,
        committer_name: committer.0.clone(),
        committer_email: committer.1.clone(),
        draft_ids,
    }
}

fn short(sha: &str) -> &str {
    &sha[..8.min(sha.len())]
}

/// Run the commit. `repo` was resolved with `refresh: true` by the caller,
/// so its `tip` is what the relay had moments ago.
pub(crate) fn commit_drafts(
    app: &AppHandle,
    repo: &AgentsRepoCheckout,
    request: &AgentsRepoCommitRequest,
    committer: (String, String),
    assets: &AssetBytes,
) -> Result<AgentsRepoCommitResult, String> {
    let draft_ids: Vec<String> = request.drafts.iter().map(|d| d.id.clone()).collect();
    let tip = repo.tip.clone();
    // A throwaway object database under the packs root, fed from the cache.
    let root = packs_cache::packs_root(app)?;
    let scratch_parent = root.join("commit");
    std::fs::create_dir_all(&scratch_parent)
        .map_err(|error| format!("could not create {}: {error}", scratch_parent.display()))?;
    let scratch = tempfile::Builder::new()
        .prefix(".drafts-")
        .tempdir_in(&scratch_parent)
        .map_err(|error| format!("could not create a scratch directory: {error}"))?;
    let catalog = packs_cache::template_catalog(app);
    let outcome = commit_in(
        scratch.path(),
        repo,
        request,
        &committer,
        &tip,
        draft_ids,
        &catalog,
        assets,
    );
    drop(scratch);
    outcome
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn commit_in(
    scratch: &Path,
    repo: &AgentsRepoCheckout,
    request: &AgentsRepoCommitRequest,
    committer: &(String, String),
    tip: &str,
    draft_ids: Vec<String>,
    catalog: &buzz_persona_pkg::template::TemplateCatalog,
    assets: &AssetBytes,
) -> Result<AgentsRepoCommitResult, String> {
    if let Some(expected) = request.expected_tip.as_deref().filter(|e| !e.is_empty()) {
        if expected != tip {
            return Ok(refused(
                Some(tip.to_owned()),
                vec![AgentsRepoCommitRefusal {
                    path: None,
                    code: "main-moved".into(),
                    message: format!(
                        "main moved from {} to {} while you were reviewing; reload and commit again",
                        short(expected),
                        short(tip)
                    ),
                }],
                committer,
                draft_ids,
            ));
        }
    }

    // Base checks against the tip.
    let mut refusals = Vec::new();
    for change in &request.drafts {
        let at_tip = blob_at_tip(repo, &change.path);
        if at_tip != change.base {
            refusals.push(AgentsRepoCommitRefusal {
                path: Some(change.path.clone()),
                code: "stale-base".into(),
                message: match (&change.base, &at_tip) {
                    (None, Some(_)) => format!(
                        "{} exists on main now, but this draft was written as a new file; reload it and re-apply the draft — nothing was pushed",
                        change.path
                    ),
                    (Some(_), None) => format!(
                        "{} is no longer on main; this draft by {} was based on a file that was since removed — nothing was pushed",
                        change.path,
                        short(&change.author)
                    ),
                    _ => format!(
                        "main changed {} after this draft by {} was based on it; reload the file and re-apply the draft — nothing was pushed",
                        change.path,
                        short(&change.author)
                    ),
                },
            });
        }
        if change.op == "file.move" {
            if let Some(to) = &change.to {
                if blob_at_tip(repo, to).is_some() {
                    refusals.push(AgentsRepoCommitRefusal {
                        path: Some(to.clone()),
                        code: "stale-base".into(),
                        message: format!(
                            "{to} already exists on main; the move of {} cannot land over it — nothing was pushed",
                            change.path
                        ),
                    });
                }
            }
        }
    }
    if !refusals.is_empty() {
        return Ok(refused(
            Some(tip.to_owned()),
            refusals,
            committer,
            draft_ids,
        ));
    }

    let work: PathBuf = scratch.join("work");
    std::fs::create_dir_all(&work)
        .map_err(|error| format!("could not create {}: {error}", work.display()))?;
    // Two auths: the local one (file transport allowed, no credential) for
    // everything that touches the cache and the object database; the relay
    // one (file transport refused, credential helper wired) for the push
    // and the verify. Both carry the committer's identity.
    let mut auth = crate::commands::project_git_exec::build_local_clone_git_auth_config()?;
    auth.set_commit_identity(committer.0.clone(), committer.1.clone());
    let mut remote_auth = repo.auth.clone();
    remote_auth.set_commit_identity(committer.0.clone(), committer.1.clone());
    let cwd = Some(work.as_path());
    run_git(&["init", "--quiet"], cwd, &auth)?;
    // The tip's objects are local: fetch them from the cache, not the relay.
    let cache = repo.checkout.display().to_string();
    let remote_ref = format!("refs/remotes/origin/{}", repo.branch);
    run_git(
        &[
            "fetch",
            "--quiet",
            "--",
            &cache,
            &format!("{remote_ref}:refs/heads/tip"),
        ],
        cwd,
        &auth,
    )?;
    let fetched = run_git(&["rev-parse", "--verify", "refs/heads/tip"], cwd, &auth)?
        .trim()
        .to_owned();
    if fetched != tip {
        return Err(format!(
            "the packs cache's {remote_ref} is {} but the tip read was {}; refresh and retry",
            short(&fetched),
            short(tip)
        ));
    }

    run_git(&["read-tree", tip], cwd, &auth)?;
    for change in &request.drafts {
        match change.op.as_str() {
            "file.put" => {
                let text = change.text.clone().unwrap_or_default();
                let blob = run_git_bytes(
                    &["hash-object", "-w", "--stdin"],
                    cwd,
                    &auth,
                    text.as_bytes(),
                )?;
                let blob = String::from_utf8_lossy(&blob).trim().to_owned();
                run_git(
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{}", change.path),
                    ],
                    cwd,
                    &auth,
                )?;
            }
            "file.move" => {
                let to = change.to.clone().ok_or("a move without a destination")?;
                let blob = change
                    .base
                    .clone()
                    .ok_or("a move of a file that is not on main")?;
                run_git(
                    &["update-index", "--force-remove", &change.path],
                    cwd,
                    &auth,
                )?;
                run_git(
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{to}"),
                    ],
                    cwd,
                    &auth,
                )?;
            }
            "file.delete" => {
                run_git(
                    &["update-index", "--force-remove", &change.path],
                    cwd,
                    &auth,
                )?;
            }
            // An asset's bytes are a media blob, not op content: the caller
            // fetched and sha-verified them before this ran, so here they are
            // just bytes to hash into the tree. A missing entry is a bug in
            // the caller, named rather than silently committing nothing.
            "asset.put" => {
                let sha256 = change
                    .sha256
                    .as_deref()
                    .ok_or("an asset.put without a sha256")?;
                let bytes = assets.get(sha256).ok_or_else(|| {
                    format!("the bytes of blob {} were never fetched", short(sha256))
                })?;
                let blob = run_git_bytes(&["hash-object", "-w", "--stdin"], cwd, &auth, bytes)?;
                let blob = String::from_utf8_lossy(&blob).trim().to_owned();
                run_git(
                    &[
                        "update-index",
                        "--add",
                        "--cacheinfo",
                        &format!("100644,{blob},{}", change.path),
                    ],
                    cwd,
                    &auth,
                )?;
            }
            other => return Err(format!("unknown draft op {other:?}")),
        }
    }
    let tree = run_git(&["write-tree"], cwd, &auth)?.trim().to_owned();

    // Validate the materialized tree before anything leaves this computer.
    let validate_dir = scratch.join("validate");
    std::fs::create_dir_all(&validate_dir)
        .map_err(|error| format!("could not create {}: {error}", validate_dir.display()))?;
    let prefix = format!("--prefix={}/", validate_dir.display());
    run_git(&["checkout-index", "-a", "-f", &prefix], cwd, &auth)?;
    let project = request.project_ref.clone();
    let mut actions_parser = move |text: &str| -> Result<usize, String> {
        buzz_workflow_pkg::actions_file::parse_actions_yml(text, &project)
            .map(|entries| entries.len())
            .map_err(|error| error.to_string())
    };
    let actions = match validate_root(&validate_dir, catalog, Some(&mut actions_parser)) {
        Ok(report) => match report.actions {
            ActionsCheck::Absent => "absent".to_owned(),
            ActionsCheck::Checked(n) => format!("checked ({n} actions)"),
            ActionsCheck::NotChecked(reason) => format!("not checked: {reason}"),
        },
        Err(tree_refusals) => {
            return Ok(refused(
                Some(tip.to_owned()),
                tree_refusals
                    .into_iter()
                    .map(|refusal| AgentsRepoCommitRefusal {
                        path: Some(refusal.path),
                        code: "invalid-tree".into(),
                        message: refusal.reason,
                    })
                    .collect(),
                committer,
                draft_ids,
            ));
        }
    };

    // The message, with every author's trailer.
    let mut message = request.message.trim().to_owned();
    if message.is_empty() {
        let paths: Vec<&str> = request.drafts.iter().map(|d| d.path.as_str()).collect();
        message = format!("docs(agents): {}", paths.join(", "));
    }
    let reasons: Vec<&str> = request
        .drafts
        .iter()
        .filter_map(|d| d.message.as_deref())
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .collect();
    if !reasons.is_empty() {
        message.push_str("\n\n");
        for reason in reasons {
            message.push_str("- ");
            message.push_str(reason);
            message.push('\n');
        }
    }
    message.push('\n');
    if !message.ends_with("\n\n") {
        message.push('\n');
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut author_pubkeys: Vec<&str> = request.drafts.iter().map(|d| d.author.as_str()).collect();
    author_pubkeys.sort_unstable();
    author_pubkeys.dedup();
    for pubkey in author_pubkeys {
        let short_key: String = pubkey.chars().take(8).collect();
        let email = format!("{short_key}@beekeeper.local");
        if email == committer.1 || !seen.insert(email.clone()) {
            continue;
        }
        let name = request
            .authors
            .iter()
            .find(|a| a.pubkey == pubkey)
            .and_then(|a| a.name.clone())
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("Beekeeper {short_key}"));
        message.push_str(&format!("Co-authored-by: {name} <{email}>\n"));
    }
    message.push_str(&format!("Beekeeper-Drafts: {}\n", draft_ids.join(",")));
    message.push_str(&format!(
        "Signed-off-by: {} <{}>\n",
        committer.0, committer.1
    ));
    let commit = run_git_bytes(
        &["commit-tree", &tree, "-p", tip, "-F", "-"],
        cwd,
        &auth,
        message.as_bytes(),
    )?;
    let commit = String::from_utf8_lossy(&commit).trim().to_owned();
    if commit.len() != 40 {
        return Err("git did not return a commit id".into());
    }
    let paths: Vec<AgentsRepoCommittedPath> = run_git(
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            tip,
            &commit,
        ],
        cwd,
        &auth,
    )?
    .lines()
    .filter_map(|line| {
        let (status, path) = line.split_once('\t')?;
        Some(AgentsRepoCommittedPath {
            path: path.to_owned(),
            status: status.chars().take(1).collect(),
        })
    })
    .collect();

    // Push under a lease on the tip this run read.
    let lease = format!("--force-with-lease=refs/heads/{}:{tip}", repo.branch);
    let refspec = format!("{commit}:refs/heads/{}", repo.branch);
    let push = run_git_status(
        &[
            "push",
            "--quiet",
            "--porcelain",
            &lease,
            "--",
            &repo.clone_url,
            &refspec,
        ],
        cwd,
        &remote_auth,
    )?;
    if !push.status.success() {
        let text = format!("{}\n{}", push.stdout, push.stderr);
        let (code, message) = if text.contains("stale info")
            || (text.contains("rejected") && text.contains("lease"))
        {
            (
                "lease-rejected",
                "someone pushed to main during the commit; nothing was pushed — reload and commit again".to_owned(),
            )
        } else {
            (
                "push-refused",
                format!("the relay refused the push: {}", push.stderr.trim()),
            )
        };
        return Ok(refused(
            Some(tip.to_owned()),
            vec![AgentsRepoCommitRefusal {
                path: None,
                code: code.into(),
                message,
            }],
            committer,
            draft_ids,
        ));
    }

    let verify = run_git(
        &[
            "ls-remote",
            "--",
            &repo.clone_url,
            &format!("refs/heads/{}", repo.branch),
        ],
        cwd,
        &remote_auth,
    );
    let (pushed, unknown_reason) = match verify {
        Ok(listing) if listing.trim().starts_with(&commit) => ("yes", None),
        Ok(listing) => (
            "unknown",
            Some(format!(
                "the push reported success but ls-remote shows main at {}; reload before retrying",
                listing.split_whitespace().next().unwrap_or("?")
            )),
        ),
        Err(error) => (
            "unknown",
            Some(format!(
                "the push ran but its result could not be confirmed: {error}"
            )),
        ),
    };
    Ok(AgentsRepoCommitResult {
        pushed: pushed.into(),
        tip_before: Some(tip.to_owned()),
        commit: Some(commit),
        tree: Some(tree),
        paths,
        actions: Some(actions),
        refusals: Vec::new(),
        unknown_reason,
        committer_name: committer.0.clone(),
        committer_email: committer.1.clone(),
        draft_ids,
    })
}

#[cfg(test)]
#[path = "agents_repo_commit_tests.rs"]
mod tests;
