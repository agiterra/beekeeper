//! The git half of `bee agents-repo commit`: turn a set of open draft heads
//! into one commit on the agents repository's `main`, or refuse naming why.
//!
//! Everything here is a subprocess over a throwaway clone, the way
//! `seed_packs_repository` works, so auth is whatever credential helper git
//! already has (`just install-git-credentials`). Nothing is pushed until
//! the tree is validated by `buzz_persona::agents_repo::validate_root`, and
//! the push is a `--force-with-lease` on the tip this run read, so a push
//! that raced someone else's lands nothing and says so.

use std::path::{Path, PathBuf};
use std::process::Command;

use buzz_persona::agents_repo::{validate_root, ActionsCheck};
use buzz_persona::template::TemplateCatalog;
use serde::Serialize;

use crate::error::CliError;

/// One draft head to apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraftChange {
    /// The draft op's event id.
    pub id: String,
    /// Its author (64 hex).
    pub author: String,
    /// `file.put`, `file.move` or `file.delete`.
    pub op: String,
    /// The path the op names.
    pub path: String,
    /// A move's destination.
    pub to: Option<String>,
    /// A put's text.
    pub text: Option<String>,
    /// The blob the author started from, or `None` for a new file.
    pub base: Option<String>,
    /// The author's one-line reason.
    pub message: Option<String>,
}

/// A name and e-mail for a commit trailer or identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    /// Display name.
    pub name: String,
    /// E-mail.
    pub email: String,
}

/// Everything one commit run needs.
pub struct CommitRequest<'a> {
    /// The relay clone URL (or a local path in tests).
    pub remote: &'a str,
    /// The `main` tip the caller showed the user; a moved `main` refuses.
    pub expected_tip: Option<&'a str>,
    /// The heads to apply.
    pub changes: &'a [DraftChange],
    /// The commit subject.
    pub message: &'a str,
    /// The committer.
    pub committer: &'a Identity,
    /// `Co-authored-by:` trailers, one per draft author.
    pub coauthors: &'a [Identity],
    /// The shipped templates the roles compose against.
    pub catalog: &'a TemplateCatalog,
    /// The project coordinate `actions.yml` entries must bind to.
    pub project: &'a str,
}

/// One path that refused, with a stable code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommitRefusal {
    /// The path, or null for a whole-run refusal.
    pub path: Option<String>,
    /// `main-moved`, `no-main`, `stale-base`, `invalid-tree`, `lease-rejected`,
    /// `push-refused`.
    pub code: &'static str,
    /// The sentence.
    pub message: String,
}

/// One path the commit touched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CommittedPath {
    /// The path.
    pub path: String,
    /// `A`, `M`, `D` or `R` as `git diff-tree` reports it.
    pub status: String,
}

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "pushed", rename_all = "lowercase")]
pub enum CommitOutcome {
    /// The commit is on `main`, verified by `ls-remote`.
    Yes {
        /// `main` before.
        tip_before: String,
        /// The new commit.
        commit: String,
        /// Its tree.
        tree: String,
        /// What changed.
        paths: Vec<CommittedPath>,
        /// How `actions.yml` was checked.
        actions: String,
    },
    /// Nothing was pushed.
    No {
        /// `main` before, when it was read.
        tip_before: Option<String>,
        /// Every refusal.
        refusals: Vec<CommitRefusal>,
    },
    /// The push ran but its result could not be confirmed; `main` may or
    /// may not have moved.
    Unknown {
        /// `main` before.
        tip_before: String,
        /// The commit that was pushed.
        commit: String,
        /// Why the verify failed.
        reason: String,
    },
}

struct Git {
    work: PathBuf,
}

impl Git {
    fn run(&self, args: &[&str]) -> Result<String, CliError> {
        let output = self
            .command()
            .args(args)
            .output()
            .map_err(|error| CliError::Other(format!("could not run git {args:?}: {error}")))?;
        if !output.status.success() {
            return Err(CliError::Other(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn run_stdin(&self, args: &[&str], stdin: &[u8]) -> Result<String, CliError> {
        use std::io::Write;
        let mut child = self
            .command()
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| CliError::Other(format!("could not run git {args:?}: {error}")))?;
        if let Some(mut pipe) = child.stdin.take() {
            pipe.write_all(stdin)
                .map_err(|error| CliError::Other(format!("git {args:?} stdin: {error}")))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| CliError::Other(format!("git {args:?}: {error}")))?;
        if !output.status.success() {
            return Err(CliError::Other(format!(
                "git {args:?} failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn command(&self) -> Command {
        crate::commands::sessions::worktree::git_command(&self.work)
    }

    /// `rev-parse --verify` that answers `None` for an unknown object.
    fn resolve(&self, spec: &str) -> Option<String> {
        let output = self
            .command()
            .args(["rev-parse", "--verify", "--quiet", spec])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let sha = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (sha.len() == 40).then_some(sha)
    }
}

/// Apply `request.changes` on top of the remote's `main` and push.
pub fn commit_drafts(request: &CommitRequest<'_>) -> Result<CommitOutcome, CliError> {
    let scratch = std::env::temp_dir().join(format!(
        "bee-agents-repo-commit-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let work = scratch.join("work");
    std::fs::create_dir_all(&work).map_err(|error| {
        CliError::Other(format!("could not create {}: {error}", work.display()))
    })?;
    let outcome = commit_in(&scratch, &work, request);
    std::fs::remove_dir_all(&scratch).ok();
    outcome
}

fn commit_in(
    scratch: &Path,
    work: &Path,
    request: &CommitRequest<'_>,
) -> Result<CommitOutcome, CliError> {
    let git = Git {
        work: work.to_path_buf(),
    };
    git.run(&[
        "clone",
        "--quiet",
        "--no-checkout",
        "--depth",
        "1",
        "--branch",
        "main",
        "--",
        request.remote,
        ".",
    ])
    .map_err(|error| {
        CliError::Other(format!(
            "{error}\nIf this is an authentication failure, run `just install-git-credentials` \
             (or `bee git setup`) so git can answer the relay's NIP-98 challenge."
        ))
    })?;
    let Some(tip) = git.resolve("refs/remotes/origin/main") else {
        return Ok(CommitOutcome::No {
            tip_before: None,
            refusals: vec![CommitRefusal {
                path: None,
                code: "no-main",
                message: "the agents repository has no refs/heads/main".into(),
            }],
        });
    };
    if let Some(expected) = request.expected_tip {
        if expected != tip {
            return Ok(CommitOutcome::No {
                tip_before: Some(tip.clone()),
                refusals: vec![CommitRefusal {
                    path: None,
                    code: "main-moved",
                    message: format!(
                        "main moved from {} to {} since these drafts were reviewed; reload and commit again",
                        &expected[..8.min(expected.len())],
                        &tip[..8]
                    ),
                }],
            });
        }
    }

    // Base checks: every head must still sit on the blob it was made from.
    let mut refusals = Vec::new();
    for change in request.changes {
        let at_tip = git.resolve(&format!("{tip}:{}", change.path));
        if at_tip != change.base {
            refusals.push(CommitRefusal {
                path: Some(change.path.clone()),
                code: "stale-base",
                message: match (&change.base, &at_tip) {
                    (None, Some(_)) => format!(
                        "{} exists on main now, but this draft was written as a new file; reload it and re-apply the draft",
                        change.path
                    ),
                    (Some(_), None) => format!(
                        "{} is no longer on main; this draft by {} was based on a file that was since removed",
                        change.path,
                        &change.author[..8]
                    ),
                    _ => format!(
                        "main changed {} after this draft by {} was based on it; reload the file and re-apply the draft — nothing was pushed",
                        change.path,
                        &change.author[..8]
                    ),
                },
            });
        }
        if change.op == "file.move" {
            if let Some(to) = &change.to {
                if git.resolve(&format!("{tip}:{to}")).is_some() {
                    refusals.push(CommitRefusal {
                        path: Some(to.clone()),
                        code: "stale-base",
                        message: format!(
                            "{to} already exists on main; the move of {} cannot land over it",
                            change.path
                        ),
                    });
                }
            }
        }
    }
    if !refusals.is_empty() {
        return Ok(CommitOutcome::No {
            tip_before: Some(tip),
            refusals,
        });
    }

    // Build the tree from the tip's index plus the changes.
    git.run(&["read-tree", &tip])?;
    for change in request.changes {
        match change.op.as_str() {
            "file.put" => {
                let text = change.text.clone().unwrap_or_default();
                let blob = git.run_stdin(&["hash-object", "-w", "--stdin"], text.as_bytes())?;
                git.run(&[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("100644,{blob},{}", change.path),
                ])?;
            }
            "file.move" => {
                let to = change
                    .to
                    .clone()
                    .ok_or_else(|| CliError::Other("a move without a destination".into()))?;
                let blob = change.base.clone().ok_or_else(|| {
                    CliError::Other("a move of a file that is not on main".into())
                })?;
                git.run(&["update-index", "--force-remove", &change.path])?;
                git.run(&[
                    "update-index",
                    "--add",
                    "--cacheinfo",
                    &format!("100644,{blob},{to}"),
                ])?;
            }
            "file.delete" => {
                git.run(&["update-index", "--force-remove", &change.path])?;
            }
            other => {
                return Err(CliError::Other(format!("unknown draft op {other:?}")));
            }
        }
    }
    let tree = git.run(&["write-tree"])?;

    // Validate the tree before anything leaves this machine.
    let validate_dir = scratch.join("validate");
    std::fs::create_dir_all(&validate_dir).map_err(|error| {
        CliError::Other(format!(
            "could not create {}: {error}",
            validate_dir.display()
        ))
    })?;
    let prefix = format!("{}/", validate_dir.display());
    git.run(&["checkout-index", "-a", "-f", &format!("--prefix={prefix}")])?;
    let project = request.project.to_owned();
    let mut actions_parser = move |text: &str| -> Result<usize, String> {
        buzz_workflow::actions_file::parse_actions_yml(text, &project)
            .map(|entries| entries.len())
            .map_err(|error| error.to_string())
    };
    let actions = match validate_root(&validate_dir, request.catalog, Some(&mut actions_parser)) {
        Ok(report) => match report.actions {
            ActionsCheck::Absent => "absent".to_owned(),
            ActionsCheck::Checked(n) => format!("checked ({n} actions)"),
            ActionsCheck::NotChecked(reason) => format!("not checked: {reason}"),
        },
        Err(tree_refusals) => {
            return Ok(CommitOutcome::No {
                tip_before: Some(tip),
                refusals: tree_refusals
                    .into_iter()
                    .map(|refusal| CommitRefusal {
                        path: Some(refusal.path),
                        code: "invalid-tree",
                        message: refusal.reason,
                    })
                    .collect(),
            });
        }
    };

    // The commit, with the committer's identity and every author's trailer.
    let mut message = String::from(request.message.trim());
    let reasons: Vec<&str> = request
        .changes
        .iter()
        .filter_map(|change| change.message.as_deref())
        .filter(|reason| !reason.trim().is_empty())
        .collect();
    if !reasons.is_empty() {
        message.push_str("\n\n");
        for reason in reasons {
            message.push_str("- ");
            message.push_str(reason.trim());
            message.push('\n');
        }
    }
    message.push('\n');
    if !message.ends_with("\n\n") {
        message.push('\n');
    }
    let mut seen = std::collections::BTreeSet::new();
    for author in request.coauthors {
        if author.email != request.committer.email && seen.insert(author.email.clone()) {
            message.push_str(&format!(
                "Co-authored-by: {} <{}>\n",
                author.name, author.email
            ));
        }
    }
    let ids: Vec<&str> = request.changes.iter().map(|c| c.id.as_str()).collect();
    message.push_str(&format!("Beekeeper-Drafts: {}\n", ids.join(",")));
    message.push_str(&format!(
        "Signed-off-by: {} <{}>\n",
        request.committer.name, request.committer.email
    ));
    let commit = {
        let output = git
            .command()
            .args(["commit-tree", &tree, "-p", &tip, "-F", "-"])
            .env("GIT_AUTHOR_NAME", &request.committer.name)
            .env("GIT_AUTHOR_EMAIL", &request.committer.email)
            .env("GIT_COMMITTER_NAME", &request.committer.name)
            .env("GIT_COMMITTER_EMAIL", &request.committer.email)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(mut pipe) = child.stdin.take() {
                    pipe.write_all(message.as_bytes())?;
                }
                child.wait_with_output()
            })
            .map_err(|error| CliError::Other(format!("git commit-tree: {error}")))?;
        if !output.status.success() {
            return Err(CliError::Other(format!(
                "git commit-tree failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    };
    let paths: Vec<CommittedPath> = git
        .run(&[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            &tip,
            &commit,
        ])?
        .lines()
        .filter_map(|line| {
            let (status, path) = line.split_once('\t')?;
            Some(CommittedPath {
                path: path.to_owned(),
                status: status.chars().take(1).collect(),
            })
        })
        .collect();

    // Push under a lease on the tip this run read.
    let lease = format!("--force-with-lease=refs/heads/main:{tip}");
    let refspec = format!("{commit}:refs/heads/main");
    let push = git
        .command()
        .args([
            "push",
            "--quiet",
            "--porcelain",
            &lease,
            "--",
            request.remote,
            &refspec,
        ])
        .output()
        .map_err(|error| CliError::Other(format!("could not run git push: {error}")))?;
    if !push.status.success() {
        let stderr = String::from_utf8_lossy(&push.stderr);
        let stdout = String::from_utf8_lossy(&push.stdout);
        let text = format!("{stdout}\n{stderr}");
        let (code, message) = if text.contains("stale info")
            || text.contains("rejected") && text.contains("lease")
        {
            (
                "lease-rejected",
                "someone pushed to main during the commit; nothing was pushed — reload and commit again".to_owned(),
            )
        } else {
            (
                "push-refused",
                format!("the relay refused the push: {}", stderr.trim()),
            )
        };
        return Ok(CommitOutcome::No {
            tip_before: Some(tip),
            refusals: vec![CommitRefusal {
                path: None,
                code,
                message,
            }],
        });
    }

    // Verify: the remote's main must now be the commit.
    match git.run(&["ls-remote", "--", request.remote, "refs/heads/main"]) {
        Ok(listing) if listing.starts_with(&commit) => Ok(CommitOutcome::Yes {
            tip_before: tip,
            commit,
            tree,
            paths,
            actions,
        }),
        Ok(listing) => Ok(CommitOutcome::Unknown {
            tip_before: tip,
            commit,
            reason: format!(
                "the push reported success but ls-remote shows main at {}; reload before retrying",
                listing.split_whitespace().next().unwrap_or("?")
            ),
        }),
        Err(error) => Ok(CommitOutcome::Unknown {
            tip_before: tip,
            commit,
            reason: format!("the push ran but its result could not be confirmed: {error}"),
        }),
    }
}
