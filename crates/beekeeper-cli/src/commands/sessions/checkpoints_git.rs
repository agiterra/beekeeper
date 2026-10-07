//! The git half of `bee sessions diff`: finding the checkout this computer
//! records for a session, and diffing two checkpoint trees inside it,
//! read-only. Split from `checkpoints.rs` for the 1000-line file cap.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use serde::Deserialize;

use super::{DiffUnavailable, REMOTE_OBJECTS_SENTENCE};
use crate::commands::sessions::worktree::{git_command, load_store};

/// Largest patch printed, in bytes. Past it the patch is cut at a line
/// boundary and `patchTruncatedBytes` says how much was left out.
pub const MAX_PATCH_BYTES: usize = 8 * 1024 * 1024;

// ── Diff: finding a checkout ────────────────────────────────────────────────

/// The slice of the host's `coding-session-workdirs.json` the diff reads.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkdirRecord {
    #[serde(default)]
    worktrees: BTreeMap<String, WorkdirSeat>,
    #[serde(default)]
    by_channel: BTreeMap<String, WorkdirEntry>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkdirSeat {
    path: PathBuf,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct WorkdirEntry {
    path: PathBuf,
}

/// Which recorded directory a diff ran in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkout {
    /// The directory git runs in.
    pub dir: PathBuf,
    /// `explicit`, `seat_worktree` or `channel_checkout`.
    pub source: &'static str,
}

/// Find the checkout the host records for one execution.
///
/// The seat worktree whose `sessionId` is the execution's (and, when the
/// umbrella is known, whose key starts with `<sessionRef>/`) answers first;
/// otherwise the directory a create in this channel resolves to. A record
/// whose directory is gone does not count. The object check that follows
/// decides whether the directory really holds the turn.
///
/// # Errors
/// The record exists but could not be read or has an unsupported version —
/// a reason, not "no checkout".
pub fn resolve_checkout(
    store: &Path,
    session_id: &str,
    session_ref: Option<&str>,
    channel: &str,
) -> Result<Option<Checkout>, String> {
    if !store.is_file() {
        return Ok(None);
    }
    load_store(store).map_err(|error| error.to_string())?;
    let text = std::fs::read_to_string(store)
        .map_err(|error| format!("failed to read {}: {error}", store.display()))?;
    let record: WorkdirRecord = serde_json::from_str(&text)
        .map_err(|error| format!("failed to parse {}: {error}", store.display()))?;
    let prefix = session_ref.map(|session_ref| format!("{session_ref}/"));
    let seat = record.worktrees.iter().find(|(key, seat)| {
        prefix
            .as_deref()
            .is_none_or(|prefix| key.starts_with(prefix))
            && seat.session_id.as_deref().map(str::trim) == Some(session_id)
            && seat.path.is_dir()
    });
    if let Some((_, seat)) = seat {
        return Ok(Some(Checkout {
            dir: seat.path.clone(),
            source: "seat_worktree",
        }));
    }
    Ok(record
        .by_channel
        .get(channel)
        .filter(|entry| entry.path.is_dir())
        .map(|entry| Checkout {
            dir: entry.path.clone(),
            source: "channel_checkout",
        }))
}

// ── Diff: running git ───────────────────────────────────────────────────────

/// A read-only git invocation in `dir`: hooks at `/dev/null`, no fsmonitor,
/// no optional locks, no prompt, no stdin.
fn read_only_git(dir: &Path, args: &[&str]) -> std::process::Command {
    let mut command = git_command(dir);
    command
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    command
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// Whether `oid` is a tree in `dir`: `Ok(false)` when the object is absent,
/// an error when it is present but not a tree or git could not answer.
fn tree_present(dir: &Path, oid: &str) -> Result<bool, DiffUnavailable> {
    let output = read_only_git(dir, &["cat-file", "-e", oid])
        .output()
        .map_err(|error| DiffUnavailable::new("GIT_FAILED", format!("git did not run: {error}")))?;
    match output.status.code() {
        Some(0) => {}
        Some(1) => return Ok(false),
        _ => {
            return Err(DiffUnavailable::new(
                "GIT_FAILED",
                format!(
                    "git could not check object {oid}: {}",
                    first_line(&output.stderr)
                ),
            ))
        }
    }
    let peeled = format!("{oid}^{{tree}}");
    let output = read_only_git(dir, &["cat-file", "-e", &peeled])
        .output()
        .map_err(|error| DiffUnavailable::new("GIT_FAILED", format!("git did not run: {error}")))?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(DiffUnavailable::new(
            "NOT_A_TREE",
            format!("object {oid} is not a tree"),
        ))
    }
}

/// The patch between two trees, or why there is none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffPatch {
    /// The patch text, possibly cut at [`MAX_PATCH_BYTES`].
    pub patch: String,
    /// Bytes left out when the patch was cut.
    pub truncated_bytes: usize,
}

/// Diff two tree ids inside `dir`, read-only.
///
/// # Errors
/// A [`DiffUnavailable`]: `OBJECTS_MISSING` (with the ids), `NOT_A_TREE`, or
/// `GIT_FAILED`.
pub fn diff_trees_in(dir: &Path, from: &str, to: &str) -> Result<DiffPatch, DiffUnavailable> {
    let mut missing = Vec::new();
    for oid in [from, to] {
        if !missing.iter().any(|seen: &String| seen == oid) && !tree_present(dir, oid)? {
            missing.push(oid.to_owned());
        }
    }
    if !missing.is_empty() {
        return Err(DiffUnavailable {
            code: "OBJECTS_MISSING",
            sentence: format!(
                "This computer's checkout does not hold {}. {REMOTE_OBJECTS_SENTENCE}",
                if missing.len() == 1 {
                    "one of the trees"
                } else {
                    "either tree"
                }
            ),
            missing,
        });
    }
    let output = read_only_git(
        dir,
        &[
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            from,
            to,
        ],
    )
    .output()
    .map_err(|error| DiffUnavailable::new("GIT_FAILED", format!("git did not run: {error}")))?;
    if !output.status.success() {
        return Err(DiffUnavailable::new(
            "GIT_FAILED",
            format!("git diff failed: {}", first_line(&output.stderr)),
        ));
    }
    let mut patch = String::from_utf8_lossy(&output.stdout).into_owned();
    let mut truncated_bytes = 0;
    if patch.len() > MAX_PATCH_BYTES {
        let mut cut = MAX_PATCH_BYTES;
        while !patch.is_char_boundary(cut) {
            cut -= 1;
        }
        let cut = patch[..cut].rfind('\n').map_or(cut, |line| line + 1);
        truncated_bytes = patch.len() - cut;
        patch.truncate(cut);
    }
    Ok(DiffPatch {
        patch,
        truncated_bytes,
    })
}

/// Clean a tree id as the wire carries it; anything else never reaches git.
pub(super) fn clean_oid(value: &str) -> Option<&str> {
    (matches!(value.len(), 40 | 64)
        && value
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)))
    .then_some(value)
}
