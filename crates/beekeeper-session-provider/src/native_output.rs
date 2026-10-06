//! Reconcile stream-assembled command output with the agent's own record.
//!
//! codex-acp delivers a command's output only as streamed chunks, and Codex
//! starts streaming a command after it has spawned it: whatever the command
//! printed in between is never streamed, and codex-acp does not resend it at
//! completion once any chunk streamed (2026-10-03 canary: the first 7 of 22
//! lines missing from the published result). Codex does keep the whole output:
//! the `item_completed` record it writes to the session's rollout carries the
//! command's `aggregated_output`.
//!
//! The provider gives every bounded Codex execution its own private
//! `CODEX_HOME` (`execution_scope_runtime::prepare_codex_home`), so that
//! record is the session's own, inside the provider's execution state. This
//! module reads it there — never the operator's `~/.codex` — and upgrades a
//! `tool_result` the translator marked `outputComplete: false`:
//!
//! - the streamed bytes are a strict suffix of the recorded output → the
//!   content is replaced by the whole output (under the usual cap, with an
//!   honest elision digest), `contentSource: "native_rollout"`,
//!   `outputComplete: true`;
//! - they are the whole recorded output → `outputComplete: true`, content
//!   unchanged;
//! - anything else (no record, a record that disagrees, an unreadable or
//!   oversized one) → left unverified, with `outputGap.aggregatedBytes` when
//!   the record was read.
//!
//! Every read is bounded in size and in time and retried only briefly, because
//! the transcript waits on it: a missing record degrades to the honest
//! unverified form, never to a stalled transcript.

use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::transcript::{
    bound_text, StreamCheck, CONTENT_SOURCE_NATIVE_ROLLOUT, MAX_TOOL_CONTENT_BYTES,
};

/// How much of a rollout's end is read. The record of a command that just
/// finished is among the last lines; a single line longer than this is not
/// recovered.
const MAX_ROLLOUT_TAIL_BYTES: u64 = 8 * 1024 * 1024;
/// Rollout files considered, newest first. One session's home holds its own
/// threads only (the agent's, and any it spawned).
const MAX_ROLLOUT_FILES: usize = 4;
/// Attempts at finding the record: Codex may flush it just after the frame.
const ATTEMPTS: usize = 3;
/// Pause between attempts.
const RETRY_DELAY: Duration = Duration::from_millis(40);
/// Ceiling on one attempt's file work.
const ATTEMPT_TIMEOUT: Duration = Duration::from_millis(300);

/// Where a session's native command records live, when the provider owns them.
#[derive(Debug, Clone)]
pub(crate) struct NativeOutputSource {
    /// The execution's private `CODEX_HOME`.
    codex_home: PathBuf,
}

impl NativeOutputSource {
    /// The private Codex home of one bounded execution.
    pub(crate) fn codex(codex_home: PathBuf) -> Self {
        Self { codex_home }
    }
}

/// Verify each stream-assembled result in `items` against the session's own
/// record, rewriting it in place. A no-op without a source or without checks.
pub(crate) async fn reconcile(
    items: &mut [Value],
    checks: Vec<StreamCheck>,
    source: Option<&NativeOutputSource>,
) {
    let Some(source) = source else {
        return;
    };
    for check in checks {
        let Some(item) = items.iter_mut().find(|item| {
            item.get("kind").and_then(Value::as_str) == Some("tool_result")
                && item.get("toolId").and_then(Value::as_str) == Some(check.tool_id.as_str())
                && item.get("toolKind").and_then(Value::as_str) == Some("execute")
        }) else {
            continue;
        };
        let recorded = find_with_retry(&source.codex_home, &check.tool_id).await;
        apply(item, &check, recorded.as_deref());
    }
}

/// Look for the record a few times, each attempt bounded, then give up.
async fn find_with_retry(codex_home: &Path, tool_id: &str) -> Option<String> {
    for attempt in 0..ATTEMPTS {
        if attempt > 0 {
            tokio::time::sleep(RETRY_DELAY).await;
        }
        let home = codex_home.to_owned();
        let id = tool_id.to_owned();
        let read = tokio::task::spawn_blocking(move || aggregated_output(&home, &id));
        if let Ok(Ok(Some(found))) = tokio::time::timeout(ATTEMPT_TIMEOUT, read).await {
            return Some(found);
        }
    }
    None
}

/// Rewrite one unverified result given what the record says, if anything.
pub(crate) fn apply(item: &mut Value, check: &StreamCheck, recorded: Option<&str>) {
    let Some(object) = item.as_object_mut() else {
        return;
    };
    let Some(recorded) = recorded else {
        return;
    };
    let streamed = check.streamed.bytes;
    let suffix_matches = recorded.len() >= streamed && {
        let tail = &recorded.as_bytes()[recorded.len() - streamed..];
        <[u8; 32]>::from(Sha256::digest(tail)) == check.streamed.sha256
    };
    if !suffix_matches {
        object.insert(
            "outputGap".into(),
            json!({ "streamedBytes": streamed, "aggregatedBytes": recorded.len() }),
        );
        return;
    }
    object.remove("outputGap");
    object.insert("outputComplete".into(), json!(true));
    if recorded.len() > streamed {
        object.insert(
            "content".into(),
            json!(bound_text(recorded, MAX_TOOL_CONTENT_BYTES)),
        );
        object.insert("contentSource".into(), json!(CONTENT_SOURCE_NATIVE_ROLLOUT));
    }
}

/// The `aggregated_output` Codex recorded for command `tool_id`, from the
/// newest rollouts under `codex_home/sessions`.
pub(crate) fn aggregated_output(codex_home: &Path, tool_id: &str) -> Option<String> {
    newest_rollouts(&codex_home.join("sessions"))
        .iter()
        .find_map(|path| find_in_rollout(path, tool_id))
}

/// Rollout files under `sessions/YYYY/MM/DD/`, newest first, at most
/// [`MAX_ROLLOUT_FILES`].
fn newest_rollouts(sessions: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut dirs = vec![(sessions.to_owned(), 0usize)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            // Never follow a link out of the private home.
            if kind.is_dir() && depth < 3 {
                dirs.push((path, depth + 1));
            } else if kind.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("rollout-") && name.ends_with(".jsonl"))
            {
                let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
                found.push((modified, path));
            }
        }
    }
    found.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    found
        .into_iter()
        .take(MAX_ROLLOUT_FILES)
        .map(|(_, path)| path)
        .collect()
}

/// The record of `tool_id` in one rollout, scanning its tail newest-first.
fn find_in_rollout(path: &Path, tool_id: &str) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(MAX_ROLLOUT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_ROLLOUT_TAIL_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        // The first line was cut by the window.
        lines.remove(0);
    }
    lines
        .into_iter()
        .rev()
        .filter(|line| line.contains(tool_id))
        .find_map(|line| command_record(line, tool_id))
}

/// `aggregated_output` of a completed command item named `tool_id`.
fn command_record(line: &str, tool_id: &str) -> Option<String> {
    let value: Value = serde_json::from_str(line).ok()?;
    let payload = value.get("payload")?;
    if payload.get("type").and_then(Value::as_str) != Some("item_completed") {
        return None;
    }
    let item = payload.get("item")?;
    if item.get("id").and_then(Value::as_str) != Some(tool_id) {
        return None;
    }
    item.get("aggregated_output")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

#[cfg(test)]
#[path = "native_output_tests.rs"]
mod tests;
