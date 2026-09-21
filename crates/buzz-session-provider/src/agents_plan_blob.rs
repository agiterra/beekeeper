//! Read one blob of the project's agents repository **at a named commit**.
//!
//! Beside [`crate::agents_checkout`] and deliberately not part of it. That
//! module answers a different question — "what does the published definition
//! say *now*" — and answers it from the fetched tip, which is right for
//! `actions.yml` and `team.yml` because a host step must run the current
//! definition. A **contract** is the opposite case: a work declaration pins
//! `planRef.commit`, and quoting a criterion from any other commit would be
//! quoting text the session never adopted. So this reader takes the commit
//! from the caller and never resolves one of its own.
//!
//! It also never fetches. The commit either is in this host's clone or it is
//! not, and a brief that stalled on a network round trip would delay the turn
//! it is supposed to make cheaper. An absent commit is an ordinary answer with
//! a reason, which the brief prints as `plan text unavailable (<reason>)`.

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;

use crate::agents_checkout::AgentsRepoRecord;

/// How long the one `git show` may take.
///
/// Short on purpose: no fetch happens here, so this is a local object read,
/// and anything slower than a couple of seconds is a sick repository rather
/// than a slow network.
const GIT_SHOW_TIMEOUT: Duration = Duration::from_secs(10);

/// Largest blob this reader will return.
///
/// The plan-file ceiling the contract already sets
/// ([`buzz_core::project_plan::MAX_PLAN_FILE_BYTES`]); a blob larger than that
/// could not be a valid plan, and reading it into memory to discover so is
/// work nobody asked for.
pub const MAX_BLOB_BYTES: usize = buzz_core::project_plan::MAX_PLAN_FILE_BYTES;

/// Why a blob could not be read at a commit. Each variant is a sentence the
/// brief prints verbatim inside `plan text unavailable (…)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanBlobError {
    /// The recorded path is not a git repository this host can read.
    NoCheckout,
    /// The caller's commit is not a full lowercase object id.
    MalformedCommit,
    /// The caller's path could escape the repository, or is not a plan path.
    UnsafePath,
    /// The commit is not in this host's clone.
    CommitMissing,
    /// The commit is present but holds no such file.
    BlobMissing,
    /// The blob is larger than [`MAX_BLOB_BYTES`].
    TooLarge,
    /// Git failed some other way, in its own bounded words.
    Git(String),
}

impl std::fmt::Display for PlanBlobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCheckout => write!(f, "this host has no readable clone of that repository"),
            Self::MalformedCommit => write!(f, "the declaration's commit is not a full object id"),
            Self::UnsafePath => write!(f, "the declaration's path is not a plan path"),
            Self::CommitMissing => write!(f, "that commit is not in this host's clone"),
            Self::BlobMissing => write!(f, "that commit holds no such file"),
            Self::TooLarge => write!(f, "that blob is larger than {MAX_BLOB_BYTES} bytes"),
            Self::Git(detail) => write!(f, "git failed: {detail}"),
        }
    }
}

/// Whether a path may be handed to `git show` as `<commit>:<path>`.
///
/// Stricter than git's own rules and deliberately so: the only paths this
/// reader is ever asked for are the contract's plan files, whose grammar
/// `buzz_core::project_plan::validate_plan_path` already froze. Anything
/// absolute, anything with a `..` component, anything with a backslash or a
/// control byte, and anything that could be read as a git revision rather than
/// a path is refused rather than escaped.
#[must_use]
pub fn is_safe_blob_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= buzz_core::project_plan::MAX_PLAN_PATH_BYTES
        && !path.starts_with('/')
        && !path.starts_with('-')
        && !path.contains('\\')
        && !path.contains(':')
        && !path.split('/').any(|part| part.is_empty() || part == "..")
        && path.bytes().all(|byte| !byte.is_ascii_control())
}

/// Whether a value is a full lowercase git object id.
fn is_full_object_id(value: &str) -> bool {
    (value.len() == 40 || value.len() == 64)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Read `<commit>:<path>` from this host's clone of the agents repository.
///
/// # Errors
/// [`PlanBlobError`], one variant per reason, every one of them a sentence the
/// caller prints rather than a failure it hides.
pub async fn read_blob_at_commit(
    record: &AgentsRepoRecord,
    commit: &str,
    path: &str,
) -> Result<String, PlanBlobError> {
    if !record.path.join(".git").exists() {
        return Err(PlanBlobError::NoCheckout);
    }
    let commit = commit.to_ascii_lowercase();
    if !is_full_object_id(&commit) {
        return Err(PlanBlobError::MalformedCommit);
    }
    if !is_safe_blob_path(path) {
        return Err(PlanBlobError::UnsafePath);
    }
    // `--` and `cat-file -e` first: asking whether the commit is here at all
    // separates "we never fetched that revision" from "the plan file was
    // renamed", and those two send a reader to two different places.
    let present = git(
        &record.path,
        &["cat-file", "-e", &format!("{commit}^{{commit}}")],
    )
    .await;
    if present.is_err() {
        return Err(PlanBlobError::CommitMissing);
    }
    let spec = format!("{commit}:{path}");
    match git(&record.path, &["show", "--no-textconv", &spec]).await {
        Ok(text) if text.len() > MAX_BLOB_BYTES => Err(PlanBlobError::TooLarge),
        Ok(text) => Ok(text),
        Err(detail)
            if detail.contains("does not exist")
                || detail.contains("exists on disk, but not in")
                || detail.contains("unknown revision or path") =>
        {
            Err(PlanBlobError::BlobMissing)
        }
        Err(detail) => Err(PlanBlobError::Git(bounded(&detail))),
    }
}

/// Keep git's words useful without letting them grow the brief.
fn bounded(detail: &str) -> String {
    let flat = detail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let trimmed = flat.trim();
    if trimmed.len() <= 200 {
        return trimmed.to_owned();
    }
    let mut end = 200;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &trimmed[..end])
}

/// One git invocation in `dir`, hermetic and never prompting.
async fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.args(args)
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let output = tokio::time::timeout(GIT_SHOW_TIMEOUT, cmd.output())
        .await
        .map_err(|_| format!("git {} timed out", args.join(" ")))?
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .await
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    #[test]
    fn a_path_that_could_escape_or_become_a_revision_is_refused() {
        assert!(is_safe_blob_path("plans/kettle.md"));
        for hostile in [
            "",
            "/etc/passwd",
            "../../etc/passwd",
            "plans/../../etc/passwd",
            "plans\\kettle.md",
            "HEAD:plans/kettle.md",
            "--output=/tmp/x",
            "plans//kettle.md",
        ] {
            assert!(!is_safe_blob_path(hostile), "{hostile:?} must be refused");
        }
    }

    #[tokio::test]
    async fn the_blob_comes_from_the_named_commit_never_a_later_one() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("agents");
        std::fs::create_dir_all(repo.join("plans")).unwrap();
        sh(&repo, &["init", "--quiet", "--initial-branch", "main"]).await;
        std::fs::write(repo.join("plans/kettle.md"), "first\n").unwrap();
        sh(&repo, &["add", "--all"]).await;
        sh(&repo, &["commit", "--quiet", "-m", "one"]).await;
        let first = sh(&repo, &["rev-parse", "HEAD"]).await;
        std::fs::write(repo.join("plans/kettle.md"), "second\n").unwrap();
        sh(&repo, &["commit", "--quiet", "-am", "two"]).await;

        let record = AgentsRepoRecord {
            path: repo.clone(),
            ref_name: "refs/heads/main".into(),
        };
        // The pinned commit, not the tip, and not the working copy.
        std::fs::write(repo.join("plans/kettle.md"), "scribble\n").unwrap();
        assert_eq!(
            read_blob_at_commit(&record, &first, "plans/kettle.md")
                .await
                .unwrap(),
            "first\n"
        );
        assert_eq!(
            read_blob_at_commit(&record, &first, "plans/missing.md").await,
            Err(PlanBlobError::BlobMissing)
        );
        assert_eq!(
            read_blob_at_commit(&record, &"a".repeat(40), "plans/kettle.md").await,
            Err(PlanBlobError::CommitMissing)
        );
        assert_eq!(
            read_blob_at_commit(&record, "HEAD", "plans/kettle.md").await,
            Err(PlanBlobError::MalformedCommit)
        );
        assert_eq!(
            read_blob_at_commit(&record, &first, "../../etc/passwd").await,
            Err(PlanBlobError::UnsafePath)
        );
        let nowhere = AgentsRepoRecord {
            path: tmp.path().join("nowhere"),
            ref_name: "refs/heads/main".into(),
        };
        assert_eq!(
            read_blob_at_commit(&nowhere, &first, "plans/kettle.md").await,
            Err(PlanBlobError::NoCheckout)
        );
    }
}
